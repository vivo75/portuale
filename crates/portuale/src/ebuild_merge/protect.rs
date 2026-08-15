//! CONFIG_PROTECT: protected paths, ._cfg names, atomic replaces, xattrs.

use crate::ebuild_merge::*;

/// Portage's `_cmpxattr`: the non-excluded attribute names and values match
/// on both files, or the filesystem has no xattr support.
/// Portage: lib/portage/util/movefile.py, _cmpxattr
pub(crate) fn xattrs_equal(src: &Path, dest: &Path, exclude: &str) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let (s, d) = (src.as_os_str().as_bytes(), dest.as_os_str().as_bytes());
    let (Ok(src_attrs), Ok(dest_attrs)) =
        (crate::helpers::xattr_list(s), crate::helpers::xattr_list(d))
    else {
        // `xattr_list` already maps ENOTSUP to empty, so an error here is
        // unreadable: treat as "differs" so the file is replaced.
        return false;
    };
    let keep = |attrs: Vec<Vec<u8>>| -> std::collections::BTreeSet<Vec<u8>> {
        attrs
            .into_iter()
            .filter(|a| !crate::helpers::xattr_excluded(exclude, a))
            .collect()
    };
    let (src_attrs, dest_attrs) = (keep(src_attrs), keep(dest_attrs));
    src_attrs == dest_attrs
        && src_attrs
            .iter()
            .all(|a| crate::helpers::xattr_get(s, a).ok() == crate::helpers::xattr_get(d, a).ok())
}
