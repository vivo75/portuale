//! Preserved libraries during unmerge and prune.

use crate::ebuild_merge::*;

/// Arguments of [`preserve_libs_on_unmerge`].
#[cfg(test)]
pub(crate) struct PreserveLibsOnUnmergeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) contents_text: &'a str,
    pub(crate) is_replacement: bool,
}
