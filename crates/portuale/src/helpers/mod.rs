// The `portuale __helper` dispatcher.
//
// Runs first in `main()`, from `args_os()` (paths may not be UTF-8),
// loading no config and writing only its own outputs (D7). Helpers:
// `python` (the script dispatcher), `chmod-lite`, `ebuild-ipc` (always
// 127: no IPC daemon) and `ping` (a fixed token). Anything unknown exits
// 127, so a new upstream call fails loudly (D1).

mod chmod_lite;
mod doins;
// The merge copies xattrs through the same `_copyxattr`/`_cmpxattr`
// port the helpers use.
#[cfg(test)]
pub(crate) use doins::xattr_set;
pub(crate) use doins::{copy_xattr, xattr_excluded, xattr_get, xattr_list};
mod filter_env;
mod gpkg;
pub(crate) use gpkg::digest_of;
mod locale;
mod python;
mod xattr;
pub(crate) mod xpak;

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
    /// processes: the helpers run across a real process boundary).
    fn portuale_exe() -> std::path::PathBuf {
        crate::self_exe::built_portuale_bin()
    }

    fn helper(args: &[&str]) -> std::process::Output {
        std::process::Command::new(portuale_exe())
            .arg("__helper")
            .args(args)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .output()
            .expect("portuale __helper spawns")
    }

    /// An unknown helper name exits 127 with the message naming the full argv.
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

    /// `ebuild-ipc` always exits 127 (never 1: that would make
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

    /// `ping` prints the fixed token and exits 0.
    #[test]
    fn ping_prints_the_fixed_token() {
        let out = helper(&["ping"]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            super::PING_TOKEN
        );
    }

    /// `python` with anything but the probe program or a known script
    /// exits 127 with the `python <argv>` message.
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

    /// A never-in-scope script like `dohtml.py` exits 127 like any other
    /// unknown name (`dohtml` dies first for EAPI >= 7, so it has no port).
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

    /// Helper scripts route without any interpreter or checkout
    /// (`xattr-helper.py --help` answers natively even with both disabled).
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

    /// `-c` with the exact QA probe program answers natively
    /// (the full locale table is `locale::table_tests`).
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
