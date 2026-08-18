//! Build logs.

use crate::emerge_build::*;

/// Arguments of [`ensure_portage_logdir_symlink`].
pub(crate) struct EnsurePortageLogdirSymlinkArgs<'a> {
    pub(crate) tmpdir_log_path: &'a Path,
    pub(crate) builddir: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) logdir: Option<&'a Path>,
    pub(crate) sep: &'a str,
    pub(crate) split_log: bool,
    pub(crate) compress: bool,
}
