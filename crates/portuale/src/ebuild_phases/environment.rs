//! The build environment of one package: paths, directories, fake DISTDIR.

use crate::ebuild_phases::*;

/// The directory layout `doebuild_environment()` computes, `PORTAGE_BUILDDIR`-relative.
/// Portage: doebuild.py, doebuild_environment
pub(crate) struct Environment {
    pub(crate) ebuild_abs: PathBuf,
    pub(crate) pkg_dir: PathBuf,
    pub(crate) category: String,
    pub(crate) split: PackageSplit,
    pub(crate) eapi: String,
    pub(crate) portage_tmpdir: PathBuf,
    pub(crate) portage_builddir: PathBuf,
    /// Space-joined eclass names for `INHERITED`, from the ebuild's md5-cache entry
    /// (`_eclasses_`, else the older `INHERITED` list); unset outside a repo.
    /// Exported into every phase env (see `phase_env_vars`).
    pub(crate) inherited: Option<String>,
}
