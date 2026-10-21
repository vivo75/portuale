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
pub use package_masks::masking_file_comment;
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

    // Profile atom warnings, collected per file above: Portage order is by
    // manager (`packages`, `package.use`, `package.license` /
    // `package.properties` / `package.accept_restrict` / `package.env`,
    // `package.accept_keywords`, `package.mask`), not by this fold order,
    // so stable-sort by rank; files read twice (chain `package.use`) warn
    // twice identically, and Portage prints each warning once (owner Q3),
    // so dedupe exactly.
    //
    // Within the `package.use` rank the user file's warnings are appended
    // after the force fold (see above), so profile `package.use*` files
    // precede it the way Portage's UseManager reads them.
    let mut warnings: Vec<ProfileWarning> = Vec::new();
    let mut user_use_warnings: Vec<ProfileWarning> = Vec::new();
    let LoadScalarLayersOut {
        chain,
        user_node,
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
        warnings: &mut warnings,
    })?;

    // The process environment's `USE_ORDER` layer, over profile chain + make.conf.
    apply_env_layer(&mut scalars, &mut config);

    expand_use_vars(
        &mut config,
        &mut scalars,
        &profile_scalars,
        &level_scalar_deltas,
    );

    fold_profile_use_masks(&chain, user_node.as_ref(), &mut config)?;

    fold_package_masks_and_keywords(FoldPackageMasksAndKeywordsArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        repo_masters,
        chain: &chain,
        user_node: user_node.as_ref(),
        config_root,
        config: &mut config,
        warnings: &mut warnings,
    })?;

    fold_package_use(FoldPackageUseArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain: &chain,
        config_root,
        config: &mut config,
        scalars: &scalars,
        eroot,
        warnings: &mut warnings,
        user_warnings: &mut user_use_warnings,
    })?;

    fold_package_use_force_mask(FoldPackageUseForceMaskArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain: &chain,
        user_node: user_node.as_ref(),
        config: &mut config,
        warnings: &mut warnings,
    })?;
    // The user `package.use` warnings sort after every profile `package.use*`
    // file: Portage's UseManager parses the profile files before the user
    // ones (S0 cell C-userconfig: `profile/package.use.force` warnings
    // precede the user `package.use` ones), and the rank sort below keeps
    // fold order within a rank. The folds are independent.
    warnings.append(&mut user_use_warnings);

    use_mask_force_levels(UseMaskForceLevelsArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain: &chain,
        user_node: user_node.as_ref(),
        config: &mut config,
        warnings: &mut warnings,
    })?;

    fold_package_sets(FoldPackageSetsArgs {
        chain: &chain,
        user_node: user_node.as_ref(),
        config: &mut config,
        all_repos: &all_repos,
        config_root,
        warnings: &mut warnings,
    })?;

    fold_licenses_and_accepts(
        &all_repos,
        config_root,
        &mut config,
        &scalars,
        &mut warnings,
    )?;

    // `package.env` folds after the license files so its warnings sort
    // after theirs: Portage reads `package.license` (LicenseManager),
    // then `package.properties` / `package.accept_restrict` /
    // `package.env` inline (config.py:855-902). The two folds are
    // independent (both read only `config` + `scalars`).
    fold_package_env(config_root, &mut config, &scalars, &mut warnings)?;

    // PKGDIR: last-level-wins scalar, `/var/cache/binpkgs` default.
    config.pkgdir = scalars
        .get("PKGDIR")
        .cloned()
        .unwrap_or_else(|| "/var/cache/binpkgs".to_string());

    // binrepos.conf + PORTAGE_BINHOST. `etc/portage/binrepos.conf` may be
    // a directory of `*.conf` fragments, not just a single file. The
    // binrepo key warnings join the profile warning stream (same
    // config-load position on stderr).
    config.binrepos = parse_binrepos(
        &read_config_lines(&config_root.join("etc/portage/binrepos.conf"))
            .unwrap_or_default()
            .join("\n"),
        scalars
            .get("PORTAGE_BINHOST")
            .map(String::as_str)
            .unwrap_or(""),
        &mut warnings,
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

    // Profile atom warnings, Portage-ordered and printed once (owner Q3):
    // stable rank sort keeps file-read order within a rank; identical
    // (rank, text) pairs (files read twice, e.g. chain `package.use`)
    // collapse to the first occurrence.
    let mut seen = std::collections::HashSet::new();
    warnings.retain(|w| seen.insert((w.rank, w.text.clone())));
    warnings.sort_by_key(|w| w.rank);
    config.config_warnings = warnings.into_iter().map(|w| w.text).collect();

    Ok(config)
}
