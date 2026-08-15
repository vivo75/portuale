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
