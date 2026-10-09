// Native phase helpers (#326 S2, decisions D1/D2/D3/D6/D7/D8).
//
// `portuale __helper <name> ...` is dispatched first in `main()`, from
// `args_os()` (never `args()`: helpers receive raw filenames that may not
// be UTF-8), before anything else. It never calls `bin_dir()` (no
// extraction, no atexit), loads no config, and writes only its own
// outputs (D7).
//
// Names: `python` (the D1 dispatcher: the native
// `filter-bash-environment` (S3) plus the transition table holding
// every not-yet-ported script of 0.3), `chmod-lite` (D2/D3),
// `ebuild-ipc` (always 127, D6), `ping` (a fixed token, D8). Anything
// unknown exits 127 with `portuale: no native helper for: <argv>`, so
// an upstream re-sync that adds a Python call fails loudly (D1).

mod chmod_lite;
mod doins;
// #329: the merge copies xattrs through the same `_copyxattr` /
// `_cmpxattr` port the helpers use.
#[cfg(test)]
pub(crate) use doins::xattr_set;
pub(crate) use doins::{copy_xattr, xattr_excluded, xattr_get, xattr_list};
mod filter_env;
mod gpkg;
mod locale;
mod python;
mod xattr;
mod xpak;

/// The fixed answer of the `ping` helper (D8: proves `$PORTUALE_BIN`
/// reaches a phase as a working binary, never the libtest harness).
pub(crate) const PING_TOKEN: &str = "portuale-helper-ping-1";

/// Run the helper named by `argv[0]`; `argv` is everything after
/// `__helper`. Returns the process exit code.
pub(crate) fn run(argv: &[std::ffi::OsString]) -> i32 {
    let name = argv
        .first()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.as_str() {
        "python" => python::run(&argv[1..]),
        "chmod-lite" => chmod_lite::run(&argv[1..]),
        "ebuild-ipc" => {
            eprintln!("portuale: ebuild-ipc is not supported (no IPC daemon)");
            127
        }
        "ping" => {
            println!("{PING_TOKEN}");
            0
        }
        _ => {
            no_native_helper(argv);
            127
        }
    }
}

/// The D1 unknown-helper message: `argv` joined by spaces (lossy), exit
/// 127. For the `python` dispatcher the caller passes the full
/// `["python", ...]` argv so the message reads
/// `portuale: no native helper for: python <argv>`.
fn no_native_helper(argv: &[std::ffi::OsString]) {
    let joined = argv
        .iter()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("portuale: no native helper for: {joined}");
}

#[cfg(test)]
mod dispatcher_tests {
    /// The `portuale` binary built next to this test binary (child
    /// processes, never in-process: the native helpers run across a
    /// real process boundary).
    fn portuale_exe() -> std::path::PathBuf {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe
    }

    fn helper(args: &[&str]) -> std::process::Output {
        std::process::Command::new(portuale_exe())
            .arg("__helper")
            .args(args)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .output()
            .expect("portuale __helper spawns")
    }

    /// S2 dispatcher table: an unknown helper name exits 127 with the
    /// `no native helper` message naming the full argv.
    #[test]
    fn an_unknown_helper_name_exits_127_with_the_message() {
        let out = helper(&["frobnicate", "a", "b"]);
        assert_eq!(out.status.code(), Some(127), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("portuale: no native helper for: frobnicate a b"),
            "{err}"
        );
    }

    /// S2/D6: `ebuild-ipc` always exits 127 (never 1: exit 1 would make
    /// `has_version` silently answer "not installed").
    #[test]
    fn ebuild_ipc_always_exits_127() {
        let out = helper(&["ebuild-ipc", "has_version", "/", "x"]);
        assert_eq!(out.status.code(), Some(127), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("portuale: ebuild-ipc is not supported (no IPC daemon)"),
            "{err}"
        );
    }

    /// S2/D8: `ping` prints the fixed token and exits 0.
    #[test]
    fn ping_prints_the_fixed_token() {
        let out = helper(&["ping"]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            super::PING_TOKEN
        );
    }

    /// S2 dispatcher table: `python` with anything but the probe program
    /// or a transition script exits 127 with the `python <argv>` message.
    #[test]
    fn python_with_an_unknown_program_exits_127() {
        let out = helper(&["python", "-c", "print(1)"]);
        assert_eq!(out.status.code(), Some(127), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("portuale: no native helper for: python -c print(1)"),
            "{err}"
        );
    }

    /// S7 dispatcher table: the transition table is empty (every 0.3
    /// script is natively ported), so a never-in-scope script like
    /// `dohtml.py` exits 127 with the `python <argv>` message — the same
    /// as any other unknown name. (`dohtml` dies first for EAPI >= 7,
    /// the ebuild floor, so it has no native port and never will.)
    #[test]
    fn python_with_a_non_native_script_exits_127() {
        for argv in [&["dohtml.py", "x"][..], &["frobnicate.py", "--dump"][..]] {
            let mut full = vec!["python"];
            full.extend(argv);
            let out = helper(&full);
            assert_eq!(out.status.code(), Some(127), "{out:?}");
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(
                err.contains(&format!(
                    "portuale: no native helper for: {}",
                    full.join(" ")
                )),
                "{err}"
            );
        }
    }

    /// S7 dispatcher table: the last native rows route without any
    /// interpreter or checkout (`xattr-helper.py --help` answers
    /// natively even with both disabled).
    #[test]
    fn python_xattr_helper_routes_natively_without_interpreter_or_checkout() {
        let out = std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "xattr-helper.py", "--help"])
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .env("PORTUALE_REAL_PYTHON", "/nonexistent")
            .output()
            .expect("portuale __helper spawns");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.starts_with("usage: xattr-helper.py"), "{stdout}");
    }

    /// S2 dispatcher table: `-c` with the exact QA probe program answers
    /// natively (the full locale table is `locale::table_tests`).
    #[test]
    fn python_c_with_the_exact_probe_program_answers_natively() {
        let out = std::process::Command::new(portuale_exe())
            .args(["__helper", "python", "-c", super::locale::PROBE_PROGRAM])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .output()
            .expect("portuale __helper spawns");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "UTF-8");
    }
}
