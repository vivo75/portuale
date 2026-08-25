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

/// [`run_buildpkgonly`]'s per-entry build env: `run_wide` plus the
/// entry's matched `package.env` build vars with the per-package `FEATURES`
/// fold on top, then [`entry_phase_env_tail`]. A free function (not a loop
/// closure) so it is directly unit-testable without a build.
pub(crate) fn buildpkgonly_entry_build_env(
    args: BuildpkgonlyEntryBuildEnvArgs<'_>,
) -> Vec<(String, String)> {
    let BuildpkgonlyEntryBuildEnvArgs {
        config,
        repos,
        entry,
        candidate,
        cpv_slot,
        run_wide,
    } = args;
    // A package task re-reads env.d; recompute `run_wide` from the
    // reloaded config when `profile.env` changed.
    let reloaded = env_d_reloaded(config);
    let (config, run_wide) = match &reloaded {
        Some(r) => (r, run_wide_phase_env(r)),
        None => (config, run_wide.to_vec()),
    };
    let run_wide = run_wide.as_slice();
    let profile_only_variables = config
        .resolved_incremental("PROFILE_ONLY_VARIABLES")
        .unwrap_or_default();
    let mut build_env = run_wide.to_vec();
    build_env.extend(matched_package_env_vars(
        &config.package_env_vars,
        cpv_slot,
        &profile_only_variables,
        run_wide,
    ));
    let run_wide_features = run_wide
        .iter()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let calling_features = std::env::var("FEATURES").unwrap_or_default();
    if let Some(features) = resolved_features_for(
        &config.package_env_vars,
        cpv_slot,
        run_wide_features,
        &calling_features,
    ) {
        build_env.push(("FEATURES".to_string(), features.clone()));
        build_env.push(("PORTAGE_FEATURES".to_string(), features));
    }
    build_env.extend(entry_phase_env_tail(
        Some(config),
        repos,
        entry,
        Some(candidate),
    ));
    // Re-derive the xpak pipe command from the layered env, as the
    // install phases save it and `__dyn_package` reads it back through
    // `${T}/environment`.
    crate::ebuild_package::refresh_entry_compression_command(&mut build_env, Some(config));
    build_env
}
