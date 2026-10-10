//! Fetching and linking the distfiles a build needs.

use crate::ebuild_phases::*;

/// The config-`USE` set the `depend` phase reduces `RESTRICT`/
/// `PROPERTIES` on: real `doebuild(mydo="depend")` runs with the
/// `setcpv` config `USE` (profile + `make.conf` + user `package.use`,
/// no per-package IUSE resolution -- the phase *generates* IUSE), and
/// `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` come from that same
/// already-USE-reduced `_pkg` accessor (`_flatten` over
/// `settings["PORTAGE_USE"]`). Portuale's equivalent is
/// `Config::use_flags` (the same config-global set, no per-candidate
/// layer). Empty when no repo checkout or config load is available --
/// the pre-existing empty-set behavior, not an error. Atom-matched
/// `package.use` / `package.env` entries for the depend target are NOT
/// folded in (matching needs the target's `cat/pkg-ver:slot/sub`
/// identity, which the `depend` path deliberately doesn't resolve --
/// metadata extraction stays on the config-global set).
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

/// The resolved config for the distfile fetch path: real `fetch.py` reads
/// `FETCHCOMMAND`/`RESUMECOMMAND` (and the `_<PROTO>` variants) and
/// `PORTAGE_SSH_OPTS` out of the same `mysettings` the phases run with,
/// so portuale resolves the same chain once here (the `depend_use_set`
/// resolution above is kept as a sibling rather than factored, since
/// this needs the whole `Config`, not just `use_flags`).
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

/// Real `fetch.py:1055-1059`: `shlex.split(PORTAGE_RO_DISTDIRS)` filtered
/// to directories that exist (each candidate is re-checked at fetch
/// time, but real filters the list up front).
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

/// The distfile names `src_uri` lists once each, in first-seen order, for
/// the `flag?` groups `active` keeps (real `_parse_uri_map`'s keys). The
/// one place both `AA` (every group) and `A` (the phase `USE`) are read
/// from.
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

/// The package's own `A` without fetching anything: its md5-cache
/// `SRC_URI` reduced against the phase `USE` (real `A` is the
/// `use_reduce(SRC_URI, uselist=USE)` distfile list, the same set
/// `fetch_src_uri` returns). Empty for an ebuild outside any repo or with
/// no `SRC_URI`. Feeds `prepare_fake_distdir` on chains with no `unpack`.
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
/// Arguments of [`fetch_sources()`].
pub(crate) struct FetchSourcesArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) distdir: &'a Path,
    pub(crate) debug: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ShellBackend,
    pub(crate) features: &'a str,
    pub(crate) use_flags: &'a str,
}

pub(crate) async fn fetch_sources(
    args: FetchSourcesArgs<'_>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let FetchSourcesArgs {
        env,
        root,
        distdir,
        debug,
        config_root,
        shell,
        features,
        use_flags,
    } = args;
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let metadata = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok();
    let src_uri = metadata
        .as_ref()
        .and_then(|m| m.get("SRC_URI").cloned())
        .unwrap_or_default();
    if src_uri.trim().is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let restrict = metadata.as_ref().and_then(|m| m.get("RESTRICT"));
    let restrict_mirror = restrict
        .map(|r| restrict_mirror_from_restrict(r))
        .unwrap_or(false);
    let restrict_fetch = restrict
        .map(|r| restrict_fetch_from_restrict(r))
        .unwrap_or(false);
    let restrict_primaryuri = restrict
        .map(|r| restrict_primaryuri_from_restrict(r))
        .unwrap_or(false);
    // Real `AA`: the keys of `_parse_uri_map` -- each distfile once.
    let aa = distfile_names(env, &src_uri, |_, _| true)?;
    // Real `fetch.py` reads its commands and `PORTAGE_SSH_OPTS` out of
    // the same settings object the phases use; resolve the chain once.
    let fetch_config = resolved_fetch_config(env, config_root, root);
    let a = fetch::fetch_src_uri(
        &env.pkg_dir,
        &src_uri,
        &FetchOptions {
            distdir: distdir.to_path_buf(),
            gentoo_mirrors: fetch::gentoo_mirrors_from_env(),
            // Real PORTAGE_CONFIGROOT (real default: "/" when unset) --
            // consulted only for real `custommirrors`, see
            // `FetchOptions::config_root`'s own doc comment.
            config_root: portage_repo::config_root_from_env(),
            // Real `"distlocks" in self.settings.features` -- same
            // env-var-not-full-config-resolution shortcut
            // `collision_protect`/`protect_owned`/`unmerge_orphans`
            // already use, defaulting to real `true` (see
            // `FetchOptions::distlocks`'s own doc comment).
            distlocks: features.split_whitespace().any(|tok| tok == "distlocks"),
            restrict_mirror,
            restrict_fetch,
            restrict_primaryuri,
            // Real `FEATURES=force-mirror` -- same env-var shortcut as
            // `distlocks` above (no full config resolution on this
            // path), defaulting to real `false`.
            force_mirror: features.split_whitespace().any(|tok| tok == "force-mirror"),
            use_flags: use_flags.split_whitespace().map(String::from).collect(),
            // Real `fetch.py:1652-1700`'s command family out of the same
            // settings object the phases use.
            fetch_commands: fetch_config.as_ref().map(fetch::fetch_commands_from_config),
            // Real `mysettings.get("PORTAGE_SSH_OPTS")` (`fetch.py:1806`).
            portage_ssh_opts: fetch_config
                .as_ref()
                .and_then(fetch::portage_ssh_opts_from_config),
            // Real `fetch.py:1055-1059`: existing directories only.
            ro_distdirs: resolved_ro_distdirs(env, config_root, root),
            mirror_cache_now: None,
            // Real `PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS` -- same env-var
            // shortcut as `distlocks` above.
            checksum_failure_max_tries: {
                let value = std::env::var("PORTAGE_FETCH_CHECKSUM_TRY_MIRRORS").ok();
                let (tries, warnings) = fetch::checksum_failure_max_tries(value.as_deref());
                for warning in warnings {
                    eprintln!("{warning}");
                }
                tries
            },
        },
    );
    let a = match a {
        Ok(a) => a,
        Err(e) => {
            // Real `fetch.py`: when a distfile can't be fetched, run the
            // ebuild's own `pkg_nofetch` phase -- it prints custom "get it
            // from <URL> and drop it in <DISTDIR>" instructions -- then
            // fail. Best-effort: the phase not being defined (or its own
            // failure) never masks the real fetch error.
            let ebuild = env.pkg_dir.join(format!("{}.ebuild", env.split.pf));
            if ebuild.is_file() {
                let _ = run_one_phase(RunOnePhaseArgs {
                    env,
                    root,
                    phase: "nofetch",
                    debug,
                    extra_env: &[],
                    config_root,
                    shell,
                    log_file: None,
                })
                .await;
            }
            return Err(e);
        }
    };
    Ok((a, aa))
}
