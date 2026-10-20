//! Elog message collection and dispatch (Portage's `lib/portage/elog/`).
//!
//! After a merge, per-phase `${T}/logging/` files are filtered by class
//! and handed to the modules in `PORTAGE_ELOG_SYSTEM` (`echo`, `save`,
//! `save_summary`, `syslog`, `custom`, `mail`, `mail_summary`).
//! Settings come from the env only; the logdir is root-relative.

use crate::color::Colorizer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Phase order for message printing, regardless of file mtime.
/// Portage: const.EBUILD_PHASES.
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
    /// Which `${T}/logging/<phase>` file this came from; headers emit on
    /// every phase/level change.
    pub phase: String,
    /// `LOG` / `INFO` / `WARN` / `ERROR` / `QA`.
    pub level: String,
    pub text: String,
}

/// One package's worth of collected, class-filtered elog messages.
pub struct ElogPackage {
    /// `cat/pkg-ver`.
    pub cpv: String,
    /// The `ROOT` the package merged to (`/` renders the short header).
    pub root: String,
    /// `PORTAGE_LOG_FILE` when `PORTAGE_LOGDIR` is set (`mod_echo.process`).
    pub logfile: Option<String>,
    pub messages: Vec<ElogMessage>,
}

/// `PORTAGE_LOG_FILE` when `PORTAGE_LOGDIR` (or `PORT_LOGDIR`) is set and
/// `${T}/build.log` (or `.gz`) links into it (`mod_echo.process`).
pub(crate) fn echo_logfile_for(t_dir: &Path) -> Option<String> {
    let (logdir, _) = crate::emerge_build::foreground_log_env();
    let logdir = logdir?;
    for name in ["build.log.gz", "build.log"] {
        let link = t_dir.join(name);
        if let Ok(target) = std::fs::read_link(&link) {
            let real = if target.is_absolute() {
                target
            } else {
                t_dir.join(target)
            };
            if real.starts_with(&logdir) {
                return Some(real.display().to_string());
            }
        }
    }
    None
}

fn elog_system_tokens() -> Vec<String> {
    std::env::var("PORTAGE_ELOG_SYSTEM")
        .unwrap_or_else(|_| "save_summary:log,warn,error,qa echo".to_string())
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Whether module `name` is listed in `PORTAGE_ELOG_SYSTEM` (bare name
/// or `name:levels`); `-` is accepted for `_`.
pub fn module_enabled(name: &str) -> bool {
    elog_system_tokens()
        .iter()
        .any(|t| t.split(':').next().map(|m| m.replace('-', "_")).as_deref() == Some(name))
}

/// Whether the `echo` module is enabled (default: yes).
pub fn echo_enabled() -> bool {
    module_enabled("echo")
}

/// `PORTAGE_ELOG_CLASSES`, uppercased (default `"log warn error"`).
fn portage_elog_classes() -> HashSet<String> {
    std::env::var("PORTAGE_ELOG_CLASSES")
        .unwrap_or_else(|_| "log warn error".to_string())
        .split_whitespace()
        .map(|c| c.to_uppercase())
        .collect()
}

/// The uppercased message classes module `name` shows: its own
/// `name:levels` override if present, else `PORTAGE_ELOG_CLASSES`.
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

/// Reads `${t_dir}/logging/*` for one package, in phase order, unfiltered.
/// Empty when there is nothing there.
/// Portage: collect_ebuild_messages.
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

/// `collect_all` restricted to the named phases; the unmerge path reads
/// `prerm`/`postrm` only so stale install-time files don't resurface.
pub fn collect_all_phases(t_dir: &Path, phases: &[&str]) -> Vec<ElogMessage> {
    collect_all(t_dir)
        .into_iter()
        .filter(|m| phases.contains(&m.phase.as_str()))
        .collect()
}

/// `collect_all` filtered to `classes`.
fn filter_by_classes<'a>(
    msgs: &'a [ElogMessage],
    classes: &HashSet<String>,
) -> Vec<&'a ElogMessage> {
    msgs.iter()
        .filter(|m| classes.contains("*") || classes.contains(&m.level))
        .collect()
}

/// `collect_all` filtered by the `echo` module's classes -- the shape
/// `echo_summary` consumes. `phases` optionally restricts the files read.
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

/// One flat string in phase order, with a `<LEVEL>: <phase>` header on
/// every (phase, level) change and a trailing blank line when non-empty.
/// `msgs` must already be in `collect_all`'s phase order.
/// Portage: _combine_logentries.
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

/// `$PORTAGE_LOGDIR` if set, else `<root>/var/log/portage`.
pub fn logdir(root: &Path) -> PathBuf {
    std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("var/log/portage"))
}

/// UTC `%Y%m%d-%H%M%S` of the current time.
fn utc_stamp() -> String {
    utc_stamp_at(std::time::SystemTime::now())
}

/// `utc_stamp`'s formatting for an arbitrary `SystemTime`, reused for
/// `.logid`-mtime-based build-log naming instead of "now".
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

/// Writes one package's `fulltext` to `<logdir>/elog/<pf>:<stamp>.log`
/// (or `<logdir>/elog/<cat>/<pf>:<stamp>.log` with `split-elog`).
/// Returns the path written.
/// Portage: mod_save.process.
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

/// Appends one package's block to `<logdir>/elog/summary.log`, with a UTC
/// stamp (not local time) for a deterministic, timezone-independent line.
/// Portage: mod_save_summary.process.
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

/// Maps a level to its syslog priority; `WARN` and `QA` fall to
/// `LOG_WARNING`. Split out for direct unit tests without `syslog(3)`.
fn syslog_priority(level: &str) -> libc::c_int {
    match level {
        "INFO" => libc::LOG_INFO,
        "ERROR" => libc::LOG_ERR,
        "LOG" => libc::LOG_NOTICE,
        _ => libc::LOG_WARNING,
    }
}

/// `"{key}: {phase}: {line}"` with the trailing newline stripped.
/// Split out for direct unit tests.
fn syslog_line(key: &str, msg: &ElogMessage) -> String {
    format!("{key}: {}: {}", msg.phase, msg.text.trim_end_matches('\n'))
}

/// One `syslog(3)` message per elog line, tagged `"portage"`, facility
/// `LOG_LOCAL5`, priority mapped from the line's level.
/// Portage: elog/mod_syslog.py, process.
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
        // SAFETY: a fixed `"%s"` format string -- `c_line` (which may
        // contain `%` from ebuild output) is never itself a format string.
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

/// Saves the file first, then runs `$PORTAGE_ELOG_COMMAND` (`bash -c`)
/// with `${LOGFILE}`/`${PACKAGE}` substituted. Returns the saved path.
/// Errors when the command is unset/empty or exits non-zero.
/// Portage: elog/mod_custom.py, process.
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

/// `PORTAGE_ELOG_MAILURI` (default `root@localhost`).
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

/// `${HOST}` for subject/from substitution: the local hostname via
/// `gethostname(2)`, no DNS lookup.
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

/// Non-ASCII becomes `\xNN`/`\uNNNN` backslash escapes (smtplib is
/// ASCII-only). Subjects go through this; bodies travel base64'd.
/// Portage: mail._force_ascii_if_necessary.
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

/// Minimal base64 (AUTH credentials, MIME bodies); standard alphabet,
/// `=` padding.
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

/// Base64 wrapped at 76 columns, like a UTF-8 `email` payload.
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

/// Local RFC-2822 stamp for the mail `Date` header.
// `libc::time_t` is deprecated in the libc crate's musl definitions
// while `localtime_r` still takes it; keep the alias for this call.
#[allow(deprecated)]
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

/// One `text/plain` UTF-8 MIME part (base64 even when pure ASCII).
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

/// `From ` mbox separator, headers, then one text part or
/// `multipart/mixed` (text first, then one part per attachment).
/// `subject` must already be `${...}`-substituted and ASCII-forced.
/// Portage: mail.create_message.
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
        // Minted from pid + time (cannot collide with a logged payload);
        // tests match the structure, never the token.
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

/// Where one message goes: an absolute-path `mailserver` pipes to that
/// sendmail binary, anything else is SMTP (`localhost:25` when bare).
/// A `:port` above 100000 means STARTTLS on `port - 100000` (a cut:
/// reported at send time, since there is no TLS stack).
/// Portage: PORTAGE_ELOG_MAILURI.
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

/// Splits `PORTAGE_ELOG_MAILURI` into its recipient and route; with no
/// second word the recipient alone (direct-SMTP default).
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

/// Sends one message over the parsed route: the sendmail binary gets
/// `-f <from> <recipient>` on argv and the message on stdin, else the
/// hand-rolled SMTP conversation below. `Err` text is already caller-ready.
/// Portage: mail.send_mail.
fn send_mail(
    from: &str,
    message: &str,
    route: &Option<MailRoute>,
    default_recipient: &str,
) -> Result<(), String> {
    match route {
        // No second MAILURI word: SMTP to localhost:25, addressed to
        // the whole URI word.
        None => smtp_send(SmtpSendArgs {
            host: "localhost",
            port: 25,
            user: "",
            passwd: "",
            recipient: default_recipient,
            from,
            message,
        }),
        Some(MailRoute::Sendmail { path }) => {
            if !std::path::Path::new(path).exists() {
                // A missing sendmail path is reported directly instead of
                // falling through to SMTP against the path as a host.
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
                // No TLS stack: report the cut and skip, keeping every
                // other module's output.
                eprintln!(
                    " elog STARTTLS delivery is not supported by portuale \
                     (no TLS stack) -- mail to {recipient} skipped"
                );
                return Ok(());
            }
            smtp_send(SmtpSendArgs {
                host,
                port: *port,
                user,
                passwd,
                recipient,
                from,
                message,
            })
        }
    }
}

/// One SMTP response: `(code, lines)` -- `250-...` continues, `250 ...` ends.
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

/// Arguments of [`smtp_send`].
struct SmtpSendArgs<'a> {
    host: &'a str,
    port: u16,
    user: &'a str,
    passwd: &'a str,
    recipient: &'a str,
    from: &'a str,
    message: &'a str,
}

/// Hand-rolled SMTP subset: greeting, EHLO (extensions pick the AUTH
/// choice), optional `AUTH LOGIN` (then `AUTH PLAIN`), `MAIL`/`RCPT`/`DATA`
/// with dot-stuffing, `QUIT`.
fn smtp_send(args: SmtpSendArgs<'_>) -> Result<(), String> {
    let SmtpSendArgs {
        host,
        port,
        user,
        passwd,
        recipient,
        from,
        message,
    } = args;
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
                // Empty authorize-id (`\0user\0pass` initial response).
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
        // Dot-stuffing.
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

/// The `${ACTION}` substitution: `merged` normally, `unmerged` when only
/// `*rm` (or `other`) phases appear, `unknown` when mixed.
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
/// One mail for package `key` right away. `phases` feeds the `${ACTION}`
/// call; `fulltext` is the class-filtered combined text.
/// Portage: mod_mail.process.
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

/// Accumulated `mod_mail_summary` state: first-seen settings snapshot +
/// per-package header+fulltext, narrowed to one config per process.
struct MailSummaryState {
    config: Option<(String, String, String)>,
    items: Vec<(String, String)>,
}

/// Process-lifetime `mail_summary` accumulation, finalized once at
/// process exit.
static MAIL_SUMMARY: std::sync::Mutex<MailSummaryState> = std::sync::Mutex::new(MailSummaryState {
    config: None,
    items: Vec::new(),
});

/// Stashes `header + fulltext` for `key`, snapshotting the three settings
/// on first use (the first config wins for the whole run).
/// Portage: mod_mail_summary.process.
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
    // First accumulation arms the exit-time finalize; `Once` keeps one
    // registration per process.
    static REGISTER_ONCE: std::sync::Once = std::sync::Once::new();
    REGISTER_ONCE.call_once(|| {
        // SAFETY: plain function pointer, no captures; the handler
        // only locks `MAIL_SUMMARY` and does I/O, and process exit
        // (not `_exit`/abort) is the only path that runs it.
        unsafe {
            libc::atexit(mail_summary_atexit);
        }
    });
}

/// One multipart mail for every accumulated package (or nothing when no
/// package produced messages). Always clears the accumulation, sent or not.
/// Portage: mod_mail_summary.finalize.
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
        // One part per package, each base64'd like the lead part.
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

/// `save` / `save_summary` / `syslog` / `custom` / `mail` / `mail_summary`
/// for one merged package: per-module `fulltext` from `all_msgs`, handed
/// to whichever modules are enabled. A module with nothing left after
/// filtering is skipped. Returns the paths written, for the caller's log
/// line. `mail_summary` only accumulates; its mail goes out at exit.
/// Portage: per-module `process`.
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
            // Only the phases present matter (`${ACTION}` detection).
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

/// Arguments of [`process_batch`].
pub struct ProcessBatchArgs<'a> {
    pub logdir: &'a Path,
    pub root_display: &'a str,
    pub items: &'a [(String, PathBuf)],
    pub phases: Option<&'a [&'a str]>,
    pub color: &'a Colorizer,
    pub split_elog: bool,
}

/// `elog_process` over packages that just merged or unmerged. Each item
/// is `(cpv, t_dir)` with that package's `${T}`. `phases` restricts the
/// files read: `None` after a merge, `Some(&["prerm", "postrm"])` for an
/// unmerge. `root_display` is the `ROOT` string for the `echo` header.
/// Portage: elog_process.
///
/// `save` / `save_summary` run immediately per package; `echo` accumulates
/// for the call and prints at its end; `mail` sends immediately per
/// package; `mail_summary` accumulates and sends once at process exit.
/// A no-op when no module is enabled or nothing has messages.
/// `split_elog` is the `split-elog` feature token.
pub fn process_batch(args: ProcessBatchArgs<'_>) {
    let ProcessBatchArgs {
        logdir,
        root_display,
        items,
        phases,
        color,
        split_elog,
    } = args;
    let echo = echo_enabled();
    // Gates the one `collect_all`/`save_modules_process` call below, so it
    // needs every module that call dispatches to, not just save/summary.
    let save_any = module_enabled("save")
        || module_enabled("save_summary")
        || module_enabled("syslog")
        || module_enabled("custom")
        || module_enabled("mail")
        || module_enabled("mail_summary");
    if !(echo || save_any) {
        return;
    }
    let mut packages = Vec::new();
    for (cpv, t_dir) in items {
        if save_any {
            let all = match phases {
                Some(p) => collect_all_phases(t_dir, p),
                None => collect_all(t_dir),
            };
            if !all.is_empty() {
                match save_modules_process(logdir, cpv, &all, split_elog) {
                    Ok(_) => {}
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
                    logfile: echo_logfile_for(t_dir),
                    messages,
                });
            }
        }
    }
    if echo {
        echo_summary(&packages, color);
    }
}

/// Formats the `* Messages for package <cpv>:` block for every accumulated
/// package (`mod_echo._finalize`).
pub fn format_echo_summary(packages: &[ElogPackage], color: &Colorizer) -> String {
    let mut out = String::new();
    for pkg in packages {
        out.push('\n');
        let key = color.c("INFORM", &pkg.cpv);
        let star = color.c("INFO", " * ");
        if pkg.root == "/" {
            out.push_str(&format!("{star}Messages for package {key}:\n"));
        } else {
            out.push_str(&format!(
                "{star}Messages for package {key} merged to {}:\n",
                pkg.root
            ));
        }
        if let Some(logfile) = &pkg.logfile {
            out.push_str(&format!("{star}Log file: {}\n", color.c("INFORM", logfile)));
        }
        out.push('\n');
        for msg in &pkg.messages {
            let style = match msg.level.as_str() {
                "INFO" => "INFO",
                "LOG" => "LOG",
                "WARN" => "WARN",
                "ERROR" => "ERR",
                _ => "QAWARN", // QA
            };
            out.push_str(&format!("{}{}\n", color.c(style, " * "), msg.text));
        }
    }
    out
}

/// The `* Messages for package <cpv>:` block for every accumulated
/// package, all message types on stdout.
/// Portage: mod_echo._finalize.
pub fn echo_summary(packages: &[ElogPackage], color: &Colorizer) {
    print!("{}", format_echo_summary(packages, color));
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tmpdir() -> std::path::PathBuf {
        let d = TempDir::new("elog_test").keep();
        std::fs::create_dir_all(d.join("logging")).unwrap();
        d
    }

    #[test]
    fn collect_reads_phase_files_in_order_and_filters_by_class() {
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
        // A builddir never cleaned: install-time logs still sit next to
        // the removal-time ones. The `("prerm","postrm")` filter must
        // ignore the install log.
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
        // The echo-filtered variant drops WARN only if classes say so;
        // default classes keep it, so both survive here.
        assert_eq!(collect(&t, Some(&["prerm", "postrm"])).len(), 2);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn echo_summary_renders_the_portage_message_block_shape() {
        let color = Colorizer::new(false);
        let pkg = ElogPackage {
            cpv: "dev-libs/foo-1.0".to_string(),
            root: "/".to_string(),
            logfile: None,
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
        assert_eq!(
            format_echo_summary(std::slice::from_ref(&pkg), &color),
            "\n * Messages for package dev-libs/foo-1.0:\n\n * read the docs\n * watch out\n"
        );
        let with_log = ElogPackage {
            cpv: "dev-libs/foo-1.0".to_string(),
            root: "/target".to_string(),
            logfile: Some("/var/log/portage/dev-libs:foo-1.0:20261020-120000.log".to_string()),
            messages: vec![ElogMessage {
                phase: "postinst".to_string(),
                level: "LOG".to_string(),
                text: "read the docs".to_string(),
            }],
        };
        assert_eq!(
            format_echo_summary(std::slice::from_ref(&with_log), &color),
            "\n * Messages for package dev-libs/foo-1.0 merged to /target:\n * Log file: /var/log/portage/dev-libs:foo-1.0:20261020-120000.log\n\n * read the docs\n"
        );
        echo_summary(std::slice::from_ref(&pkg), &color);
    }

    #[test]
    fn echo_logfile_for_returns_portage_logdir_target_only_when_configured() {
        let d = TempDir::new("elog_logfile_test").keep();
        let builddir = d.join("portage/dev-libs/foo-1.0");
        let t_dir = builddir.join("temp");
        let logdir = d.join("logs");
        std::fs::create_dir_all(&t_dir).unwrap();
        crate::emerge_build::ensure_portage_logdir_symlink(
            crate::emerge_build::EnsurePortageLogdirSymlinkArgs {
                tmpdir_log_path: &t_dir.join("build.log"),
                builddir: &builddir,
                category: "dev-libs",
                pf: "foo-1.0",
                logdir: Some(&logdir),
                sep: ":",
                split_log: false,
                compress: false,
            },
        );
        temp_env(&[("PORTAGE_LOGDIR", None), ("PORT_LOGDIR", None)], || {
            assert_eq!(echo_logfile_for(&t_dir), None);
        });
        let logdir_str = logdir.to_str().unwrap().to_string();
        temp_env(
            &[
                ("PORTAGE_LOGDIR", Some(logdir_str.as_str())),
                ("PORT_LOGDIR", None),
            ],
            || {
                let got = echo_logfile_for(&t_dir).expect("PORTAGE_LOGDIR symlink should resolve");
                assert!(got.starts_with(&logdir_str), "got: {got}");
                assert!(got.contains("dev-libs:foo-1.0:"), "got: {got}");
            },
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn module_classes_honours_the_per_module_override() {
        // `save_summary:log,warn,error,qa` (the default token) overrides
        // the bare PORTAGE_ELOG_CLASSES for that module only.
        temp_env(
            &[(
                "PORTAGE_ELOG_SYSTEM",
                Some("save_summary:log,warn,error,qa echo"),
            )],
            || {
                let sc = module_classes("save_summary");
                assert!(sc.contains("QA"));
                assert!(sc.contains("LOG"));
                let ec = module_classes("echo");
                assert!(!ec.contains("QA"));
                assert!(ec.contains("WARN"));
            },
        );
    }

    #[test]
    fn combine_logentries_matches_portage_combine_shape() {
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
                let saved: Vec<_> = portage_util::read_dir_entries(&elog)
                    .unwrap()
                    .into_iter()
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
    fn syslog_priority_matches_portage_mod_syslogs_own_pri_map() {
        assert_eq!(syslog_priority("INFO"), libc::LOG_INFO);
        assert_eq!(syslog_priority("ERROR"), libc::LOG_ERR);
        assert_eq!(syslog_priority("LOG"), libc::LOG_NOTICE);
        assert_eq!(syslog_priority("WARN"), libc::LOG_WARNING);
        assert_eq!(syslog_priority("QA"), libc::LOG_WARNING);
    }

    #[test]
    fn syslog_line_matches_portage_key_phase_line_shape() {
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
        // Bare recipient: direct SMTP to localhost:25.
        let (to, route) = parse_mailuri("root@localhost");
        assert_eq!(to, "root@localhost");
        assert!(route.is_none());
        let (to, route) = parse_mailuri("root@localhost /usr/sbin/sendmail");
        assert_eq!(to, "root@localhost");
        assert!(
            matches!(route, Some(MailRoute::Sendmail { path }) if path == "/usr/sbin/sendmail")
        );
        let (to, route) = parse_mailuri("me@example.com mail.example.com");
        assert_eq!(to, "me@example.com");
        assert!(
            matches!(route, Some(MailRoute::Smtp { host, port, starttls, user, .. })
                if host == "mail.example.com" && port == 25 && !starttls && user.is_empty())
        );
        let (to, route) = parse_mailuri("me@example.com user:secret@mail.example.com:587");
        assert_eq!(to, "me@example.com");
        assert!(
            matches!(route, Some(MailRoute::Smtp { host, port, user, passwd, .. })
                if host == "mail.example.com" && port == 587 && user == "user" && passwd == "secret")
        );
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
        // Bodies are base64 even when pure ASCII.
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
        let dir = TempDir::new("elog-mail-test").keep();
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
                // The summary substitutes only `${PACKAGE}` and `${HOST}`;
                // a `${ACTION}` in the template stays literal.
                assert!(
                    body.contains(&format!(
                        "Subject: [portage] ${{ACTION}} multiple packages on {}",
                        mail_host()
                    )),
                    "{body}"
                );
                // The lead part is base64, so the package list is asserted
                // in its encoded form.
                let lead = format!(
                    "elog messages for the following packages generated by process {} on host {}:\n- dev-libs/foo-1.0\n- dev-libs/bar-2.0\n",
                    std::process::id(),
                    mail_host(),
                );
                assert!(body.contains(base64_wrapped(&lead).trim_end()), "{body}");
                // Lead part + one MIME part per attachment, on top of the
                // first mail's own single part. Full MIME round-trip is
                // covered end to end in test_portuale.py.
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
