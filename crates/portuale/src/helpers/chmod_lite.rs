//! Native `chmod-lite`: recursive permission normalization.
//!
//! Port of `bin/chmod-lite.py`'s `main()` over `apply_recursive_permissions`
//! (filemode 0o644/filemask 0o022, dirmode 0o755/dirmask 0o022), relative
//! to the current directory, over raw bytes (paths may not be UTF-8).
// Portage: bin/chmod-lite.py, main

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const FILEMODE: u32 = 0o644;
const FILEMASK: u32 = 0o022;
const DIRMODE: u32 = 0o755;

/// Port of `_do_stat`'s error mapping for the top-level stat: EPERM
/// prints through `onerror`, anything else Portage would let propagate.
fn report_stat_error(top: &Path, e: &std::io::Error) -> bool {
    let display = String::from_utf8_lossy(top.as_os_str().as_bytes()).into_owned();
    match e.raw_os_error() {
        Some(code) if code == libc::EPERM => {
            eprintln!("Operation Not Permitted: stat('{display}')");
            false
        }
        Some(code) if code == libc::ENOENT => {
            eprintln!("File Not Found: '{display}'");
            false
        }
        _ => {
            eprintln!("portuale: chmod-lite: cannot stat '{display}': {e}");
            true
        }
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use portage_util::TempDir;
    use std::collections::BTreeMap;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    /// One manifest entry: type, mode (files/dirs only) or target (links).
    #[derive(Debug, PartialEq)]
    enum Entry {
        Dir(u32),
        File(u32),
        Link(Vec<u8>),
    }

    fn join(root: &Path, rel: &[u8]) -> PathBuf {
        use std::ffi::OsStr;
        root.join(OsStr::from_bytes(rel))
    }
}
