// Real `lib/portage/elog/`. After every package merge, real
// `elog_process(cpv, settings)` reads the per-phase message files
// `bin/isolated-functions.sh::__elog_base` wrote under `${T}/logging/`,
// filters them by `PORTAGE_ELOG_CLASSES` (default `"log warn error"`),
// and hands them to each module named in `PORTAGE_ELOG_SYSTEM` (default
// `"save_summary:log,warn,error,qa echo"`).
//
// This module ports:
//   - `mod_echo` -- `collect` reads one package's `${T}/logging/`,
//     `echo_summary` prints the accumulated `* Messages for package
//     <cpv>:` blocks (real `finalize()`, an atexit handler).
//   - `mod_save` -- one `<logdir>/elog/[<cat>/]<pf>:<ts>.log` file per
//     package (`save_process`).
//   - `mod_save_summary` -- append every package's messages to a single
//     `<logdir>/elog/summary.log` (`save_summary_process`). This one is
//     ON by default (it's in `make.globals`'s `PORTAGE_ELOG_SYSTEM`).
//   - `mod_syslog` -- one real `syslog(3)` call per elog line, tagged
//     `"portage"`, facility `LOG_LOCAL5` (`syslog_process`).
//   - `mod_custom` -- always saves the file first (real, unconditional
//     `mod_save.process` call), then runs the real, unmodified
//     `$PORTAGE_ELOG_COMMAND` with `${LOGFILE}`/`${PACKAGE}` substituted
//     (`custom_process`).
//   - `mod_mail` -- one MIME mail per package, sent immediately via a
//     sendmail binary (`PORTAGE_ELOG_MAILURI` naming an absolute path)
//     or hand-rolled plain SMTP (AUTH LOGIN/PLAIN, dot-stuffing),
//     `mail_process`.
//   - `mod_mail_summary` -- accumulates every package's messages and
//     sends ONE multipart mail at process exit (real atexit `finalize`,
//     armed on first use), `mail_summary_accumulate` /
//     `mail_summary_finalize`.
//
// Portuale never deletes the builddir, so the driver (`pretend.rs`)
// re-scans each entry's `${T}/logging/` after the merge loop (and after
// the `emerge -C`/`--depclean`/`--prune` removal loop, filtered to the
// `prerm`/`postrm` phases -- real `dblink.unmerge`'s own
// `_elog_process(phasefilter=...)`) via `process_batch` -- no message
// buffer threaded through the (un)merge machinery.
//
// `PORTAGE_ELOG_CLASSES` / `PORTAGE_ELOG_SYSTEM` / the three
// `PORTAGE_ELOG_MAIL*` settings are read from the
// env only (no `make.conf`), defaulting to `make.globals`. The `logdir`
// is `$PORTAGE_LOGDIR` else `<root>/var/log/portage` -- root-relative, a
// deliberate divergence from real `mod_save`'s `<BROOT>/var/log/portage`
// (`BROOT` is `/`, needs privileges), matching portuale's other
// `<root>`-relative path choices for a relocatable tree. The real
// uid/gid/mode chmod dance on the log dir/files is a documented cut, like
// every other privilege-preserving `chown` in portuale.

use crate::color::Colorizer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Real `portage.const.EBUILD_PHASES`, in order -- `mod_echo._finalize`
/// walks this so messages print in phase order regardless of file mtime.
const EBUILD_PHASES: &[&str] = &[
    "pretend",
    "setup",
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
    "package",
    "instprep",
    "preinst",
    "postinst",
    "prerm",
    "postrm",
    "nofetch",
    "config",
    "info",
    "other",
];

/// One `<TYPE> <message>` line from a `${T}/logging/<phase>` file.
pub struct ElogMessage {
    /// Which `${T}/logging/<phase>` file this came from -- real
    /// `collect_ebuild_messages` keys on it, and `_combine_logentries`
    /// emits a `<LEVEL>: <phase>` header on every phase/level change.
    pub phase: String,
    /// `LOG` / `INFO` / `WARN` / `ERROR` / `QA`.
    pub level: String,
    pub text: String,
}

/// One package's worth of collected, class-filtered elog messages.
pub struct ElogPackage {
    /// `cat/pkg-ver` (real `mod_echo`'s `key`).
    pub cpv: String,
    /// The `ROOT` the package merged to (`/` renders the short header).
    pub root: String,
    pub messages: Vec<ElogMessage>,
}

fn elog_system_tokens() -> Vec<String> {
    std::env::var("PORTAGE_ELOG_SYSTEM")
        .unwrap_or_else(|_| "save_summary:log,warn,error,qa echo".to_string())
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Whether the module named `name` (`echo`, `save`, `save_summary`, …)
/// is listed in `PORTAGE_ELOG_SYSTEM` -- its token is either the bare
/// name or `name:levels`. `-` is accepted for `_` (real `elog_process`:
/// `s = s.replace("-", "_")`), so `save-summary` == `save_summary`.
pub fn module_enabled(name: &str) -> bool {
    elog_system_tokens()
        .iter()
        .any(|t| t.split(':').next().map(|m| m.replace('-', "_")).as_deref() == Some(name))
}

/// Whether the `echo` module is enabled (real default: yes).
pub fn echo_enabled() -> bool {
    module_enabled("echo")
}

/// `PORTAGE_ELOG_CLASSES` (real `make.globals` default `"log warn
/// error"`), uppercased.
fn portage_elog_classes() -> HashSet<String> {
    std::env::var("PORTAGE_ELOG_CLASSES")
        .unwrap_or_else(|_| "log warn error".to_string())
        .split_whitespace()
        .map(|c| c.to_uppercase())
        .collect()
}

/// The uppercased message classes module `name` shows -- its own
/// `name:levels` override in `PORTAGE_ELOG_SYSTEM` if present (real
/// `elog_process`'s per-module `filter_loglevels(..., levels)`), else
/// `PORTAGE_ELOG_CLASSES`.
fn module_classes(name: &str) -> HashSet<String> {
    for t in elog_system_tokens() {
        if let Some((m, levels)) = t.split_once(':')
            && m.replace('-', "_") == name
        {
            return levels
                .split(',')
                .map(|l| l.trim().to_uppercase())
                .filter(|l| !l.is_empty())
                .collect();
        }
    }
    portage_elog_classes()
}

/// Reads `${t_dir}/logging/*` for one package -- every `LOG`/`INFO`/
/// `WARN`/`ERROR`/`QA` line, in real `EBUILD_PHASES` order, unfiltered
/// (real `collect_ebuild_messages`). Empty when there is nothing there.
pub fn collect_all(t_dir: &Path) -> Vec<ElogMessage> {
    let logging = t_dir.join("logging");
    let mut out = Vec::new();
    for phase in EBUILD_PHASES {
        let Ok(content) = std::fs::read_to_string(logging.join(phase)) else {
            continue;
        };
        for line in content.split('\n') {
            if line.is_empty() {
                continue;
            }
            let Some((level, text)) = line.split_once(' ') else {
                continue;
            };
            if !matches!(level, "ERROR" | "INFO" | "LOG" | "QA" | "WARN") {
                continue;
            }
            out.push(ElogMessage {
                phase: (*phase).to_string(),
                level: level.to_string(),
                text: text.to_string(),
            });
        }
    }
    out
}

/// `collect_all`, restricted to the named phases -- real
/// `_elog_process(phasefilter=...)`, which `dblink.unmerge()` calls with
/// `("prerm", "postrm")` so a package's stale install-time `${T}/logging`
/// files (portuale never cleans the builddir) don't resurface on removal.
pub fn collect_all_phases(t_dir: &Path, phases: &[&str]) -> Vec<ElogMessage> {
    collect_all(t_dir)
        .into_iter()
        .filter(|m| phases.contains(&m.phase.as_str()))
        .collect()
}

/// `collect_all` filtered to `classes` (real `filter_loglevels`).
fn filter_by_classes<'a>(
    msgs: &'a [ElogMessage],
    classes: &HashSet<String>,
) -> Vec<&'a ElogMessage> {
    msgs.iter()
        .filter(|m| classes.contains("*") || classes.contains(&m.level))
        .collect()
}

/// `collect_all` filtered by the `echo` module's classes -- the shape
/// `echo_summary` consumes. `phases` optionally restricts which
/// `${T}/logging/<phase>` files are read (real `phasefilter`).
pub fn collect(t_dir: &Path, phases: Option<&[&str]>) -> Vec<ElogMessage> {
    let classes = module_classes("echo");
    let all = match phases {
        Some(p) => collect_all_phases(t_dir, p),
        None => collect_all(t_dir),
    };
    all.into_iter()
        .filter(|m| classes.contains("*") || classes.contains(&m.level))
        .collect()
}

/// Real `_combine_logentries`: one flat string, phases in `EBUILD_PHASES`
/// order, a `<LEVEL>: <phase>` header emitted whenever the (phase, level)
/// pair changes, a trailing blank line when anything was written.
/// `msgs` must already be in `collect_all`'s phase order.
fn combine_logentries(msgs: &[&ElogMessage]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut prev: Option<(&str, &str)> = None;
    for m in msgs {
        let cur = (m.phase.as_str(), m.level.as_str());
        if prev != Some(cur) {
            lines.push(format!("{}: {}", m.level, m.phase));
            prev = Some(cur);
        }
        lines.push(m.text.trim_end_matches('\n').to_string());
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.join("\n")
}

/// `$PORTAGE_LOGDIR` if set, else `<root>/var/log/portage` -- see this
/// module's own doc comment on the `<BROOT>` divergence.
pub fn logdir(root: &Path) -> PathBuf {
    std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("var/log/portage"))
}

/// UTC `%Y%m%d-%H%M%S` of the current time (real `mod_save`'s
/// `time.strftime(..., time.gmtime())`).
fn utc_stamp() -> String {
    utc_stamp_at(std::time::SystemTime::now())
}

/// UTC `%Y%m%d-%H%M%S` of an arbitrary `SystemTime` -- `utc_stamp`'s own
/// formatting, generalized so `emerge_build`'s own `PORTAGE_LOGDIR`
/// build-log naming can reuse it against a `.logid` marker file's mtime
/// (real `prepare_build_dirs.py`'s own `time.gmtime(os.stat(logid_path)
/// .st_mtime)`) instead of "now".
pub(crate) fn utc_stamp_at(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant's algorithm) -- no chrono dep.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

/// Real `mod_save.process`: write one package's `fulltext` to
/// `<logdir>/elog/<pf>:<utc-stamp>.log` (or, with `FEATURES=split-elog`,
/// `<logdir>/elog/<cat>/<pf>:<stamp>.log`; otherwise the `<cat>:` is
/// prefixed onto the filename). `key` is `cat/pkg-ver`. Returns the path
/// written. Skipped entirely by the caller when the package has no
/// class-filtered messages.
pub fn save_process(
    logdir: &Path,
    key: &str,
    fulltext: &str,
    split_elog: bool,
) -> Result<PathBuf, String> {
    let (cat, pf) = key.split_once('/').unwrap_or(("", key));
    let stamp = utc_stamp();
    let (subdir, filename) = if split_elog {
        (logdir.join("elog").join(cat), format!("{pf}:{stamp}.log"))
    } else {
        (logdir.join("elog"), format!("{cat}:{pf}:{stamp}.log"))
    };
    std::fs::create_dir_all(&subdir).map_err(|e| format!("{}: {e}", subdir.display()))?;
    let path = subdir.join(filename);
    std::fs::write(&path, fulltext).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Real `mod_save_summary.process`: append one package's block to
/// `<logdir>/elog/summary.log` -- a `>>> Messages generated by process
/// <pid> on <local-time> for package <key>:\n\n` header, then `fulltext`,
/// then `\n`. Portuale uses the same UTC stamp `mod_save` does (with a
/// `UTC` suffix) rather than real `time.localtime()` + `%Z`, for a
/// deterministic, timezone-independent line.
pub fn save_summary_process(logdir: &Path, key: &str, fulltext: &str) -> Result<PathBuf, String> {
    use std::io::Write as _;
    let elogdir = logdir.join("elog");
    std::fs::create_dir_all(&elogdir).map_err(|e| format!("{}: {e}", elogdir.display()))?;
    let path = elogdir.join("summary.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    write!(
        f,
        ">>> Messages generated by process {} on {} UTC for package {key}:\n\n{fulltext}\n",
        std::process::id(),
        utc_stamp(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Real `mod_syslog.py`'s own `_pri` map: `INFO`/`ERROR`/`LOG` each get
/// their own real syslog level; `WARN` and `QA` (a real, distinct level
/// -- see `ElogMessage::level`'s own vocabulary) both fall to
/// `LOG_WARNING`, matching real `_pri["QA"] = syslog.LOG_WARNING`. Split
/// out from `syslog_process` so the mapping is directly unit-testable
/// without any real `syslog(3)` call involved.
fn syslog_priority(level: &str) -> libc::c_int {
    match level {
        "INFO" => libc::LOG_INFO,
        "ERROR" => libc::LOG_ERR,
        "LOG" => libc::LOG_NOTICE,
        _ => libc::LOG_WARNING,
    }
}

/// Real `mod_syslog.process`'s own per-line format: `"{key}: {phase}:
/// {line}"`, trailing newline stripped (real `line.rstrip("\n")`). Split
/// out from `syslog_process` for the same direct-unit-test reason
/// `syslog_priority` is.
fn syslog_line(key: &str, msg: &ElogMessage) -> String {
    format!("{key}: {}: {}", msg.phase, msg.text.trim_end_matches('\n'))
}

/// Real `mod_syslog.process` (`elog/mod_syslog.py`): one `syslog(3)`
/// message per elog line, tagged `"portage"`, facility `LOG_LOCAL5`,
/// priority mapped from that line's own level (`syslog_priority`).
/// `key` is `cat/pkg-ver`. The real `openlog()` "logopt" argument is
/// literally `LOG_ERR | LOG_WARNING | LOG_INFO | LOG_NOTICE` in real
/// portage's own source -- those are priority-level constants, not
/// `LOG_PID`/`LOG_CONS`-style option flags, almost certainly a
/// copy-paste mistake upstream -- but this ports real behavior
/// bug-for-bug rather than the (more sensible) `0` an author fixing it
/// today would likely use.
pub fn syslog_process(key: &str, msgs: &[&ElogMessage]) {
    if msgs.is_empty() {
        return;
    }
    let Ok(ident) = std::ffi::CString::new("portage") else {
        return;
    };
    // SAFETY: `ident` outlives the openlog/closelog pair (glibc keeps
    // only the pointer, not a copy); the priority/facility arguments
    // are plain ints.
    unsafe {
        libc::openlog(
            ident.as_ptr(),
            libc::LOG_ERR | libc::LOG_WARNING | libc::LOG_INFO | libc::LOG_NOTICE,
            libc::LOG_LOCAL5,
        );
    }
    for msg in msgs {
        let priority = syslog_priority(&msg.level);
        let line = syslog_line(key, msg);
        let Ok(c_line) = std::ffi::CString::new(line) else {
            continue;
        };
        // SAFETY: a real, fixed `"%s"` format string -- `c_line` (which
        // may contain `%` from arbitrary ebuild output) is never itself
        // interpreted as a format string, matching real Python's own
        // `syslog.syslog()` (which always does the same internally).
        unsafe {
            libc::syslog(priority, c"%s".as_ptr(), c_line.as_ptr());
        }
    }
    // SAFETY: `closelog` takes no arguments; it is always paired with
    // the `openlog` above, so `ident` is never accessed after close.
    unsafe {
        libc::closelog();
    }
}

/// Real `mod_custom.process` (`elog/mod_custom.py`): always calls
/// `mod_save.process` first (real, unconditionally, even when the
/// `save` module itself isn't separately enabled -- `${LOGFILE}` below
/// needs a real path to substitute), then runs `$PORTAGE_ELOG_COMMAND`
/// (real, unmodified `bash -c`, matching portuale's own "run the real
/// external process" precedent) with `${LOGFILE}`/`${PACKAGE}`
/// substituted. Errors if `PORTAGE_ELOG_COMMAND` is unset/empty (real
/// `MissingParameter`) or the command exits non-zero (real
/// `PortageException`). Returns the saved log file's own path, for the
/// caller's "written to" line -- the same file `${LOGFILE}` pointed the
/// command at.
pub fn custom_process(
    logdir: &Path,
    key: &str,
    fulltext: &str,
    split_elog: bool,
) -> Result<PathBuf, String> {
    let logfile = save_process(logdir, key, fulltext, split_elog)?;
    let cmd_template = std::env::var("PORTAGE_ELOG_COMMAND").unwrap_or_default();
    if cmd_template.trim().is_empty() {
        return Err("Custom logging requested but PORTAGE_ELOG_COMMAND is not defined".to_string());
    }
    let cmd = cmd_template
        .replace("${LOGFILE}", &logfile.display().to_string())
        .replace("${PACKAGE}", key);
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .status()
        .map_err(|e| format!("failed to spawn PORTAGE_ELOG_COMMAND: {e}"))?;
    if !status.success() {
        return Err(format!(
            "PORTAGE_ELOG_COMMAND failed with exitcode {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(logfile)
}

// --- mail settings / MIME / delivery -------------------------------------

/// Real `lib/portage/elog/mod_mail.py` + `mod_mail_summary.py` +
/// `lib/portage/mail.py`, in one place.
///
/// `mod_mail.process` sends one mail per package right away:
/// `PORTAGE_ELOG_MAILURI` picks the recipient (default
/// `root@localhost`, real `make.globals`) and optionally the route --
/// `"address [[user:passwd@]mailserver[:port]]"`, where a `mailserver`
/// that is an absolute path means "pipe the message to that sendmail
/// binary" (`sendmail -f <from> <recipient>` on stdin) instead of SMTP.
/// `PORTAGE_ELOG_MAILFROM` (default `portage@localhost`) and
/// `PORTAGE_ELOG_MAILSUBJECT` (default
/// `"[portage] ebuild log for ${PACKAGE} on ${HOST}"`) take the same
/// `${HOST}`/`${PACKAGE}`/`${ACTION}` substitutions real does;
/// `${ACTION}` is `merged`, or `unmerged` when only `prerm`/`postrm`
/// (or `other`) phases appear, else `unknown`.
///
/// `mod_mail_summary.process` only accumulates
/// (`header + fulltext` per package, with a snapshot of the three
/// settings -- real's `_items[config_root]`); its `finalize` (an atexit
/// handler in real, registered from `elog/__init__.py`) sends ONE mail
/// for the whole run -- subject's `${PACKAGE}` is `one package` /
/// `multiple packages`, the body lists every package, each package's
/// messages ride as a MIME attachment.
///
/// MIME shape mirrors real `portage.mail.create_message` (single
/// `text/plain` UTF-8 part, `multipart/mixed` with one text part plus
/// one part per attachment, `From `/`To`/`From`/`Subject`/`Date`
/// headers, bodies base64-wrapped at 76 columns exactly like Python's
/// `email` package with an explicit UTF-8 charset). `Date` is a local
/// RFC-2822 stamp (`formatdate(localtime=True)`); the `From ` mbox
/// separator carries the same stamp in asctime shape, as Python's
/// `set_unixfrom` flattening does.
///
/// Delivery without new dependencies: the sendmail-binary route spawns
/// the binary directly (real `os.popen`s a shell word-split command
/// line -- argv is the same for sane addresses and safer for odd
/// ones); the SMTP route is a small hand-rolled conversation over
/// `std::net::TcpStream` (EHLO extensions, `AUTH LOGIN` then `AUTH
/// PLAIN`, `MAIL`/`RCPT`/`DATA` with dot-stuffing, `QUIT`, 60 s
/// I/O timeouts matching real's finalize alarm). Deliberate cut:
/// STARTTLS (`:port` above 100000) needs a TLS stack portuale doesn't
/// carry -- attempting it prints a one-line cut notice and skips, the
/// same "message still logged elsewhere, action skipped" shape the old
/// `mail*`-unsupported notice had. `socket.getfqdn()` for `${HOST}` is
/// approximated with `gethostname()` (no DNS lookup); on a system
/// without DNS-backed names these already agree.
///
/// Errors follow real's own `PortageException` text: SMTP protocol
/// failures print `!!! An error occurred while trying to send
/// logmail:`, unreachable hosts `!!! A network error occurred ... Sure
/// you configured PORTAGE_ELOG_MAILURI correctly?`, a failing sendmail
/// binary its own `!!! <path> returned with a non-zero exit code`
/// line -- all to stderr, delivery of the other modules unaffected.
/// Real `make.globals` defaults for the three mail settings.
fn elog_mailuri() -> String {
    std::env::var("PORTAGE_ELOG_MAILURI").unwrap_or_else(|_| "root@localhost".to_string())
}

fn elog_mailfrom() -> String {
    std::env::var("PORTAGE_ELOG_MAILFROM").unwrap_or_else(|_| "portage@localhost".to_string())
}

fn elog_mailsubject() -> String {
    std::env::var("PORTAGE_ELOG_MAILSUBJECT")
        .unwrap_or_else(|_| "[portage] ebuild log for ${PACKAGE} on ${HOST}".to_string())
}

/// `${HOST}` for subject/from substitution: real
/// `socket.getfqdn()` (a DNS-backed canonical name); portuale reads
/// the local hostname via `gethostname(2)` with no DNS lookup, which
/// agrees on every system whose hostname already is the FQDN.
fn mail_host() -> String {
    let mut buf = [0 as libc::c_char; 256];
    // SAFETY: `buf` is a valid 256-byte stack array; `gethostname`
    // writes at most its length including the NUL.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } == 0;
    if !ok {
        return "localhost".to_string();
    }
    // SAFETY: NUL-terminated by `gethostname` on success; hostname
    // bytes are never interior-NUL. Invalid UTF-8 is lossy-mapped
    // (a subject line, never a protocol field).
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// Real `portage.mail._force_ascii_if_necessary` (gentoo bug #291331):
/// smtplib is ASCII-only, so non-ASCII becomes `\xNN`/`\uNNNN`
/// backslash escapes. Subjects go through this; bodies travel
/// base64'd instead.
fn force_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) < 128 {
            out.push(c);
        } else {
            let n = c as u32;
            if n <= 0xFF {
                out.push_str(&format!("\\x{n:02x}"));
            } else if n <= 0xFFFF {
                out.push_str(&format!("\\u{n:04x}"));
            } else {
                out.push_str(&format!("\\U{n:08x}"));
            }
        }
    }
    out
}

/// Minimal base64 (AUTH credentials, MIME bodies) -- no new dependency
/// for ~20 lines; standard alphabet, `=` padding.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut n: u32 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            n |= (b as u32) << (8 * (2 - i));
        }
        let pad = 3 - chunk.len();
        for i in 0..4 - pad {
            out.push(ALPHABET[((n >> (6 * (3 - i))) & 0x3F) as usize] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

/// Wrap base64 at 76 columns, the way Python's `email` package emits a
/// UTF-8-charset payload.
fn base64_wrapped(text: &str) -> String {
    let encoded = base64_encode(text.as_bytes());
    let mut out = String::new();
    let mut it = encoded.as_bytes().chunks(76).peekable();
    while let Some(chunk) = it.next() {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        if it.peek().is_some() {
            out.push('\n');
        }
    }
    out.push('\n');
    out
}

/// Real `email.utils.formatdate(localtime=True)`: local RFC-2822 stamp
/// (`Thu, 10 Sep 2026 07:45:00 +0200`). `%a`/`%b` follow the process
/// locale like any C `strftime` (real Python always renders English --
/// portuale runs under the C locale in practice, same result).
fn rfc2822_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `localtime_r` writes exactly one `struct tm` through a
    // valid pointer; the input is a live `time_t`.
    unsafe { libc::localtime_r(&now, &mut tm) };
    let mut buf = [0 as libc::c_char; 64];
    // SAFETY: `buf` fits any `strftime` expansion of this fixed
    // format; `tm` was just filled by `localtime_r`.
    let len = unsafe {
        libc::strftime(
            buf.as_mut_ptr(),
            buf.len(),
            c"%a, %d %b %Y %H:%M:%S %z".as_ptr(),
            &tm,
        )
    };
    if len == 0 {
        return String::new();
    }
    // SAFETY: `strftime` wrote `len` bytes of (locale-)text with no
    // interior NUL; lossy-mapped like the hostname above.
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

// --- MIME --------------------------------------------------------------

/// One `text/plain` MIME part, real `MIMEText(body)` +
/// `set_charset("UTF-8")` (which is why the payload is base64, even
/// when it happens to be pure ASCII).
fn mime_text_part(body: &str) -> String {
    format!(
        "Content-Type: text/plain; charset=\"utf-8\"\n\
         MIME-Version: 1.0\n\
         Content-Transfer-Encoding: base64\n\
         \n\
         {}",
        base64_wrapped(body)
    )
}

/// Real `portage.mail.create_message`: `From ` mbox separator, then
/// `To`/`From`/`Subject`/`Date`, then either one text part or
/// `multipart/mixed` (text body first, then one part per attachment).
/// `subject` must already be `${...}`-substituted; it is
/// ASCII-forced here, like real's `Header(...)` assignment.
fn create_message(
    from: &str,
    recipient: &str,
    subject: &str,
    body: &str,
    attachments: &[String],
) -> String {
    let date = rfc2822_now();
    let mut msg = format!(
        "From {from} {date}\n\
         To: {recipient}\n\
         From: {from}\n\
         Subject: {}\n\
         Date: {date}\n",
        force_ascii(subject)
    );
    if attachments.is_empty() {
        msg.push_str(&mime_text_part(body));
    } else {
        // Real's boundary is `email`'s random token; portuale mints
        // one from pid + time (same uniqueness role, and -- unlike a
        // fixed string -- it cannot collide with a logged payload).
        // Tests match the structure, never the token.
        let boundary = format!(
            "portuale-elog-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        msg.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\n\
             MIME-Version: 1.0\n\
             \n\
             This is a multi-part message in MIME format.\n\
             --{boundary}\n\
             {}",
            mime_text_part(body)
        ));
        for attachment in attachments {
            msg.push_str(&format!("--{boundary}\n{attachment}"));
        }
        msg.push_str(&format!("--{boundary}--\n"));
    }
    msg
}

// --- routing ------------------------------------------------------------

/// Where one message goes: real `PORTAGE_ELOG_MAILURI`
/// `"address [[user:passwd@]mailserver[:port]]"` -- an absolute-path
/// `mailserver` pipes to that sendmail binary, anything else is SMTP
/// (`localhost:25` when bare). A `:port` above 100000 means STARTTLS
/// on `port - 100000` (real `mail.py`); portuale cannot do TLS and
/// reports that as a cut at send time.
enum MailRoute {
    Sendmail {
        path: String,
    },
    Smtp {
        recipient: String,
        host: String,
        port: u16,
        starttls: bool,
        user: String,
        passwd: String,
    },
}

/// Split `PORTAGE_ELOG_MAILURI` into its recipient and route. Returns
/// the recipient alone (direct-SMTP-default shape) when there is no
/// second word, exactly like real's `else` branch.
fn parse_mailuri(uri: &str) -> (String, Option<MailRoute>) {
    let mut words = uri.split_whitespace();
    let recipient = words.next().unwrap_or("root@localhost").to_string();
    let Some(second) = words.next() else {
        return (recipient.clone(), None);
    };
    let (auth, conn) = match second.rsplit_once('@') {
        Some((a, c)) => (a, c),
        None => ("", second),
    };
    let (user, passwd) = match auth.split_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => {
            if !auth.is_empty() {
                eprintln!("!!! invalid SMTP AUTH configuration, trying unauthenticated ...");
            }
            (String::new(), String::new())
        }
    };
    if conn.starts_with('/') {
        return (
            recipient,
            Some(MailRoute::Sendmail {
                path: conn.to_string(),
            }),
        );
    }
    let (host, port) = match conn.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u32>().unwrap_or(25)),
        None => (conn, 25),
    };
    let (port, starttls) = if port > 100000 {
        ((port - 100000) as u16, true)
    } else {
        (port as u16, false)
    };
    (
        recipient.clone(),
        Some(MailRoute::Smtp {
            recipient,
            host: host.to_string(),
            port,
            starttls,
            user,
            passwd,
        }),
    )
}

/// Real `portage.mail.send_mail` over the parsed route: the sendmail
/// binary gets `-f <from> <recipient>` on argv and the message on
/// stdin; SMTP runs the hand-rolled conversation below. `Ok(())`
/// means accepted; `Err` text already follows real's own
/// `PortageException` shapes (the caller just prints it).
fn send_mail(
    from: &str,
    message: &str,
    route: &Option<MailRoute>,
    default_recipient: &str,
) -> Result<(), String> {
    match route {
        // No second MAILURI word: real still SMTPs to localhost:25,
        // addressed to the whole URI word.
        None => smtp_send("localhost", 25, "", "", default_recipient, from, message),
        Some(MailRoute::Sendmail { path }) => {
            if !std::path::Path::new(path).exists() {
                // Real `os.path.exists` gate: a missing sendmail path
                // falls through to SMTP against the path as a host
                // (which then fails as a network error). Portuale
                // reports the missing binary directly -- same failure,
                // less confusing detour.
                return Err(format!("!!! sendmail binary {path} does not exist"));
            }
            let mut child = std::process::Command::new(path)
                .arg("-f")
                .arg(from)
                .arg(default_recipient)
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| {
                    format!("!!! A network error occurred while trying to send logmail:\n{e}\nSure you configured PORTAGE_ELOG_MAILURI correctly?")
                })?;
            {
                use std::io::Write;
                if let Some(stdin) = child.stdin.take() {
                    let mut stdin = stdin;
                    let _ = stdin.write_all(force_ascii(message).as_bytes());
                }
            }
            let status = child.wait().map_err(|e| {
                format!("!!! A network error occurred while trying to send logmail:\n{e}\nSure you configured PORTAGE_ELOG_MAILURI correctly?")
            })?;
            if !status.success() {
                eprintln!(
                    "!!! {path} returned with a non-zero exit code. This generally indicates an error."
                );
            }
            Ok(())
        }
        Some(MailRoute::Smtp {
            recipient,
            host,
            port,
            starttls,
            user,
            passwd,
        }) => {
            if *starttls {
                // Portuale carries no TLS stack: report the cut and
                // skip, keeping every other module's output.
                eprintln!(
                    " elog STARTTLS delivery is not supported by portuale \
                     (no TLS stack) -- mail to {recipient} skipped"
                );
                return Ok(());
            }
            smtp_send(host, *port, user, passwd, recipient, from, message)
        }
    }
}

/// One SMTP response: `(code, lines)` -- `250-...` continues,
/// `250 ...` ends (real `smtplib` parses the same way).
fn smtp_read_response(reader: &mut impl std::io::BufRead) -> Result<(u16, Vec<String>), String> {
    let mut lines = Vec::new();
    let code = loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read: {e}"))?;
        if line.len() < 4 {
            return Err(format!("short SMTP response: {line:?}"));
        }
        let code: u16 = line[..3]
            .parse()
            .map_err(|_| format!("bad SMTP response: {line:?}"))?;
        lines.push(
            line[3..]
                .trim_start_matches(['-', ' '])
                .trim_end()
                .to_string(),
        );
        if line.as_bytes()[3] == b' ' {
            break code;
        }
    };
    Ok((code, lines))
}

/// Hand-rolled `smtplib` subset: greeting, EHLO (extensions parsed for
/// the AUTH choice), optional `AUTH LOGIN` (then `AUTH PLAIN`),
/// `MAIL`/`RCPT`/`DATA` with dot-stuffing, `QUIT`. Response codes
/// follow real's `SMTPException`-on-error shape; transport failures
/// follow its `OSError` shape.
fn smtp_send(
    host: &str,
    port: u16,
    user: &str,
    passwd: &str,
    recipient: &str,
    from: &str,
    message: &str,
) -> Result<(), String> {
    use std::io::{BufReader, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    fn net_err(e: impl std::fmt::Display) -> String {
        format!(
            "!!! A network error occurred while trying to send logmail:\n{e}\nSure you configured PORTAGE_ELOG_MAILURI correctly?"
        )
    }
    fn smtp_err(e: impl std::fmt::Display) -> String {
        format!("!!! An error occurred while trying to send logmail:\n{e}")
    }
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(net_err)?
        .next()
        .ok_or_else(|| net_err(format!("cannot resolve {host}")))?;
    let timeout = std::time::Duration::from_secs(60);
    let stream = TcpStream::connect_timeout(&addr, timeout).map_err(net_err)?;
    stream.set_read_timeout(Some(timeout)).map_err(net_err)?;
    stream.set_write_timeout(Some(timeout)).map_err(net_err)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(net_err)?);
    let mut writer = stream;
    let mut cmd = |text: &str| -> Result<(), String> {
        writer.write_all(text.as_bytes()).map_err(net_err)?;
        writer.write_all(b"\r\n").map_err(net_err)?;
        writer.flush().map_err(net_err)?;
        Ok(())
    };
    let expect = |reader: &mut BufReader<TcpStream>,
                  want: &[u16],
                  what: &str|
     -> Result<Vec<String>, String> {
        let (code, lines) = smtp_read_response(reader)?;
        if !want.contains(&code) {
            return Err(smtp_err(format!("{what}: SMTP server replied {code}")));
        }
        Ok(lines)
    };

    expect(&mut reader, &[220], "greeting")?;
    cmd(&format!("EHLO {}", mail_host()))?;
    let extensions = expect(&mut reader, &[250], "EHLO")?
        .into_iter()
        .map(|l| l.to_uppercase())
        .collect::<Vec<_>>();
    if !user.is_empty() {
        let mut authed = false;
        if extensions
            .iter()
            .any(|e| e == "AUTH" || e.starts_with("AUTH "))
        {
            cmd("AUTH LOGIN")?;
            let (code, _) = smtp_read_response(&mut reader)?;
            if code == 334 {
                cmd(&base64_encode(user.as_bytes()))?;
                let (code, _) = smtp_read_response(&mut reader)?;
                if code == 334 {
                    cmd(&base64_encode(passwd.as_bytes()))?;
                    let (code, _) = smtp_read_response(&mut reader)?;
                    authed = code == 235;
                }
            }
            if !authed {
                // Empty authorize-id, like `smtplib`'s own PLAIN
                // initial response (`\0user\0pass`).
                let token = base64_encode(format!("\0{user}\0{passwd}").as_bytes());
                cmd(&format!("AUTH PLAIN {token}"))?;
                let (code, _) = smtp_read_response(&mut reader)?;
                authed = code == 235;
            }
        }
        if !authed {
            return Err(smtp_err("SMTP AUTH failed"));
        }
    }
    cmd(&format!("MAIL FROM:<{from}>"))?;
    expect(&mut reader, &[250], "MAIL FROM")?;
    cmd(&format!("RCPT TO:<{recipient}>"))?;
    expect(&mut reader, &[250, 251], "RCPT TO")?;
    cmd("DATA")?;
    expect(&mut reader, &[354], "DATA")?;
    for line in force_ascii(message).lines() {
        // Dot-stuffing (real `smtplib.sendmail` does the same).
        if let Some(stripped) = line.strip_prefix('.') {
            cmd(&format!(".{stripped}"))?;
        } else {
            cmd(line)?;
        }
    }
    // `BufRead::lines` drops a trailing empty line; the message must
    // still end `<CRLF>.<CRLF>` (and an empty body still terminates).
    cmd(".")?;
    expect(&mut reader, &[250], "message data")?;
    let _ = cmd("QUIT");
    Ok(())
}

// --- modules --------------------------------------------------------------

/// Real `mod_mail.process`'s own `${ACTION}` call: `merged` normally;
/// `unmerged` when only `*rm` (or `other`) phases appear; `unknown`
/// when an `*rm` phase is mixed with anything else (to "avoid
/// misinformation").
fn mail_action(phases: &[&str]) -> &'static str {
    if !phases.iter().any(|p| matches!(*p, "postrm" | "prerm")) {
        return "merged";
    }
    if phases
        .iter()
        .any(|p| !matches!(*p, "postrm" | "prerm" | "other"))
    {
        return "unknown";
    }
    "unmerged"
}
/// Real `mod_mail.process`: one mail for package `key` right away.
/// `phases` are the phases present in this package's logentries (for
/// the merged/unmerged/unknown `${ACTION}` call); `fulltext` is the
/// class-filtered combined text. Delivery failures print real's own
/// `PortageException` line; nothing is returned (real `process`
/// returns `None`).
fn mail_process(key: &str, phases: &[&str], fulltext: &str) {
    let uri = elog_mailuri();
    let (recipient, route) = parse_mailuri(&uri);
    let host = mail_host();
    let from = elog_mailfrom().replace("${HOST}", &host);
    let mut subject = elog_mailsubject()
        .replace("${PACKAGE}", key)
        .replace("${HOST}", &host);
    subject = subject.replace("${ACTION}", mail_action(phases));
    let message = create_message(&from, &recipient, &subject, fulltext, &[]);
    if let Err(e) = send_mail(&from, &message, &route, &recipient) {
        eprintln!("{e}");
    }
}

/// Accumulated `mod_mail_summary` state: real `_items[config_root]`
/// (first-seen settings snapshot + per-package header+fulltext),
/// narrowed to portuale's single config per process.
struct MailSummaryState {
    config: Option<(String, String, String)>,
    items: Vec<(String, String)>,
}

/// Process-lifetime `mail_summary` accumulation (real module-global
/// `_items`); finalized once at process exit (real atexit `finalize`).
static MAIL_SUMMARY: std::sync::Mutex<MailSummaryState> = std::sync::Mutex::new(MailSummaryState {
    config: None,
    items: Vec::new(),
});

/// Real `mod_mail_summary.process`: stash `header + fulltext` for
/// `key`, snapshotting the three settings on first use (real
/// `setdefault` keeps the first config for the whole run).
fn mail_summary_accumulate(key: &str, fulltext: &str) {
    let header = format!(
        ">>> Messages generated for package {key} by process {} on {}:\n\n",
        std::process::id(),
        rfc2822_now(),
    );
    let Ok(mut state) = MAIL_SUMMARY.lock() else {
        return;
    };
    if state.config.is_none() {
        state.config = Some((elog_mailuri(), elog_mailfrom(), elog_mailsubject()));
    }
    match state.items.iter_mut().find(|(k, _)| k == key) {
        Some((_, body)) => *body = format!("{header}{fulltext}"),
        None => state
            .items
            .push((key.to_string(), format!("{header}{fulltext}"))),
    }
    // First accumulation arms the exit-time finalize (real atexit
    // registration); `Once` keeps one registration per process.
    static REGISTER_ONCE: std::sync::Once = std::sync::Once::new();
    REGISTER_ONCE.call_once(|| {
        // SAFETY: plain function pointer, no captures; the handler
        // only locks `MAIL_SUMMARY` and does I/O, and process exit
        // (not `_exit`/abort) is the only path that runs it -- the
        // same contract real's `atexit_register` relies on.
        unsafe {
            libc::atexit(mail_summary_atexit);
        }
    });
}

/// Real `mod_mail_summary.finalize` (via `_finalize`): one
/// multipart mail for every accumulated package (or nothing when no
/// package produced messages -- real returns early on zero items).
/// Always clears the accumulation, sent or not.
fn mail_summary_finalize() {
    let (config, items) = match MAIL_SUMMARY.lock() {
        Ok(mut state) => (state.config.take(), std::mem::take(&mut state.items)),
        Err(_) => return,
    };
    if items.is_empty() {
        return;
    }
    let (uri, from_tpl, subject_tpl) =
        config.unwrap_or_else(|| (elog_mailuri(), elog_mailfrom(), elog_mailsubject()));
    let (recipient, route) = parse_mailuri(&uri);
    let host = mail_host();
    let from = from_tpl.replace("${HOST}", &host);
    let count = if items.len() == 1 {
        "one package"
    } else {
        "multiple packages"
    };
    let subject = subject_tpl
        .replace("${PACKAGE}", count)
        .replace("${HOST}", &host);
    let mut body = format!(
        "elog messages for the following packages generated by process {} on host {}:\n",
        std::process::id(),
        host,
    );
    let mut attachments = Vec::new();
    for (key, item) in &items {
        body.push_str(&format!("- {key}\n"));
        // Real attaches one `TextMessage` per package (its own
        // `header + fulltext`), each base64'd like the lead part.
        attachments.push(mime_text_part(item));
    }
    let message = create_message(&from, &recipient, &subject, &body, &attachments);
    if let Err(e) = send_mail(&from, &message, &route, &recipient) {
        eprintln!("{e}");
    }
}

/// `extern "C"` entry for `libc::atexit` -- see
/// `mail_summary_accumulate`. Must not unwind (that would abort);
/// every fallible step is already `Option`/`Result`-guarded inside
/// `mail_summary_finalize` (lock failure returns silently, delivery
/// errors print).
extern "C" fn mail_summary_atexit() {
    mail_summary_finalize();
}

/// Real `mod_save` / `mod_save_summary` / `mod_syslog` / `mod_custom`
/// / `mod_mail` / `mod_mail_summary`
/// for one merged package: build the per-module `fulltext` from
/// `all_msgs` (unfiltered `collect_all`) and hand it to whichever
/// modules are enabled. Each module's own `filter_loglevels` runs here
/// (`save_summary`'s default token carries `:log,warn,error,qa`). A
/// module is skipped for this package when the filter leaves nothing
/// (real `if len(mod_logentries) == 0: continue`). Returns the paths
/// written (`save`/`save_summary`/`custom`; `syslog`/`mail`/
/// `mail_summary` write no file),
/// for the caller's log line. `mail_summary` only accumulates here --
/// its single run-wide mail goes out from `mail_summary_finalize` at
/// process exit (real atexit `finalize`).
pub fn save_modules_process(
    logdir: &Path,
    key: &str,
    all_msgs: &[ElogMessage],
    split_elog: bool,
) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    for (name, is_summary) in [("save", false), ("save_summary", true)] {
        if !module_enabled(name) {
            continue;
        }
        let filtered = filter_by_classes(all_msgs, &module_classes(name));
        if filtered.is_empty() {
            continue;
        }
        let fulltext = combine_logentries(&filtered);
        let path = if is_summary {
            save_summary_process(logdir, key, &fulltext)?
        } else {
            save_process(logdir, key, &fulltext, split_elog)?
        };
        written.push(path);
    }
    if module_enabled("syslog") {
        let filtered = filter_by_classes(all_msgs, &module_classes("syslog"));
        if !filtered.is_empty() {
            syslog_process(key, &filtered);
        }
    }
    if module_enabled("custom") {
        let filtered = filter_by_classes(all_msgs, &module_classes("custom"));
        if !filtered.is_empty() {
            let fulltext = combine_logentries(&filtered);
            written.push(custom_process(logdir, key, &fulltext, split_elog)?);
        }
    }
    if module_enabled("mail") {
        let filtered = filter_by_classes(all_msgs, &module_classes("mail"));
        if !filtered.is_empty() {
            // Real hands the module its filtered logentries; only the
            // phases present matter (`${ACTION}` detection).
            let phases: Vec<&str> = {
                let mut seen = std::collections::HashSet::new();
                filtered
                    .iter()
                    .map(|m| m.phase.as_str())
                    .filter(|p| seen.insert(*p))
                    .collect()
            };
            let fulltext = combine_logentries(&filtered);
            mail_process(key, &phases, &fulltext);
        }
    }
    if module_enabled("mail_summary") {
        let filtered = filter_by_classes(all_msgs, &module_classes("mail_summary"));
        if !filtered.is_empty() {
            let fulltext = combine_logentries(&filtered);
            mail_summary_accumulate(key, &fulltext);
        }
    }
    Ok(written)
}

/// Real `elog_process` over a batch of packages that just merged or
/// unmerged. Each item is `(cpv, t_dir)` where `t_dir` is that package's
/// `${T}` (`${PORTAGE_BUILDDIR}/temp`). `phases` restricts which
/// `${T}/logging/<phase>` files are read (real
/// `_elog_process(phasefilter=...)`): `None` after a merge (every
/// phase), `Some(&["prerm", "postrm"])` for `dblink.unmerge()`.
/// `root_display` is the `ROOT` string for the `echo` header (`/` gives
/// the short form).
///
/// The `save` / `save_summary` modules run immediately per package (real
/// `mod_save.process`), printing the `Elog messages ... written to ...`
/// line; `echo` is accumulated and printed once at the end (real
/// `mod_echo._finalize`, an atexit handler). `mail` sends immediately
/// per package (real `mod_mail.process`); `mail_summary` accumulates
/// and sends once at process exit (real atexit `finalize`, armed on
/// first use). A no-op when no module is enabled
/// or nothing has messages. Portuale never cleans the builddir, so the
/// caller re-scans `${T}/logging/` here rather than threading a message
/// buffer through the (un)merge machinery.
pub fn process_batch(
    logdir: &Path,
    root_display: &str,
    items: &[(String, PathBuf)],
    phases: Option<&[&str]>,
    color: &Colorizer,
) {
    let echo = echo_enabled();
    // "save_any" gates the one `collect_all`/`save_modules_process` call
    // below, so it needs every module that call also dispatches to --
    // `syslog`/`custom`/`mail`/`mail_summary` included, not just the
    // `save`/`save_summary` pair its own name still refers to.
    let save_any = module_enabled("save")
        || module_enabled("save_summary")
        || module_enabled("syslog")
        || module_enabled("custom")
        || module_enabled("mail")
        || module_enabled("mail_summary");
    if !(echo || save_any) {
        return;
    }
    let split_elog = std::env::var("FEATURES")
        .unwrap_or_default()
        .split_whitespace()
        .any(|f| f == "split-elog");
    let mut packages = Vec::new();
    for (cpv, t_dir) in items {
        if save_any {
            let all = match phases {
                Some(p) => collect_all_phases(t_dir, p),
                None => collect_all(t_dir),
            };
            if !all.is_empty() {
                match save_modules_process(logdir, cpv, &all, split_elog) {
                    Ok(paths) => {
                        for p in paths {
                            println!(
                                "{}Elog messages for {cpv} written to {}",
                                color.c("INFO", " * "),
                                p.display()
                            );
                        }
                    }
                    Err(e) => eprintln!("elog: {e}"),
                }
            }
        }
        if echo {
            let messages = collect(t_dir, phases);
            if !messages.is_empty() {
                packages.push(ElogPackage {
                    cpv: cpv.clone(),
                    root: root_display.to_string(),
                    messages,
                });
            }
        }
    }
    if echo {
        echo_summary(&packages, color);
    }
}

/// Real `mod_echo._finalize`: the `* Messages for package <cpv>:` block
/// for every accumulated package, all message types on stdout, each line
/// `<colour> * </colour><msg>` (`EOutput.e{info,log,warn,error,qawarn}`).
pub fn echo_summary(packages: &[ElogPackage], color: &Colorizer) {
    for pkg in packages {
        println!();
        let key = color.c("INFORM", &pkg.cpv);
        let star = color.c("INFO", " * ");
        if pkg.root == "/" {
            println!("{star}Messages for package {key}:");
        } else {
            println!("{star}Messages for package {key} merged to {}:", pkg.root);
        }
        println!();
        for msg in &pkg.messages {
            let style = match msg.level.as_str() {
                "INFO" => "INFO",
                "LOG" => "LOG",
                "WARN" => "WARN",
                "ERROR" => "ERR",
                _ => "QAWARN", // QA
            };
            println!("{}{}", color.c(style, " * "), msg.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "elog_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(d.join("logging")).unwrap();
        d
    }

    #[test]
    fn collect_reads_phase_files_in_order_and_filters_by_class() {
        // Default classes ("log warn error") -- INFO/QA are dropped, and
        // phase order (install before postinst) is honoured.
        let t = tmpdir();
        std::fs::write(
            t.join("logging/postinst"),
            "WARN later warning\nINFO postinst info\n",
        )
        .unwrap();
        std::fs::write(
            t.join("logging/install"),
            "LOG first message\nQA a qa note\nERROR an error\n",
        )
        .unwrap();

        let msgs = collect(&t, None);
        let got: Vec<(&str, &str)> = msgs
            .iter()
            .map(|m| (m.level.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("LOG", "first message"),
                ("ERROR", "an error"),
                ("WARN", "later warning"),
            ]
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn collect_is_empty_when_there_are_no_message_files() {
        let t = tmpdir();
        assert!(collect(&t, None).is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn collect_phasefilter_keeps_only_the_named_phases() {
        // A builddir portuale never cleaned: install-time logs still
        // sit next to the removal-time ones. `dblink.unmerge`'s
        // `phasefilter=("prerm","postrm")` must ignore the install log.
        let t = tmpdir();
        std::fs::write(t.join("logging/install"), "LOG built fine\n").unwrap();
        std::fs::write(t.join("logging/prerm"), "WARN leaving config behind\n").unwrap();
        std::fs::write(t.join("logging/postrm"), "LOG run revdep-rebuild\n").unwrap();

        let filtered = collect_all_phases(&t, &["prerm", "postrm"]);
        let got: Vec<(&str, &str)> = filtered
            .iter()
            .map(|m| (m.phase.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("prerm", "leaving config behind"),
                ("postrm", "run revdep-rebuild"),
            ]
        );
        // The echo-filtered variant drops the WARN only if classes say so;
        // default classes keep it, so both survive here.
        assert_eq!(collect(&t, Some(&["prerm", "postrm"])).len(), 2);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn echo_summary_renders_the_real_message_block_shape() {
        let color = Colorizer::new(false);
        let pkg = ElogPackage {
            cpv: "dev-libs/foo-1.0".to_string(),
            root: "/".to_string(),
            messages: vec![
                ElogMessage {
                    phase: "postinst".to_string(),
                    level: "LOG".to_string(),
                    text: "read the docs".to_string(),
                },
                ElogMessage {
                    phase: "postinst".to_string(),
                    level: "WARN".to_string(),
                    text: "watch out".to_string(),
                },
            ],
        };
        // No colour -> plain " * " prefixes; capture is via a real run in
        // test_portuale.py, this just exercises the non-panicking path.
        echo_summary(std::slice::from_ref(&pkg), &color);
    }

    #[test]
    fn module_classes_honours_the_per_module_override() {
        // `save_summary:log,warn,error,qa` (the make.globals default token)
        // overrides the bare PORTAGE_ELOG_CLASSES for that module only.
        temp_env(
            &[(
                "PORTAGE_ELOG_SYSTEM",
                Some("save_summary:log,warn,error,qa echo"),
            )],
            || {
                let sc = module_classes("save_summary");
                assert!(sc.contains("QA"));
                assert!(sc.contains("LOG"));
                // `echo` has no override -> the bare default {LOG,WARN,ERROR}.
                let ec = module_classes("echo");
                assert!(!ec.contains("QA"));
                assert!(ec.contains("WARN"));
            },
        );
    }

    #[test]
    fn combine_logentries_matches_real_combine_shape() {
        let msgs = [
            ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "a".to_string(),
            },
            ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "b".to_string(),
            },
            ElogMessage {
                phase: "install".to_string(),
                level: "WARN".to_string(),
                text: "c".to_string(),
            },
            ElogMessage {
                phase: "postinst".to_string(),
                level: "WARN".to_string(),
                text: "d".to_string(),
            },
        ];
        let refs: Vec<&ElogMessage> = msgs.iter().collect();
        assert_eq!(
            combine_logentries(&refs),
            "LOG: install\na\nb\nWARN: install\nc\nWARN: postinst\nd\n"
        );
        assert_eq!(combine_logentries(&[]), "");
    }

    #[test]
    fn save_and_save_summary_write_the_expected_files() {
        let t = tmpdir();
        let logs = t.join("logs");
        temp_env(
            &[
                (
                    "PORTAGE_ELOG_SYSTEM",
                    Some("save save_summary:log,warn,error,qa echo"),
                ),
                ("PORTAGE_ELOG_CLASSES", Some("log warn error")),
            ],
            || {
                let msgs = [
                    ElogMessage {
                        phase: "install".to_string(),
                        level: "LOG".to_string(),
                        text: "hello".to_string(),
                    },
                    ElogMessage {
                        phase: "install".to_string(),
                        level: "QA".to_string(),
                        text: "a qa note".to_string(),
                    },
                ];
                let written =
                    save_modules_process(&logs, "dev-libs/foo-1.0", &msgs, false).unwrap();
                assert_eq!(written.len(), 2);

                // save: one <cat>:<pf>:<stamp>.log, `fulltext` only has the
                // LOG line (QA filtered out by PORTAGE_ELOG_CLASSES).
                let elog = logs.join("elog");
                let saved: Vec<_> = std::fs::read_dir(&elog)
                    .unwrap()
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("dev-libs:foo-1.0:"))
                    .collect();
                assert_eq!(saved.len(), 1);
                assert_eq!(
                    std::fs::read_to_string(elog.join(&saved[0])).unwrap(),
                    "LOG: install\nhello\n"
                );

                // save_summary: its :log,warn,error,qa override keeps the QA line.
                let summary = std::fs::read_to_string(elog.join("summary.log")).unwrap();
                assert!(summary.contains("for package dev-libs/foo-1.0:"));
                assert!(summary.contains("LOG: install\nhello\nQA: install\na qa note\n"));
            },
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn save_modules_process_dispatches_to_custom_when_enabled() {
        let t = tmpdir();
        let marker = t.join("marker");
        temp_env(
            &[
                ("PORTAGE_ELOG_SYSTEM", Some("custom")),
                ("PORTAGE_ELOG_CLASSES", Some("log warn error")),
                (
                    "PORTAGE_ELOG_COMMAND",
                    Some(&format!("touch {}", marker.display())),
                ),
            ],
            || {
                let msgs = [ElogMessage {
                    phase: "install".to_string(),
                    level: "LOG".to_string(),
                    text: "hello".to_string(),
                }];
                let written =
                    save_modules_process(&t.join("logs"), "dev-libs/foo-1.0", &msgs, false)
                        .expect("save_modules_process succeeds");
                assert_eq!(written.len(), 1, "custom's own saved-file path");
                assert!(marker.exists(), "PORTAGE_ELOG_COMMAND must have run");
            },
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn save_modules_are_off_when_not_in_portage_elog_system() {
        let t = tmpdir();
        temp_env(&[("PORTAGE_ELOG_SYSTEM", Some("echo"))], || {
            let msgs = [ElogMessage {
                phase: "install".to_string(),
                level: "LOG".to_string(),
                text: "x".to_string(),
            }];
            let written =
                save_modules_process(&t.join("logs"), "dev-libs/foo-1.0", &msgs, false).unwrap();
            assert!(written.is_empty());
            assert!(!t.join("logs").exists());
        });
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn syslog_priority_matches_real_mod_syslogs_own_pri_map() {
        assert_eq!(syslog_priority("INFO"), libc::LOG_INFO);
        assert_eq!(syslog_priority("ERROR"), libc::LOG_ERR);
        assert_eq!(syslog_priority("LOG"), libc::LOG_NOTICE);
        assert_eq!(syslog_priority("WARN"), libc::LOG_WARNING);
        assert_eq!(syslog_priority("QA"), libc::LOG_WARNING);
    }

    #[test]
    fn syslog_line_matches_real_key_phase_line_shape() {
        let msg = ElogMessage {
            phase: "install".to_string(),
            level: "WARN".to_string(),
            text: "watch out\n".to_string(),
        };
        assert_eq!(
            syslog_line("dev-libs/foo-1.0", &msg),
            "dev-libs/foo-1.0: install: watch out"
        );
    }

    #[test]
    fn custom_process_runs_the_real_command_with_logfile_and_package_substituted() {
        let t = tmpdir();
        let marker = t.join("marker");
        temp_env(
            &[(
                "PORTAGE_ELOG_COMMAND",
                Some(&format!(
                    "echo \"${{PACKAGE}} $(cat \"${{LOGFILE}}\")\" > {}",
                    marker.display()
                )),
            )],
            || {
                let path =
                    custom_process(&t.join("logs"), "dev-libs/foo-1.0", "hello there", false)
                        .expect("custom_process succeeds");
                assert!(path.exists(), "the underlying saved log file must exist");
                let recorded = std::fs::read_to_string(&marker).unwrap();
                assert_eq!(recorded.trim(), "dev-libs/foo-1.0 hello there");
            },
        );
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn custom_process_errors_when_the_command_is_unset() {
        let t = tmpdir();
        temp_env(&[("PORTAGE_ELOG_COMMAND", None)], || {
            let err = custom_process(&t.join("logs"), "dev-libs/foo-1.0", "hello", false)
                .expect_err("no PORTAGE_ELOG_COMMAND must fail");
            assert!(err.contains("PORTAGE_ELOG_COMMAND"), "{err}");
        });
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn custom_process_errors_when_the_command_fails() {
        let t = tmpdir();
        temp_env(&[("PORTAGE_ELOG_COMMAND", Some("exit 1"))], || {
            let err = custom_process(&t.join("logs"), "dev-libs/foo-1.0", "hello", false)
                .expect_err("a failing command must fail");
            assert!(err.contains("exitcode"), "{err}");
        });
        let _ = std::fs::remove_dir_all(&t);
    }

    /// Minimal process-env scoping for these serial tests -- `env::var`
    /// reads are process-global, so run one closure at a time.
    fn temp_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                // SAFETY: env mutation is unsafe on edition 2024 (UB in
                // the presence of other threads); these serial tests
                // scope every env write behind `LOCK` (held by `_g`).
                Some(v) => unsafe { std::env::set_var(k, v) },
                // SAFETY: same serial-`LOCK`-scoped env mutation as the
                // `Some` arm above (removing a var).
                None => unsafe { std::env::remove_var(k) },
            }
        }
        f();
        for (k, v) in saved {
            match v {
                // SAFETY: same serial-`LOCK`-scoped env mutation as
                // above; `_g` still holds `LOCK` here.
                Some(v) => unsafe { std::env::set_var(&k, v) },
                // SAFETY: same serial-`LOCK`-scoped env mutation as
                // above (removing a var).
                None => unsafe { std::env::remove_var(&k) },
            }
        }
    }

    #[test]
    fn base64_encode_matches_the_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode("\0user\0pass".as_bytes()), "AHVzZXIAcGFzcw==");
    }

    #[test]
    fn force_ascii_backslash_escapes_non_ascii() {
        assert_eq!(force_ascii("plain"), "plain");
        assert_eq!(force_ascii("caf\u{e9}"), "caf\\xe9");
        assert_eq!(force_ascii("A\u{4e2d}B"), "A\\u4e2dB");
    }

    #[test]
    fn mail_action_follows_the_merged_unmerged_unknown_call() {
        assert_eq!(mail_action(&["install", "postinst"]), "merged");
        assert_eq!(mail_action(&[]), "merged");
        assert_eq!(mail_action(&["prerm", "postrm"]), "unmerged");
        assert_eq!(mail_action(&["prerm", "other"]), "unmerged");
        assert_eq!(mail_action(&["prerm", "install"]), "unknown");
        assert_eq!(mail_action(&["postrm", "preinst"]), "unknown");
    }

    #[test]
    fn parse_mailuri_covers_recipient_only_sendmail_and_smtp() {
        // Bare recipient: direct SMTP to localhost:25 (real `else`).
        let (to, route) = parse_mailuri("root@localhost");
        assert_eq!(to, "root@localhost");
        assert!(route.is_none());
        // Absolute-path server: the sendmail binary route.
        let (to, route) = parse_mailuri("root@localhost /usr/sbin/sendmail");
        assert_eq!(to, "root@localhost");
        assert!(
            matches!(route, Some(MailRoute::Sendmail { path }) if path == "/usr/sbin/sendmail")
        );
        // SMTP host, default port, no auth.
        let (to, route) = parse_mailuri("me@example.com mail.example.com");
        assert_eq!(to, "me@example.com");
        assert!(
            matches!(route, Some(MailRoute::Smtp { host, port, starttls, user, .. })
                if host == "mail.example.com" && port == 25 && !starttls && user.is_empty())
        );
        // AUTH + explicit port.
        let (to, route) = parse_mailuri("me@example.com user:secret@mail.example.com:587");
        assert_eq!(to, "me@example.com");
        assert!(
            matches!(route, Some(MailRoute::Smtp { host, port, user, passwd, .. })
                if host == "mail.example.com" && port == 587 && user == "user" && passwd == "secret")
        );
        // Port above 100000: STARTTLS on port-100000.
        let (_, route) = parse_mailuri("me@example.com mail.example.com:100465");
        assert!(matches!(route, Some(MailRoute::Smtp { port, starttls: true, .. }) if port == 465));
    }

    #[test]
    fn create_message_renders_headers_and_base64_body() {
        let msg = create_message(
            "portage@testhost",
            "root@testhost",
            "[portage] ebuild log for dev-libs/foo-1.0 on testhost",
            "LOG: install\nhello\n",
            &[],
        );
        assert!(msg.starts_with("From portage@testhost "), "{msg}");
        assert!(msg.contains("\nTo: root@testhost\n"), "{msg}");
        assert!(msg.contains("\nFrom: portage@testhost\n"), "{msg}");
        assert!(
            msg.contains("\nSubject: [portage] ebuild log for dev-libs/foo-1.0 on testhost\n"),
            "{msg}"
        );
        assert!(msg.contains("\nDate: "), "{msg}");
        assert!(msg.contains("Content-Transfer-Encoding: base64"), "{msg}");
        // Python's `email` base64-codes even pure-ASCII UTF-8 bodies.
        assert!(msg.contains("TE9HOiBpbnN0YWxsCmhlbGxvCg=="), "{msg}");
    }

    /// `mail_process` + `mail_summary` accumulation/finalize through a
    /// fake sendmail binary: argv (`-f <from> <recipient>`) and the
    /// MIME bytes on stdin. One test function (not three) because
    /// `MAIL_SUMMARY` is process-global -- internal steps stay
    /// sequential, and `temp_env` serializes the env side.
    #[test]
    fn mail_modules_deliver_through_a_sendmail_binary() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!(
            "elog-mail-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sendmail = dir.join("sendmail");
        std::fs::write(
            &sendmail,
            format!(
                "#!/bin/sh\n\
                 echo \"ARGS: $@\" >> {}\n\
                 cat >> {}\n",
                dir.join("args").display(),
                dir.join("body").display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&sendmail, std::fs::Permissions::from_mode(0o755)).unwrap();

        temp_env(
            &[
                (
                    "PORTAGE_ELOG_MAILURI",
                    Some(&format!("root@testhost {}", sendmail.display())),
                ),
                ("PORTAGE_ELOG_MAILFROM", Some("portage@${HOST}")),
                (
                    "PORTAGE_ELOG_MAILSUBJECT",
                    Some("[portage] ${ACTION} ${PACKAGE} on ${HOST}"),
                ),
            ],
            || {
                // Drain any accumulation a parallel test left behind
                // (finalize always clears, sent or not).
                mail_summary_finalize();
                assert!(MAIL_SUMMARY.lock().unwrap().items.is_empty());

                // One immediate `mail` for a merged package.
                mail_process("dev-libs/foo-1.0", &["install"], "LOG: install\nhello\n");
                let args = std::fs::read_to_string(dir.join("args")).unwrap();
                let host = mail_host();
                assert!(
                    args.contains(&format!("ARGS: -f portage@{host} root@testhost")),
                    "{args}"
                );
                let body = std::fs::read_to_string(dir.join("body")).unwrap();
                assert!(
                    body.contains(&format!(
                        "Subject: [portage] merged dev-libs/foo-1.0 on {host}"
                    )),
                    "{body}"
                );

                // Two packages into `mail_summary`: nothing is sent
                // yet (one run-wide mail at finalize).
                std::fs::remove_file(dir.join("args")).unwrap();
                mail_summary_accumulate("dev-libs/foo-1.0", "LOG: install\nhello\n");
                mail_summary_accumulate("dev-libs/bar-2.0", "WARN: postinst\ncareful\n");
                assert!(
                    !dir.join("args").exists(),
                    "summary must not send per package"
                );
                mail_summary_finalize();
                assert!(
                    MAIL_SUMMARY.lock().unwrap().items.is_empty(),
                    "finalize clears"
                );
                let body = std::fs::read_to_string(dir.join("body")).unwrap();
                assert!(body.contains("multipart/mixed"), "{body}");
                // Real's summary substitutes only `${PACKAGE}` (the
                // one/multiple-packages count) and `${HOST}` -- a
                // `${ACTION}` in the template stays literal, exactly
                // as real `mod_mail_summary._finalize` leaves it.
                assert!(
                    body.contains(&format!(
                        "Subject: [portage] ${{ACTION}} multiple packages on {}",
                        mail_host()
                    )),
                    "{body}"
                );
                // The lead part is base64 (like real), so the package
                // list is asserted in its encoded form.
                let lead = format!(
                    "elog messages for the following packages generated by process {} on host {}:\n- dev-libs/foo-1.0\n- dev-libs/bar-2.0\n",
                    std::process::id(),
                    mail_host(),
                );
                assert!(body.contains(base64_wrapped(&lead).trim_end()), "{body}");
                // Lead part + one MIME part per attachment (real
                // attaches a `TextMessage` per package), on top of the
                // first mail's own single part. Full MIME
                // round-trip (decode each part) is covered end to end
                // in test_portuale.py with Python's own `email`
                // parser.
                assert_eq!(
                    body.matches("Content-Type: text/plain").count(),
                    4,
                    "{body}"
                );

                // Finalize with nothing accumulated sends nothing.
                let before = std::fs::read(dir.join("body")).unwrap().len();
                mail_summary_finalize();
                assert_eq!(std::fs::read(dir.join("body")).unwrap().len(), before);
            },
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
