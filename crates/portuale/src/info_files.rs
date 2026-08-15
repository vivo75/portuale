//! Regenerate stale GNU info directory indexes (`<infodir>/dir`) after a
//! vdb-changing merge, by running the host `/usr/bin/install-info` over
//! each info file in them. Without the host binary the whole run is a
//! silent no-op.
// Portage: _info_files.py, chk_updated_info_files (called from post_emerge.py, post_emerge)

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// An `already installed` file is not an error.
const ALREADY_EXISTS: &str = "already exists, for file `";

/// The (harmless) no-DIR-header warning prefix: `install-info: warning:
/// no info dir entry in ` (all 44 chars).
const NO_DIR_ENTRY_PREFIX: &str = "install-info: warning: no info dir entry in ";

/// How one install-info run's merged output classifies: silent, `already
/// exists`, or the no-entry warning are all harmless (`None`); anything
/// else is collected error text (`Some`).
fn classify_output(myso: &str) -> Option<&str> {
    if myso.is_empty() {
        return None;
    }
    if myso.contains(ALREADY_EXISTS) {
        return None;
    }
    if myso.starts_with(NO_DIR_ENTRY_PREFIX) {
        return None;
    }
    Some(myso)
}
