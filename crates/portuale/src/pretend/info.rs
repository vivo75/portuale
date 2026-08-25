//! `emerge --info`.

use crate::pretend::*;

/// The highest installed version of `cat/pkg` (`-r0` dropped), if any.
pub(crate) fn highest_installed_pvr(root: &Path, category: &str, package: &str) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .max_by(|a, b| portage_versions::vercmp(a, b).unwrap_or(0).cmp(&0))
        .map(|v| v.strip_suffix("-r0").unwrap_or(&v).to_string())
}
