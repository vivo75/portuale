//! Where the phase runtime (bin/, pym/) and the repository live; the embedded runtime.

use crate::ebuild_phases::*;

/// Locates a repo from an ebuild file by walking up for `profiles/repo_name`;
/// `None` for standalone files. Shared by `fetch_sources` and `ebuild_package`'s
/// metadata lookup.
/// Portage: portage's own repo-locating mechanism
pub(crate) fn repo_root_for(pkg_dir: &Path) -> Option<PathBuf> {
    for ancestor in pkg_dir.ancestors() {
        if ancestor.join("profiles").join("repo_name").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}
