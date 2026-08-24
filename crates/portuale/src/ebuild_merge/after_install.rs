//! The post-install merge driver and same-slot unmerge.

use crate::ebuild_merge::*;

/// Arguments of [`unmerge_replaced_same_slot`].
pub(crate) struct UnmergeReplacedSameSlotArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
}

/// Arguments of [`unmerge_one_installed`].
pub(crate) struct UnmergeOneInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) also_keep: &'a [String],
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) backup: Option<&'a crate::ebuild_package::PackageOptions>,
    pub(crate) is_replacement: bool,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
    pub(crate) deferred_delete: Option<&'a [String]>,
}

/// Arguments of [`run_vdb_saved_env_phase`].
pub(crate) struct RunVdbSavedEnvPhaseArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) phase: &'a str,
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
}
