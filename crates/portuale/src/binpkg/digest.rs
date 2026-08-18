//! Digest checks against the index and checksum-failure renames.

use crate::binpkg::*;

/// Whole seconds since the epoch, the same integer Portage's `_mtime_`
/// index field records.
// Portage: `bintree._pkgindex_entry`.
pub(crate) fn file_mtime(st: &fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.mtime()
}
