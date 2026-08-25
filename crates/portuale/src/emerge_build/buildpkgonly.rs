//! `--buildpkgonly`: build binary packages without merging.

use crate::emerge_build::*;

/// Arguments of [`run_buildpkgonly`].
pub struct RunBuildpkgonlyArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a portage_profile::Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a PackageOptions,
    pub keep_going: bool,
    /// Scheduler-display mode (background + Jobs visibility only).
    pub mode: StatusMode,
}

/// Portage re-reads `env.d` for every package task: returns the reloaded
/// config when `<eroot>/etc/profile.env` changed since `config` was
/// resolved, `None` otherwise (the caller keeps its old path).
// Portage: Scheduler.py, _allocate_config
pub(crate) fn env_d_reloaded(config: &portage_profile::Config) -> Option<portage_profile::Config> {
    if config.envd_eroot.as_os_str().is_empty() || !config.env_d_changed(&config.envd_eroot) {
        return None;
    }
    let mut reloaded = config.clone();
    reloaded.reload_env_d(&config.envd_eroot);
    Some(reloaded)
}

/// Arguments of [`buildpkgonly_entry_build_env`].
pub(crate) struct BuildpkgonlyEntryBuildEnvArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entry: &'a GraphEntry,
    pub(crate) candidate: &'a Candidate,
    pub(crate) cpv_slot: &'a str,
    pub(crate) run_wide: &'a [(String, String)],
}
