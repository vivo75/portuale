//! Fetching and linking the distfiles a build needs.

use crate::ebuild_phases::*;

/// The config-`USE` set the `depend` phase reduces `RESTRICT`/`PROPERTIES` on:
/// Portage runs `doebuild(mydo="depend")` with the config-global `USE` (no
/// per-package layer — the phase generates IUSE). Empty when no repo or config
/// is available. Atom-matched `package.use`/`package.env` entries stay out.
/// Portage: doebuild.py, config.py
pub(crate) fn depend_use_set(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> std::collections::HashSet<String> {
    let Some(_) = repo_root_for(&env.pkg_dir) else {
        return std::collections::HashSet::new();
    };
    let repos = match portage_repo::find_repos(config_root) {
        Ok(repos) => repos,
        Err(_) => return std::collections::HashSet::new(),
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        return std::collections::HashSet::new();
    };
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .map(|config| config.use_flags)
    .unwrap_or_default()
}

/// The resolved config for the fetch path (`FETCHCOMMAND`/`RESUMECOMMAND`,
/// `PORTAGE_SSH_OPTS`): Portage reads them from the same settings the phases
/// use, so this resolves that chain once. Sibling of `depend_use_set`, which
/// needs only `use_flags`.
pub(crate) fn resolved_fetch_config(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let _ = repo_root_for(&env.pkg_dir)?;
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
}

/// Portage's `PORTAGE_RO_DISTDIRS`, filtered to directories that exist.
/// Portage: fetch.py
pub(crate) fn resolved_ro_distdirs(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Vec<std::path::PathBuf> {
    let _ = repo_root_for(&env.pkg_dir);
    let Some(repos) = portage_repo::find_repos(config_root).ok() else {
        return Vec::new();
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        return Vec::new();
    };
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
    .and_then(|config| config.other_vars.get("PORTAGE_RO_DISTDIRS").cloned())
    .map(|value| {
        portage_fetch::split_shell_words(&value)
            .into_iter()
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    })
    .unwrap_or_default()
}

/// The distfile names `src_uri` lists once each, in first-seen order, for the
/// `flag?` groups `active` keeps. The one place both `AA` and `A` are read from.
/// Portage: fetch.py, _parse_uri_map
pub(crate) fn distfile_names(
    env: &Environment,
    src_uri: &str,
    active: impl Fn(bool, &str) -> bool,
) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for entry in portage_fetch::flatten_src_uri(src_uri, active)
        .map_err(|e| format!("{}: {e}", env.pkg_dir.display()))?
    {
        if !names.contains(&entry.filename) {
            names.push(entry.filename);
        }
    }
    Ok(names)
}

/// The package's `A` without fetching: md5-cache `SRC_URI` reduced against the
/// phase `USE`. Empty outside a repo or without `SRC_URI`; feeds
/// `prepare_fake_distdir` on chains with no `unpack`.
pub(crate) fn package_a(env: &Environment, use_flags: &str) -> Result<Vec<String>, String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok(Vec::new());
    };
    let src_uri = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
        .ok()
        .and_then(|m| m.get("SRC_URI").cloned())
        .unwrap_or_default();
    let flags: std::collections::HashSet<&str> = use_flags.split_whitespace().collect();
    distfile_names(env, &src_uri, |negated, flag| {
        flags.contains(flag) != negated
    })
}
