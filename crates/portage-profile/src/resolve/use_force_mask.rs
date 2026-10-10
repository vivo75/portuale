//! package.use.force/mask and their stable variants, repo and profile levels.

use crate::resolve::*;

/// package.use.force/mask and their stable variants, repo and profile levels.
pub(crate) fn fold_package_use_force_mask(
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    chain: &[PathBuf],
    config: &mut Config,
) -> Result<(), Error> {
    // package.use.mask/.force: repo-level (every repo, `::repo`-scoped like
    // package.use) plus every profile level's file -- no user-level source.
    // No masters merge at load (Portage only consults masters per-package
    // later); specificity ordering decides at application time.
    // Portage: UseManager
    let mut use_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.force"))?;
    let mut use_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_force = read_config_lines(&repo_location.join("profiles/package.use.force"))?;
        use_force_lines.extend(scope_repo_package_use_lines(&overlay_force, repo_name));
        let overlay_mask = read_config_lines(&repo_location.join("profiles/package.use.mask"))?;
        use_mask_lines.extend(scope_repo_package_use_lines(&overlay_mask, repo_name));
    }
    for level in chain {
        use_force_lines.extend(read_config_lines(&level.join("package.use.force"))?);
        use_mask_lines.extend(read_config_lines(&level.join("package.use.mask"))?);
    }
    // No shorthand here: none of these files have a user-level source, and
    // Portage's shorthand support is genuinely user-only.
    config.package_use_force = parse_package_use_lines(&use_force_lines, false);
    config.package_use_mask = parse_package_use_lines(&use_mask_lines, false);

    // use.stable.mask/.force: profile-chain only, same stacking; never folded
    // into use_flags (stability is per-candidate). Always recognized here
    // (no EAPI parametrization). Portage: eapi_supports_stable_use_forcing
    let mut use_stable_force_sources: Vec<Vec<String>> = Vec::new();
    let mut use_stable_mask_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        use_stable_force_sources.push(read_config_lines(&level.join("use.stable.force"))?);
        use_stable_mask_sources.push(read_config_lines(&level.join("use.stable.mask"))?);
    }
    config.use_stable_force = stack_mask_lines(&use_stable_force_sources)
        .into_iter()
        .collect();
    config.use_stable_mask = stack_mask_lines(&use_stable_mask_sources)
        .into_iter()
        .collect();

    // package.use.stable.mask/.force: repo-level plus profile-chain, same
    // sourcing as .mask/.force above. No shorthand, same reasoning.
    let mut use_stable_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.force"))?;
    let mut use_stable_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_stable_force =
            read_config_lines(&repo_location.join("profiles/package.use.stable.force"))?;
        use_stable_force_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_force,
            repo_name,
        ));
        let overlay_stable_mask =
            read_config_lines(&repo_location.join("profiles/package.use.stable.mask"))?;
        use_stable_mask_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_mask,
            repo_name,
        ));
    }
    for level in chain {
        use_stable_force_lines.extend(read_config_lines(&level.join("package.use.stable.force"))?);
        use_stable_mask_lines.extend(read_config_lines(&level.join("package.use.stable.mask"))?);
    }
    config.package_use_stable_force = parse_package_use_lines(&use_stable_force_lines, false);
    config.package_use_stable_mask = parse_package_use_lines(&use_stable_mask_lines, false);
    Ok(())
}
