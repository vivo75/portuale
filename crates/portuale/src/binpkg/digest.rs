//! Digest checks against the index and checksum-failure renames.

use crate::binpkg::*;

/// Whole seconds since the epoch, the same integer Portage's `_mtime_`
/// index field records.
// Portage: `bintree._pkgindex_entry`.
pub(crate) fn file_mtime(st: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.mtime()
}

pub(crate) fn run_tar(args: &[&str]) -> Result<(), String> {
    let status = Command::new("tar")
        .args(args)
        .status()
        .map_err(|e| format!("failed to spawn tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar {args:?} failed ({status})"));
    }
    Ok(())
}

pub(crate) fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
