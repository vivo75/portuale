//! Config and repo loading, EMERGE_DEFAULT_OPTS, scheduling policy.

use crate::pretend::*;

/// `INSTALL_MASK` folded with the `no{man,info,doc}` tokens; non-empty process env
/// wins, else the resolved config. Empty-blanking-the-config is an unhandled corner.
/// Portage: bin/misc-functions.sh, preinst_mask
///
/// Explicit `--buildpkg[=n]` wins, else resolved `FEATURES` decides; split out for
/// testability. Portage: create_depgraph_params.py
pub(crate) fn buildpkg_from_config(
    buildpkg_opt: Option<bool>,
    config: &portage_profile::Config,
) -> bool {
    buildpkg_opt.unwrap_or_else(|| config_features_list(config).iter().any(|t| t == "buildpkg"))
}

/// Resolved `FEATURES` if assigned anywhere: the folded stack, else a raw split of
/// `other_vars["FEATURES"]`. `None` means nothing configured anywhere.
pub(crate) fn config_features_opt(config: &portage_profile::Config) -> Option<Vec<String>> {
    config.resolved_incremental("FEATURES").or_else(|| {
        config
            .other_vars
            .get("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
    })
}

pub(crate) fn config_features_list(config: &portage_profile::Config) -> Vec<String> {
    config_features_opt(config).unwrap_or_default()
}

/// Resolved merge-time `PORTAGE_BZIP2_COMMAND`; shared with the remote server.
pub(crate) fn config_bzip2_command(config: &portage_profile::Config) -> String {
    config
        .other_vars
        .get("PORTAGE_BZIP2_COMMAND")
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bzip2".to_string())
}

/// Profile half over a loaded repo list: both roots resolve the same trees.
/// Portage: create_trees (running tree inherits `PORTAGE_REPOSITORIES`)
pub(crate) fn resolve_config_for_root(
    config_root: &std::path::Path,
    repos: &[portage_repo::RepoConfig],
    root: &std::path::Path,
) -> Result<portage_profile::Config, String> {
    let main_repo = repos
        .iter()
        .find(|r| r.is_main)
        .ok_or_else(|| "no main repo found in repos.conf".to_string())?;

    // Each overlay's (name, location): for `::name` scoping and cross-repo parents.
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();

    // Each repo's resolved `masters` chain, keyed by name.
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();

    // Every repo's `aliases` with locations, for aliased profile parents.
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();

    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot: root,
    })
    .map_err(|e| e.to_string())?;
    Ok(config)
}
