// Native `xattr-helper.py` + `install.py`: xattr dump/restore and install.
//
// Ports of `bin/xattr-helper.py` (`quote`, `unquote`, `dump_xattrs`,
// `restore_xattrs`) and `bin/install.py` (`parse_args`, `copy_xattrs`,
// `Which`), routed from the `python` dispatcher. `--dump` prints the
// getfattr-style text; `--restore` reads it back; `install.py` runs the
// real `install(1)` verbatim, then copies xattrs through doins.rs's
// shared `copy_xattr`. Uncaught failures print only the final
// `ExceptionType: message` line.
// Deviations: help text is pinned at 80 columns; missing env, failed
// chdir or `Which` stat print the final line only (rc 1); a
// signal-killed `install(1)` reports rc 1. Deliberately NO D7 write
// guard: the caller-named paths ARE the operation, as with `install(1)`
// itself, so there is no anchor to check against.
// Portage: bin/xattr-helper.py, main; bin/install.py, main

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

use super::doins::{copy_xattr, errno_line, xattr_get, xattr_list, xattr_set};

/// Read all of stdin as raw bytes.
fn read_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    buf
}

// ---------------------------------------------------------------------
// `xattr-helper.py` argv (argparse)
// ---------------------------------------------------------------------

// Probed from Portage `xattr-helper.py --help` (80 columns, piped).
// `xattr-helper.py` occurs in the usage line only.
const XATTR_USAGE: &str = "usage: xattr-helper.py [-h] [--dump] [--restore] [paths ...]\n";

fn xattr_usage(prog: &str) -> Vec<u8> {
    XATTR_USAGE
        .replacen("xattr-helper.py", prog, 1)
        .into_bytes()
}
