// The `$PORTAGE_PYTHON` dispatcher (#326 D1 + Q7, S7): the vendored `bin/`
// keeps upstream's call sites byte for byte, and `PORTAGE_PYTHON` points
// at `bin/portuale-python`, which execs `"$PORTUALE_BIN" __helper python
// "$@"`. This dispatcher picks the helper from the script basename (the
// first argument at every 0.3 call site), or, for `-c`, from the exact
// `90config-impl-decl` program string.
//
// Every script of 0.3 is natively ported now (S7 was the last one, so the
// D1 transition table is empty and gone): `filter-bash-environment.py`
// (`helpers/filter_env.rs`, S3), `xpak-helper.py` (`helpers/xpak.rs`,
// S5), `gpkg-helper.py` (`helpers/gpkg.rs`, S4), `doins.py`
// (`helpers/doins.rs`, S6), `xattr-helper.py` + `install.py`
// (`helpers/xattr.rs`, S7). Anything else — `dohtml.py` included
// (`dohtml` dies first for EAPI >= 7, the ebuild floor, so it is never
// in scope) — exits 127 with the unknown-helper message, so an upstream
// re-sync that adds a Python call fails loudly (D1).

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

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
    // S6: `doins.py` is natively ported (`helpers/doins.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"doins.py").then_some((arg, i)))
    {
        return super::doins::run(script, &argv[index + 1..]);
    }
    // S7: `xattr-helper.py` is natively ported (`helpers/xattr.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"xattr-helper.py").then_some((arg, i)))
    {
        return super::xattr::run_xattr_helper(script, &argv[index + 1..]);
    }
    // S7: `install.py` is natively ported (`helpers/xattr.rs`).
    if let Some((script, index)) = argv
        .iter()
        .enumerate()
        .find_map(|(i, arg)| (basename(arg) == b"install.py").then_some((arg, i)))
    {
        return super::xattr::run_install(script, &argv[index + 1..]);
    }
    // No transition rows remain (S7 deleted the last ones): anything
    // else is an unknown helper (D1).
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
