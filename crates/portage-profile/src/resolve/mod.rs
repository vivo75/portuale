//! `resolve_config`: building the full `Config` from disk and the environment.

use crate::*;

mod licenses;
mod package_env;
mod package_masks;
mod package_sets;
mod package_use;
mod scalar_layers;
mod use_expand;
mod use_force_mask;
mod use_levels;
mod use_masks;
pub(crate) use licenses::*;
pub(crate) use package_env::*;
pub(crate) use package_masks::*;
pub(crate) use package_sets::*;
pub(crate) use package_use::*;
pub(crate) use scalar_layers::*;
pub(crate) use use_expand::*;
pub(crate) use use_force_mask::*;
pub(crate) use use_levels::*;
pub(crate) use use_masks::*;

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn resolve_config(
    config_root: &Path,
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    main_repo_name: &str,
    repo_masters: &HashMap<String, Vec<PathBuf>>,
    eroot: &Path,
) -> Result<Config, Error> {
    let mut config = Config {
        envd_eroot: eroot.to_path_buf(),
        ..Config::default()
    };
    let mut scalars: HashMap<String, String> = HashMap::new();

    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();

    // Real `layout.conf` `profile-formats = portage-2` gate on a profile
    // `parent` line's own `reponame:path`/`:path` cross-repo syntax
    // (real `_config/LocationsManager.py:47`/`259`): the set of repo
    // names that declare it. Read here directly (a tiny dedicated
    // layout.conf reader -- see `repo_profile_formats` -- rather than
    // threading a param through every `resolve_config` call site) so
    // `masters` resolution in `portage_repo::find_repos` and this gate
    // stay independently minimal.
    let parent_colon_repos: HashSet<String> = all_repos
        .iter()
        .filter(|(_, loc)| repo_profile_formats(loc).iter().any(|f| f == "portage-2"))
        .map(|(name, _)| name.clone())
        .collect();

    let LoadScalarLayersOut {
        chain,
        level_scalar_deltas,
        profile_scalars,
    } = load_scalar_layers(LoadScalarLayersArgs {
        eroot,
        scalars: &mut scalars,
        config: &mut config,
        config_root,
        all_repos: &all_repos,
        repo_aliases,
        parent_colon_repos: &parent_colon_repos,
    })?;

    // Real `config.regenerate()`'s `env` `USE_ORDER` layer -- the process
    // environment overrides / stacks on the profile chain + `make.conf`.
    // See `apply_env_layer` / `ENV_*_VARS`.
    apply_env_layer(&mut scalars, &mut config);

    expand_use_vars(
        &mut config,
        &mut scalars,
        &profile_scalars,
        &level_scalar_deltas,
    );

    fold_profile_use_masks(&chain, &mut config)?;

    fold_package_masks_and_keywords(FoldPackageMasksAndKeywordsArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        repo_masters,
        chain: &chain,
        config_root,
        config: &mut config,
    })?;

    fold_package_use(FoldPackageUseArgs {
        main_repo_location,
        overlay_repos,
        chain: &chain,
        config_root,
        config: &mut config,
        scalars: &scalars,
        eroot,
    })?;

    fold_package_env(config_root, &mut config, &scalars)?;

    fold_package_use_force_mask(main_repo_location, overlay_repos, &chain, &mut config)?;

    use_mask_force_levels(main_repo_location, overlay_repos, &chain, &mut config)?;

    fold_package_sets(&chain, &mut config, &all_repos, config_root)?;

    fold_licenses_and_accepts(&all_repos, config_root, &mut config, &scalars)?;

    // PKGDIR: last-level-wins scalar, real "/var/cache/binpkgs" default
    // (see `pkgdir`'s own doc comment).
    config.pkgdir = scalars
        .get("PKGDIR")
        .cloned()
        .unwrap_or_else(|| "/var/cache/binpkgs".to_string());

    // binrepos.conf + PORTAGE_BINHOST (see `binrepos`'s own doc comment).
    // Real `BinRepoConfigLoader._parse` runs `_recursive_file_list` over
    // the path, so `etc/portage/binrepos.conf` may be a **directory** of
    // `*.conf` fragments (the standard portage "file or dir" pattern, and
    // how the release profile ships it -- `gentoobinhost.conf`), not just
    // a single file.
    config.binrepos = parse_binrepos(
        &read_config_lines(&config_root.join("etc/portage/binrepos.conf"))
            .unwrap_or_default()
            .join("\n"),
        scalars
            .get("PORTAGE_BINHOST")
            .map(String::as_str)
            .unwrap_or(""),
    );

    // Real `config.py.__init__`: `PORTAGE_CONFIGROOT` is stamped into the
    // settings, and `CBUILD` defaults to `CHOST` when unset anywhere
    // (`config.py`'s own `if self.get("CBUILD") is None and
    // self.get("CHOST")`). Both surface in `emerge --info`.
    scalars
        .entry("PORTAGE_CONFIGROOT".to_string())
        .or_insert_with(|| {
            let s = config_root.to_string_lossy();
            if s.is_empty() {
                "/".to_string()
            } else {
                s.into_owned()
            }
        });
    if !scalars.contains_key("CBUILD")
        && let Some(chost) = scalars.get("CHOST").cloned()
    {
        scalars.insert("CBUILD".to_string(), chost);
    }

    config.other_vars = scalars;

    // #105: freeze the immutable USE-context digest now that the config
    // is complete. The `'backtrack` loop mutates only `autounmask_use`
    // (on clones), which the memo keys hash against this base.
    config.use_context_base = Some(use_context_base_fingerprint(&config));
    // #114: and the visibility base (masks + accept lists).
    config.is_visible_base = Some(is_visible_base_fingerprint(&config));

    Ok(config)
}
