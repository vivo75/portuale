//! INSTALL_MASK and what is already installed.

use crate::ebuild_merge::*;

/// Whether the installed `<package>-<version>` already claims `abs_path`
/// in its `CONTENTS`, through the `PackagesDb` seam. `abs_path` is a
/// logical absolute path; the leading `/` is stripped to meet the seam's
/// shape.
pub(crate) fn owns_path(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    abs_path: &str,
) -> bool {
    let relative = abs_path.strip_prefix('/').unwrap_or(abs_path);
    mrg_director::VdbReader::new(root)
        .contents_files(category, package, version)
        .iter()
        .any(|path| path == relative)
}

/// The `CONTENTS` text of `category/pf`: the live entry first, falling
/// back to its `-MERGING-<pf>` temporary when the live one is absent
/// (mid-merge, the new entry is not yet renamed into place, so an
/// ownership check against the replacing version sees the just-written
/// list). Outside a merge the live entry always exists when queried.
/// Portage: vartree.py, dblink.getcontents
pub(crate) fn read_contents_pf(root: &Path, category: &str, pf: &str) -> Option<String> {
    if let Some(text) = read_entry_text(root, category, pf, "CONTENTS") {
        return Some(text);
    }
    portage_vdb::for_root(root)
        .read_pending_file(&portage_vdb::EntryKey::new(category, pf), "CONTENTS")
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// One file of the live entry `category/pf` as UTF-8 text. `None` when
/// missing, unreadable or not UTF-8.
pub(crate) fn read_entry_text(root: &Path, category: &str, pf: &str, name: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_file(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}
