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
    pub messages: Vec<ElogMessage>,
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

/// The `* Messages for package <cpv>:` block for every accumulated
/// package, all message types on stdout.
/// Portage: mod_echo._finalize.
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
