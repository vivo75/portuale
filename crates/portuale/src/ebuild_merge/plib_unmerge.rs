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

/// Arguments of [`find_unused_preserved_libs`].
#[cfg(test)]
pub(crate) struct FindUnusedPreservedLibsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) unmerge_no_replacement: bool,
    pub(crate) being_unmerged: &'a dyn Fn(&str) -> bool,
    pub(crate) exclude_cpvs: &'a [String],
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
}
