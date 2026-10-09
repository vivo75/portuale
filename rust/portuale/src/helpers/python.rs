// The `$PORTAGE_PYTHON` dispatcher (#326 D1 + Q7): the vendored `bin/`
// keeps upstream's call sites byte for byte, and `PORTAGE_PYTHON` points
// at `bin/portuale-python`, which execs `"$PORTUALE_BIN" __helper python
// "$@"`. This dispatcher picks the helper from the script basename (the
// first argument at every 0.3 call site), or, for `-c`, from the exact
// `90config-impl-decl` program string.
//
// Transition table: a script with no native port yet is re-executed with
// the real interpreter (`$PORTUALE_REAL_PYTHON`, else `/usr/bin/python`)
// with the original argv, except that the script path is resolved per
// Q7: the original path when it exists; else `<checkout>/bin/<name>`
// when that exists (`<checkout>/lib` on `PYTHONPATH` when not already
// set); else the first `/usr/lib/portage/python*/<name>` that exists.
// With no script or no interpreter, exit 127 with the unknown-helper
// message, the same as an unknown name (D1). `exec` semantics keep the
// exit status and stdio the interpreter's. `filter-bash-environment.py`
// is natively ported (`helpers/filter_env.rs`, S3), and so are
// `xpak-helper.py` (`helpers/xpak.rs`, S5) and `gpkg-helper.py`
// (`helpers/gpkg.rs`, S4); all route before this table; each of S6, S7
// deletes its own row; the last asserts the table is empty.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Every script of 0.3 still awaiting its native port: each is
/// re-executed until its own slice ports it (`filter-bash-environment.py`
/// went native in S3, `xpak-helper.py` in S5 and `gpkg-helper.py` in S4;
/// all route before this table).
const TRANSITION_SCRIPTS: &[&str] = &["doins.py", "dohtml.py", "xattr-helper.py", "install.py"];

/// The real interpreter: `$PORTUALE_REAL_PYTHON`, else `/usr/bin/python`.
fn real_interpreter() -> OsString {
    std::env::var_os("PORTUALE_REAL_PYTHON").unwrap_or_else(|| OsString::from("/usr/bin/python"))
}

/// Run the `python` helper on `argv` (everything after `__helper
/// python`, exactly as the shell passed it to `$PORTAGE_PYTHON`).
pub(crate) fn run(argv: &[OsString]) -> i32 {
    // The QA probe: `-c` with the exact program string (S2.4, D3).
    if argv.len() == 2
        && argv[0].as_os_str().as_bytes() == b"-c"
        && argv[1].as_os_str().as_bytes() == super::locale::PROBE_PROGRAM.as_bytes()
    {
        super::locale::print_answer();
        return 0;
    }
    // S3: `filter-bash-environment.py` is natively ported
    // (`helpers/filter_env.rs`): the script element keeps real's
    // `sys.argv[0]` role (the usage basename) and everything after it
    // is the filter's argv (real's `sys.argv[1:]`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"filter-bash-environment.py").then_some((arg, i)))
    {
        return super::filter_env::run(script, &argv[index + 1..]);
    }
    // S5: `xpak-helper.py` is natively ported (`helpers/xpak.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"xpak-helper.py").then_some((arg, i)))
    {
        return super::xpak::run(script, &argv[index + 1..]);
    }
    // S4: `gpkg-helper.py` is natively ported (`helpers/gpkg.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"gpkg-helper.py").then_some((arg, i)))
    {
        return super::gpkg::run(script, &argv[index + 1..]);
    }
    // The first argument whose basename is a transition script.
    let found = argv.iter().enumerate().find_map(|(i, arg)| {
        let base = basename(arg);
        TRANSITION_SCRIPTS
            .iter()
            .find(|name| base == name.as_bytes())
            .map(|name| (*name, i))
    });
    let Some((name, index)) = found else {
        super::no_native_helper(&join_head(argv));
        return 127;
    };
    run_transition(name, index, argv)
}

/// The raw basename of an argv path (bytes after the last `/`).
fn basename(arg: &OsString) -> &[u8] {
    let bytes = arg.as_os_str().as_bytes();
    match bytes.iter().rposition(|b| *b == b'/') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    }
}

/// `["python"] + argv` for the unknown-helper message.
fn join_head(argv: &[OsString]) -> Vec<OsString> {
    let mut full = Vec::with_capacity(argv.len() + 1);
    full.push(OsString::from("python"));
    full.extend(argv.iter().cloned());
    full
}

/// Re-exec the real interpreter for a transition row (Q7).
fn run_transition(name: &str, index: usize, argv: &[OsString]) -> i32 {
    let Some((resolved, pythonpath)) = resolve_script(name, &argv[index]) else {
        super::no_native_helper(&join_head(argv));
        return 127;
    };
    let interpreter = real_interpreter();
    if !Path::new(&interpreter).exists() {
        super::no_native_helper(&join_head(argv));
        return 127;
    }
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(&interpreter);
    for (i, arg) in argv.iter().enumerate() {
        if i == index {
            cmd.arg(&resolved);
        } else {
            cmd.arg(arg);
        }
    }
    if let Some(lib) = pythonpath
        && std::env::var_os("PYTHONPATH").is_none()
    {
        cmd.env("PYTHONPATH", lib);
    }
    let err = cmd.exec();
    eprintln!(
        "portuale: cannot exec {}: {err}",
        Path::new(&interpreter).display()
    );
    127
}

/// Resolve a transition script per Q7: the original path when it exists;
/// else `<checkout>/bin/<name>` (with `<checkout>/lib` for `PYTHONPATH`
/// when not already set); else the first `/usr/lib/portage/python*/<name>`.
/// Returns the path plus an optional `PYTHONPATH` value.
fn resolve_script(name: &str, script_arg: &OsString) -> Option<(PathBuf, Option<PathBuf>)> {
    if Path::new(script_arg).exists() {
        return Some((PathBuf::from(script_arg), None));
    }
    // `portage_checkout()` reads only env and the build path (D7-safe).
    let checkout = crate::ebuild_phases::portage_checkout();
    let checkout_script = checkout.join("bin").join(name);
    if checkout_script.is_file() {
        return Some((checkout_script, Some(checkout.join("lib"))));
    }
    if let Ok(dir) = std::fs::read_dir("/usr/lib/portage") {
        let mut candidates: Vec<PathBuf> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("python"))
            })
            .collect();
        candidates.sort();
        for dir in candidates {
            let script = dir.join(name);
            if script.is_file() {
                return Some((script, None));
            }
        }
    }
    None
}
