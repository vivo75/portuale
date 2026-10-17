//! package.use.force/mask and their stable variants, repo and profile levels.

use crate::resolve::*;

/// The values [`fold_package_use_force_mask`] reads from its caller.
pub(crate) struct FoldPackageUseForceMaskArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) main_repo_name: &'a str,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) chain: &'a [PathBuf],
    pub(crate) user_node: Option<&'a UserProfileNode>,
    pub(crate) config: &'a mut Config,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// package.use.force/mask and their stable variants, repo and profile levels.
pub(crate) fn fold_package_use_force_mask(
    args: FoldPackageUseForceMaskArgs<'_>,
) -> Result<(), Error> {
    let FoldPackageUseForceMaskArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain,
        user_node,
        config,
        warnings,
    } = args;
    // package.use.mask/.force: repo-level (every repo, `::repo`-scoped like
    // package.use) plus every profile level's file plus the user profile's
    // (the node's EAPI gating the wildcard syntax).
    // No masters merge at load (Portage only consults masters per-package
    // later); specificity ordering decides at application time.
    // Portage: UseManager._parse_profile_files_to_tuple_of_dicts
    //
    // Every source validates first (use dropped, no wildcards); Portage
    // reads all of them with `allow_use=False` and the profile flags.
    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();
    let repo_rules = repo_rules_for(&all_repos, warnings);
    let dict = AtomLineKind::Dict { allow_use: false };
    let mut use_force_lines: Vec<String> = filter_atom_lines(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.use.force"))?,
        &repo_file_rules(&repo_rules, main_repo_name),
        dict,
        WARN_RANK_USE,
        warnings,
    );
    let mut use_mask_lines: Vec<String> = filter_atom_lines(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.use.mask"))?,
        &repo_file_rules(&repo_rules, main_repo_name),
        dict,
        WARN_RANK_USE,
        warnings,
    );
    for (repo_name, repo_location) in overlay_repos {
        for (file, lines) in [
            ("package.use.force", &mut use_force_lines),
            ("package.use.mask", &mut use_mask_lines),
        ] {
            lines.extend(scope_repo_package_use_lines(
                &filter_atom_lines(
                    &read_config_lines_with_sources(&repo_location.join("profiles").join(file))?,
                    &repo_file_rules(&repo_rules, repo_name),
                    dict,
                    WARN_RANK_USE,
                    warnings,
                ),
                repo_name,
            ));
        }
    }
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        use_force_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use.force"))?,
            &rules,
            dict,
            WARN_RANK_USE,
            warnings,
        ));
        use_mask_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use.mask"))?,
            &rules,
            dict,
            WARN_RANK_USE,
            warnings,
        ));
    }
    // The user profile's files stack last, with the node's own EAPI
    // gating the wildcard syntax (`extended_syntax = eapi is None and
    // user_config`). Absent without the node: without `make.profile`
    // Portage never appends it.
    // Portage: UseManager.py:277-288
    if let Some(user) = user_node {
        let user_use_rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::PackageUse);
        for (file, lines) in [
            ("package.use.force", &mut use_force_lines),
            ("package.use.mask", &mut use_mask_lines),
        ] {
            lines.extend(filter_atom_lines(
                &read_config_lines_with_sources(&user.path.join(file))?,
                &user_use_rules,
                dict,
                WARN_RANK_USE,
                warnings,
            ));
        }
    }
    // No shorthand here: none of these files have a user-level source, and
    // Portage's shorthand support is genuinely user-only.
    config.package_use_force = parse_package_use_lines(&use_force_lines, false);
    config.package_use_mask = parse_package_use_lines(&use_mask_lines, false);

    // use.stable.mask/.force: profile-chain only, same stacking; never folded
    // into use_flags (stability is per-candidate). Always recognized here
    // (no EAPI parametrization). The user profile closes the stack, like
    // every other profile list. Portage: eapi_supports_stable_use_forcing
    let mut use_stable_force_sources: Vec<Vec<String>> = Vec::new();
    let mut use_stable_mask_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        use_stable_force_sources.push(read_config_lines(&level.join("use.stable.force"))?);
        use_stable_mask_sources.push(read_config_lines(&level.join("use.stable.mask"))?);
    }
    if let Some(user) = user_node {
        use_stable_force_sources.push(read_config_lines(&user.path.join("use.stable.force"))?);
        use_stable_mask_sources.push(read_config_lines(&user.path.join("use.stable.mask"))?);
    }
    config.use_stable_force = stack_mask_lines(&use_stable_force_sources)
        .into_iter()
        .collect();
    config.use_stable_mask = stack_mask_lines(&use_stable_mask_sources)
        .into_iter()
        .collect();

    // package.use.stable.mask/.force: repo-level plus profile-chain, same
    // sourcing as .mask/.force above. No shorthand, same reasoning.
    let mut use_stable_force_lines: Vec<String> = filter_atom_lines(
        &read_config_lines_with_sources(
            &main_repo_location.join("profiles/package.use.stable.force"),
        )?,
        &repo_file_rules(&repo_rules, main_repo_name),
        dict,
        WARN_RANK_USE,
        warnings,
    );
    let mut use_stable_mask_lines: Vec<String> = filter_atom_lines(
        &read_config_lines_with_sources(
            &main_repo_location.join("profiles/package.use.stable.mask"),
        )?,
        &repo_file_rules(&repo_rules, main_repo_name),
        dict,
        WARN_RANK_USE,
        warnings,
    );
    for (repo_name, repo_location) in overlay_repos {
        for (file, lines) in [
            ("package.use.stable.force", &mut use_stable_force_lines),
            ("package.use.stable.mask", &mut use_stable_mask_lines),
        ] {
            lines.extend(scope_repo_package_use_lines(
                &filter_atom_lines(
                    &read_config_lines_with_sources(&repo_location.join("profiles").join(file))?,
                    &repo_file_rules(&repo_rules, repo_name),
                    dict,
                    WARN_RANK_USE,
                    warnings,
                ),
                repo_name,
            ));
        }
    }
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        use_stable_force_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use.stable.force"))?,
            &rules,
            dict,
            WARN_RANK_USE,
            warnings,
        ));
        use_stable_mask_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use.stable.mask"))?,
            &rules,
            dict,
            WARN_RANK_USE,
            warnings,
        ));
    }
    // Same for the `.stable` pair (same reader, same flags).
    if let Some(user) = user_node {
        let user_use_rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::PackageUse);
        for (file, lines) in [
            ("package.use.stable.force", &mut use_stable_force_lines),
            ("package.use.stable.mask", &mut use_stable_mask_lines),
        ] {
            lines.extend(filter_atom_lines(
                &read_config_lines_with_sources(&user.path.join(file))?,
                &user_use_rules,
                dict,
                WARN_RANK_USE,
                warnings,
            ));
        }
    }
    config.package_use_stable_force = parse_package_use_lines(&use_stable_force_lines, false);
    config.package_use_stable_mask = parse_package_use_lines(&use_stable_mask_lines, false);
    Ok(())
}
