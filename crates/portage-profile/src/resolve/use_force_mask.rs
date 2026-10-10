//! package.use.force/mask and their stable variants, repo and profile levels.

use crate::resolve::*;

/// package.use.force/mask and their stable variants, repo and profile levels.
pub(crate) fn fold_package_use_force_mask(
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    chain: &[PathBuf],
    config: &mut Config,
) -> Result<(), Error> {
    // package.use.mask/package.use.force: repo-level (every repo, not
    // just main -- see the package.use bullet just above for the same
    // `::repo`-scoping this now applies here too) plus every profile
    // level's own file (in chain order) -- NO
    // user-level source at all, unlike package.use: confirmed by reading
    // UseManager.__init__'s own file/variable table (the "user config"
    // section lists only "package.use -> _pusedict", nothing for
    // mask/force). Unlike package.mask/.unmask, real UseManager.py never
    // merges an overlay's own file with its master's own at load time
    // (no `stack_lists`-equivalent combination here at all -- confirmed
    // by reading `_parse_repository_files_to_dict_of_tuples`/`_of_dicts`,
    // which just parse each repo's own file independently); the masters
    // chain is only consulted later, per-package, in
    // `getUseMask`/`getUseForce` (`repos = masters + [pkg.repo]`, each
    // repo's own already-independent dict appended in that order) -- so
    // no `stack_mask_lines`-style merge is needed here either, just the
    // same scope-then-append `package.use` above already does. Flat-
    // concatenated the same way package_use already is; which entry
    // actually wins when more than one matches the same candidate is
    // decided later, at application time -- see `portage-repo`'s own doc
    // comment on why (atom-specificity ordering).
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
    // No shorthand here either: package.use.force/.mask have no
    // user-level source at all (see this crate's own doc comment), and
    // real portage's own shorthand support is genuinely user-only.
    config.package_use_force = parse_package_use_lines(&use_force_lines, false);
    config.package_use_mask = parse_package_use_lines(&use_mask_lines, false);

    // use.stable.mask/use.stable.force (PMS 5+; real
    // eapi_supports_stable_use_forcing_and_masking's own EAPI floor,
    // always recognized here -- see this crate's own module doc comment
    // for portuale's established "no EAPI parametrization"
    // precedent): profile-chain only, same -atom-removal stacking
    // use_force/use_mask already get -- deliberately NOT folded into
    // use_flags here; see use_stable_force's own doc comment for why.
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

    // package.use.stable.mask/package.use.stable.force: repo-level
    // (every repo, not just main -- same `::repo`-scoped, no-masters-
    // merge treatment package.use.mask/.force just above now gets,
    // confirmed by the same UseManager.__init__ table entries for
    // `_repo_pusestablemask_dict`/`_repo_pusestableforce_dict`) plus
    // every profile level's own file (in chain order) -- NO user-level
    // source at all, mirroring package_use_force/_mask's own confirmed
    // sourcing exactly. No shorthand either, same reasoning.
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
