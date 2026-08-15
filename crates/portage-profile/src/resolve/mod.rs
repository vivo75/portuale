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
/// Arguments of [`resolve_config`].
pub struct ResolveConfigArgs<'a> {
    pub config_root: &'a Path,
    pub main_repo_location: &'a Path,
    pub overlay_repos: &'a [(String, PathBuf)],
    pub repo_aliases: &'a [(String, PathBuf)],
    pub main_repo_name: &'a str,
    pub repo_masters: &'a HashMap<String, Vec<PathBuf>>,
    pub eroot: &'a Path,
}

pub fn resolve_config(args: ResolveConfigArgs<'_>) -> Result<Config, Error> {
    let ResolveConfigArgs {
        config_root,
        main_repo_location,
        overlay_repos,
        repo_aliases,
        main_repo_name,
        repo_masters,
        eroot,
    } = args;
    let mut config = Config {
        envd_eroot: eroot.to_path_buf(),
        ..Config::default()
    };
    let mut scalars: HashMap<String, String> = HashMap::new();

    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();

    // `layout.conf` `profile-formats = portage-2` gate on a profile `parent`
    // line's cross-repo syntax, read here directly so this gate and the
    // `masters` resolution in `portage_repo::find_repos` stay minimal.
    // Portage: _config/LocationsManager.py
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

    // The process environment's `USE_ORDER` layer, over profile chain + make.conf.
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

    // PKGDIR: last-level-wins scalar, `/var/cache/binpkgs` default.
    config.pkgdir = scalars
        .get("PKGDIR")
        .cloned()
        .unwrap_or_else(|| "/var/cache/binpkgs".to_string());

    // binrepos.conf + PORTAGE_BINHOST. `etc/portage/binrepos.conf` may be
    // a directory of `*.conf` fragments, not just a single file.
    config.binrepos = parse_binrepos(
        &read_config_lines(&config_root.join("etc/portage/binrepos.conf"))
            .unwrap_or_default()
            .join("\n"),
        scalars
            .get("PORTAGE_BINHOST")
            .map(String::as_str)
            .unwrap_or(""),
    );

    // `PORTAGE_CONFIGROOT` is stamped into the settings, and `CBUILD`
    // defaults to `CHOST` when unset. Both surface in `emerge --info`.
    // Portage: config.py `__init__`
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

    // The `Packages`-index `PROFILE`: the selected profile relative to the
    // main repo's `profiles/`, falling back to the absolute path when the
    // profile lives outside it.
    // Portage: dbapi/bintree.py, _update_pkgindex_header
    if let Some(selected) = chain.last() {
        let profiles_base = main_repo_location.join("profiles");
        let canon_base = profiles_base
            .canonicalize()
            .unwrap_or_else(|_| profiles_base.clone());
        config.pkgindex_profile = selected
            .strip_prefix(&canon_base)
            .map(|rel| rel.to_string_lossy().into_owned())
            .unwrap_or_else(|_| selected.to_string_lossy().into_owned());
    }

    // Freeze the immutable USE-context and visibility digests now that the
    // config is complete (the backtrack loop mutates only `autounmask_use`).
    config.use_context_base = Some(use_context_base_fingerprint(&config));
    config.is_visible_base = Some(is_visible_base_fingerprint(&config));

    Ok(config)
}
