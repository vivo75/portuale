// The `$PORTAGE_PYTHON` dispatcher: the vendored `bin/` keeps upstream's
// call sites byte for byte, and `PORTAGE_PYTHON` points at
// `bin/portuale-python`, which execs `"$PORTUALE_BIN" __helper python "$@"`.
// The helper is picked from the script basename (or, for `-c`, from the
// exact locale-probe program string). Every script is natively ported;
// anything else exits 127, so a new upstream call fails loudly (D1).
// (`dohtml.py` is never in scope: `dohtml` dies first for EAPI >= 7.)

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

/// Run the `python` helper on `argv` (everything after `__helper
/// python`, exactly as the shell passed it to `$PORTAGE_PYTHON`).
pub(crate) fn run(argv: &[OsString]) -> i32 {
    // The QA probe: `-c` with the exact program string.
    if argv.len() == 2
        && argv[0].as_os_str().as_bytes() == b"-c"
        && argv[1].as_os_str().as_bytes() == super::locale::PROBE_PROGRAM.as_bytes()
    {
        super::locale::print_answer();
        return 0;
    }
    // Each row below: the script element keeps Portage's `sys.argv[0]`
    // role (the usage basename); everything after it is that helper's argv.
    // `filter-bash-environment.py` (see `helpers/filter_env.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"filter-bash-environment.py").then_some((arg, i)))
    {
        return super::filter_env::run(script, &argv[index + 1..]);
    }
    // `xpak-helper.py` (see `helpers/xpak.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"xpak-helper.py").then_some((arg, i)))
    {
        return super::xpak::run(script, &argv[index + 1..]);
    }
    // `gpkg-helper.py` (see `helpers/gpkg.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"gpkg-helper.py").then_some((arg, i)))
    {
        return super::gpkg::run(script, &argv[index + 1..]);
    }
    // `doins.py` (see `helpers/doins.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"doins.py").then_some((arg, i)))
    {
        return super::doins::run(script, &argv[index + 1..]);
    }
    // `xattr-helper.py` (see `helpers/xattr.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"xattr-helper.py").then_some((arg, i)))
    {
        return super::xattr::run_xattr_helper(script, &argv[index + 1..]);
    }
    // `install.py` (see `helpers/xattr.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"install.py").then_some((arg, i)))
    {
        return super::xattr::run_install(script, &argv[index + 1..]);
    }
    // Anything else is an unknown helper (D1).
    super::no_native_helper(&join_head(argv));
    127
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
