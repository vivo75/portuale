//! The terminal title Portage sets around an `emerge` run: `\x1b]0;<host>:
//! emerge\x07` on stderr at the start, the `LOGNAME@HOST:PWD` title (or the
//! `$PROMPT_COMMAND` evaluation) again at the exit. Only when stderr is a
//! tty, `$TERM` matches Portage's list, and `FEATURES` lacks `notitles`.
//! Portage: lib/portage/output.py, lib/_emerge/actions.py,
//! lib/_emerge/main.py

use std::io::IsTerminal;
use std::io::Write;

/// `$TERM` values Portage touches the title for (prefix match).
// Portage: output.py, _legal_terms_re
const TITLE_TERMS: &[&str] = &[
    "xterm",
    "xterm-color",
    "Eterm",
    "aterm",
    "rxvt",
    "screen",
    "kterm",
    "rxvt-unicode",
    "gnome",
    "interix",
    "tmux",
    "st-256color",
    "alacritty",
    "konsole",
    "foot",
];

/// Whether `$TERM` allows the title dance: present and prefix-matching
/// Portage's list (`re.match` anchors at the start only).
// Portage: output.py, xtermTitle
pub(crate) fn term_allows_title(term: Option<&str>) -> bool {
    term.is_some_and(|t| TITLE_TERMS.iter().any(|p| t.starts_with(p)))
}

/// Whether this run sets and restores the title: stderr is a tty, `$TERM`
/// matches, and `FEATURES` lacks `notitles`.
// Portage: output.py, xtermTitle; _emerge/actions.py, run_action
pub(crate) fn title_enabled(features: &[String]) -> bool {
    std::io::stderr().is_terminal()
        && term_allows_title(std::env::var("TERM").ok().as_deref())
        && !features.iter().any(|f| f == "notitles")
}

/// `socket.gethostname()`, `None` when the call fails (then no title is
/// set at all).
fn title_host() -> Option<String> {
    let mut buf = [0 as libc::c_char; 256];
    // SAFETY: `buf` is a valid 256-byte stack array; `gethostname`
    // writes at most its length including the NUL.
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } != 0 {
        return None;
    }
    // SAFETY: NUL-terminated by `gethostname` on success.
    Some(
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// `\x1b]0;<host>: emerge\x07`, the whole title truncated to Portage's
/// 253-character cap (Python-slice semantics: characters, not bytes).
// Portage: output.py, xtermTitle
fn start_title(host: &str) -> Vec<u8> {
    let mut title = format!("{host}: emerge");
    if title.len() > 253 {
        let end = title
            .char_indices()
            .map(|(i, _)| i)
            .nth(253)
            .unwrap_or(title.len());
        title.truncate(end);
    }
    format!("\x1b]0;{title}\x07").into_bytes()
}

/// The reset title for explicit parts: `$PWD` abbreviated with `~`
/// like Portage (`$HOME`-prefixed → `~`-prefixed, byte-prefix rule).
// Portage: output.py, xtermTitleReset
fn default_title_for(logname: &str, hostname: &str, pwd: &str, home: &str) -> Vec<u8> {
    let host = hostname.split('.').next().unwrap_or("");
    let pwd = if !home.is_empty() && pwd.starts_with(home) {
        format!("~{}", &pwd[home.len()..])
    } else {
        pwd.to_string()
    };
    format!("\x1b]0;{logname}@{host}:{pwd}\x07").into_bytes()
}

fn write_stderr(bytes: &[u8]) {
    let mut err = std::io::stderr();
    let _ = err.write_all(bytes);
    let _ = err.flush();
}

/// `$SHELL -c "$PROMPT_COMMAND"` with stdin inherited and both outputs
/// on stderr, like Portage's `fd_pipes={0: stdin, 1: stderr, 2: stderr}`.
/// Falls back to `sh` when `$SHELL` is unset or not executable.
// Portage: output.py, xtermTitleReset
fn run_prompt_command(cmd: &str) {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;
    use std::process::Stdio;
    let shell = std::env::var_os("SHELL").filter(|s| {
        !s.is_empty()
            && std::ffi::CString::new(s.as_bytes()).is_ok_and(|c| {
                // SAFETY: `access` reads only the path; `X_OK` has no
                // side effects.
                unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
            })
    });
    let shell: &std::ffi::OsStr = shell.as_deref().unwrap_or(std::ffi::OsStr::new("sh"));
    // A dup of stderr for the child's stdout, so both land on stderr.
    // SAFETY: `dup` returns an owned fd (or -1); the `OwnedFd` closes it.
    let stdout_to_stderr = unsafe {
        let fd = libc::dup(libc::STDERR_FILENO);
        if fd >= 0 {
            Stdio::from(std::os::fd::OwnedFd::from_raw_fd(fd))
        } else {
            Stdio::inherit()
        }
    };
    let _ = std::process::Command::new(shell)
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::inherit())
        .stdout(stdout_to_stderr)
        .stderr(Stdio::inherit())
        .status();
}

/// The exit half: with `$PROMPT_COMMAND` set, evaluate it on stderr; with
/// it set-but-empty, write nothing; otherwise the reset title. Only runs
/// when the start title was set.
// Portage: output.py, xtermTitleReset
fn reset_title() {
    match std::env::var("PROMPT_COMMAND") {
        Ok(cmd) if cmd.is_empty() => {}
        Ok(cmd) => run_prompt_command(&cmd),
        Err(_) => write_stderr(&default_title_for(
            &std::env::var("LOGNAME").unwrap_or_default(),
            &std::env::var("HOSTNAME").unwrap_or_default(),
            &std::env::var("PWD").unwrap_or_default(),
            &std::env::var("HOME").unwrap_or_default(),
        )),
    }
}

/// Writes the start title on construction and the reset title on drop,
/// when [`title_enabled`]; a plain value otherwise. Portage resets in
/// `main.py`'s `finally:`, so every return path restores it.
// Portage: _emerge/main.py, emerge_main
pub(crate) struct TitleGuard {
    enabled: bool,
}

impl TitleGuard {
    pub(crate) fn set(features: &[String]) -> Self {
        let enabled = title_host()
            .filter(|_| title_enabled(features))
            .map(|host| write_stderr(&start_title(&host)))
            .is_some();
        Self { enabled }
    }
}

impl Drop for TitleGuard {
    fn drop(&mut self) {
        if self.enabled {
            reset_title();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_prefix_table_matches_portage() {
        for term in [
            "xterm",
            "xterm-256color",
            "xterm-color",
            "screen-256color",
            "tmux",
            "alacritty",
            "foot",
            "konsole",
            "rxvt-unicode",
            "Eterm",
        ] {
            assert!(term_allows_title(Some(term)), "{term}");
        }
        for term in [None, Some(""), Some("dumb"), Some("vt100"), Some("linux")] {
            assert!(!term_allows_title(term), "{term:?}");
        }
    }

    #[test]
    fn reset_title_abbreviates_pwd_with_tilde() {
        assert_eq!(
            default_title_for("vivo", "portuale.box", "/home/vivo/work", "/home/vivo"),
            b"\x1b]0;vivo@portuale:~/work\x07".to_vec()
        );
        assert_eq!(
            default_title_for("vivo", "", "/var/tmp/x", "/home/vivo"),
            b"\x1b]0;vivo@:/var/tmp/x\x07".to_vec()
        );
    }
}
