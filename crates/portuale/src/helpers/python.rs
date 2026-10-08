// The `$PORTAGE_PYTHON` dispatcher: the vendored `bin/` keeps upstream's
// call sites byte for byte, and `PORTAGE_PYTHON` points at
// `bin/portuale-python`, which execs `"$PORTUALE_BIN" __helper python "$@"`.
// The helper is picked from the script basename (or, for `-c`, from the
// exact locale-probe program string). Every script is natively ported;
// anything else exits 127, so a new upstream call fails loudly (D1).
// (`dohtml.py` is never in scope: `dohtml` dies first for EAPI >= 7.)

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

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
