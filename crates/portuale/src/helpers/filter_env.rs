// Native `filter-bash-environment`: keeps only the wanted variables.
//
// A byte-level port of `filter_bash_environment()`: stdin streams to
// stdout over raw bytes with no UTF-8 decoding; per line, first match
// wins (multi-line-quote continuation, then assignments and bare declares
// outside here-docs and functions, then here-doc and function lines).
// Python `re` on bytes becomes `regex::bytes::Regex` with `(?-u)`,
// `\A` for `.match` anchoring and `\n?\z` for a trailing `$`.
// Portage: bin/filter-bash-environment.py, filter_bash_environment

use std::borrow::Cow;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;

/// The raw basename of a script argv element (bytes after the last
/// `/`), like `os.path.basename`.
fn basename_of(arg: &OsString) -> &[u8] {
    let bytes = arg.as_os_str().as_bytes();
    match bytes.iter().rposition(|b| *b == b'/') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    }
}

/// Translate raw PATTERN bytes: ASCII passes through, non-ASCII becomes
/// `\xHH`. `None` when a `\`-escape of a non-ASCII byte diverges.
fn rust_pattern_source(joined: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(joined.len());
    let mut escaped = false;
    for &b in joined {
        if escaped {
            if b >= 0x80 {
                return None;
            }
            out.push(b as char);
            escaped = false;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            out.push('\\');
        } else if b < 0x80 {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::Path;
    use std::process::Stdio;

    /// The `portuale` binary next to this test binary (child processes:
    /// the helper reads stdin, so only a real process boundary matches).
    fn portuale_exe() -> std::path::PathBuf {
        crate::self_exe::built_portuale_bin()
    }

    /// Run the native filter as the shim would, stdin bytes verbatim
    /// (the `args` file bytes ARE argv[1], with no added newline).
    fn run_native(pattern: &[u8], stdin_bytes: &[u8]) -> std::process::Output {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // A pipe could deadlock past one pipe buffer (both sides stream),
        // so stdin goes through a temp file while stdout/stderr stream.
        let tmp = portage_util::TempDir::new("helper-filter-env-stdin");
        let input_path = tmp.join("in");
        std::fs::write(&input_path, stdin_bytes).unwrap();
        let input_file = std::fs::File::open(&input_path).unwrap();
        std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "filter-bash-environment.py"])
            .arg(OsStr::from_bytes(pattern))
            .stdin(Stdio::from(input_file))
            .output()
            .expect("portuale __helper spawns")
    }

    /// `-h`/`--help` anywhere prints the usage to stdout and exits 0.
    #[test]
    fn help_flag_prints_usage_and_exits_zero() {
        let exe = portuale_exe();
        for argv in [
            vec!["__helper", "python", "filter-bash-environment.py", "-h"],
            vec!["__helper", "python", "filter-bash-environment.py", "--help"],
            vec![
                "__helper",
                "python",
                "filter-bash-environment.py",
                "D",
                "--help",
            ],
        ] {
            let out = std::process::Command::new(&exe)
                .args(&argv)
                .output()
                .expect("portuale __helper spawns");
            assert_eq!(out.status.code(), Some(0), "{argv:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "usage: filter-bash-environment.py PATTERN\n",
                "{argv:?}"
            );
        }
    }

    /// Anything but exactly one PATTERN arg prints the usage plus the
    /// `Exactly one PATTERN argument required.` line to stderr, exit 2.
    #[test]
    fn wrong_argc_prints_usage_to_stderr_and_exits_two() {
        let exe = portuale_exe();
        for argv in [
            vec!["__helper", "python", "filter-bash-environment.py"],
            vec!["__helper", "python", "filter-bash-environment.py", "A", "B"],
        ] {
            let out = std::process::Command::new(&exe)
                .args(&argv)
                .output()
                .expect("portuale __helper spawns");
            assert_eq!(out.status.code(), Some(2), "{argv:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stderr),
                "usage: filter-bash-environment.py PATTERN\n\
                 Exactly one PATTERN argument required.\n",
                "{argv:?}"
            );
            assert!(out.stdout.is_empty(), "{argv:?}");
        }
    }
}
