//! The resolver parameters derived from the options: bdeps, selective, autounmask, binary packages, rebuild and complete-graph switches, seed atoms.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// What [`plan_resolve`] hands back to its caller.
pub(crate) struct ResolvePlan {
    pub(crate) selective: bool,
    pub(crate) autounmask_suggest_keywords: bool,
    pub(crate) autounmask_suggest_use: bool,
    pub(crate) autounmask_suggest_license: bool,
    pub(crate) autounmask_suggest_masks: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) usepkg: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) binpkg_respect_use: bool,
    pub(crate) rebuilt_binaries: bool,
    pub(crate) root_deps_running_root: Option<PathBuf>,
    pub(crate) running_config: Option<Config>,
    pub(crate) distdir: PathBuf,
    pub(crate) rebuild_if_unbuilt: bool,
    pub(crate) rebuild_if_new_rev: bool,
    pub(crate) rebuild_if_new_ver: bool,
    pub(crate) dynamic_deps: bool,
    pub(crate) complete_graph: bool,
    pub(crate) complete_if_new_use: bool,
    pub(crate) complete_if_new_ver: bool,
    pub(crate) complete_seed_atoms: Vec<String>,
}
