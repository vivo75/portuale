//! The per-level use.mask/use.force stack the USE-flip suggestions read.

use crate::resolve::*;

/// The values [`use_mask_force_levels`] reads from its caller.
pub(crate) struct UseMaskForceLevelsArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) main_repo_name: &'a str,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) chain: &'a [PathBuf],
    pub(crate) user_node: Option<&'a UserProfileNode>,
    pub(crate) config: &'a mut Config,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// The per-level use.mask/use.force stack the USE-flip suggestions read.
pub(crate) fn use_mask_force_levels(args: UseMaskForceLevelsArgs<'_>) -> Result<(), Error> {
    let UseMaskForceLevelsArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain,
        user_node,
        config,
        warnings,
    } = args;
    // Per-level view of the eight sources parsed flat above, replaying
    // Portage's interleaved per-level stacking. Level 0 is synthetic
    // (repo-level package.use.*); levels 1.. are the profile chain, and
    // the user profile (Portage's last profile node) closes the stack.
    // Portage: UseManager.getUseMask/getUseForce
    //
    // The `package.use.*` entries validate like the flat fold (use dropped,
    // no wildcards); identical warnings dedupe at the end of `resolve_config`.
    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();
    let repo_rules = repo_rules_for(&all_repos, warnings);
    let dict = AtomLineKind::Dict { allow_use: false };
    let mut levels: Vec<UseMaskForceLevel> = Vec::new();
    {
        // Level 0: repo-level `package.use.*`, same concatenation the flat
        // fields' repo-level prefix uses.
        let repo_lines = |file: &str,
                          warnings: &mut Vec<ProfileWarning>|
         -> Result<Vec<String>, Error> {
            let mut l = filter_atom_lines(
                &read_config_lines_with_sources(&main_repo_location.join("profiles").join(file))?,
                &repo_file_rules(&repo_rules, main_repo_name),
                dict,
                WARN_RANK_USE,
                warnings,
            );
            for (repo_name, repo_location) in overlay_repos {
                l.extend(scope_repo_package_use_lines(
                    &filter_atom_lines(
                        &read_config_lines_with_sources(
                            &repo_location.join("profiles").join(file),
                        )?,
                        &repo_file_rules(&repo_rules, repo_name),
                        dict,
                        WARN_RANK_USE,
                        warnings,
                    ),
                    repo_name,
                ));
            }
            Ok(l)
        };
        levels.push(UseMaskForceLevel {
            package_use_mask: parse_package_use_lines(
                &repo_lines("package.use.mask", warnings)?,
                false,
            ),
            package_use_force: parse_package_use_lines(
                &repo_lines("package.use.force", warnings)?,
                false,
            ),
            package_use_stable_mask: parse_package_use_lines(
                &repo_lines("package.use.stable.mask", warnings)?,
                false,
            ),
            package_use_stable_force: parse_package_use_lines(
                &repo_lines("package.use.stable.force", warnings)?,
                false,
            ),
            ..Default::default()
        });
    }
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        let filtered = |file: &str,
                        warnings: &mut Vec<ProfileWarning>|
         -> Result<Vec<(String, Vec<String>)>, Error> {
            Ok(parse_package_use_lines(
                &filter_atom_lines(
                    &read_config_lines_with_sources(&level.join(file))?,
                    &rules,
                    dict,
                    WARN_RANK_USE,
                    warnings,
                ),
                false,
            ))
        };
        levels.push(UseMaskForceLevel {
            use_mask: read_config_lines(&level.join("use.mask"))?,
            use_force: read_config_lines(&level.join("use.force"))?,
            use_stable_mask: read_config_lines(&level.join("use.stable.mask"))?,
            use_stable_force: read_config_lines(&level.join("use.stable.force"))?,
            package_use_mask: filtered("package.use.mask", warnings)?,
            package_use_force: filtered("package.use.force", warnings)?,
            package_use_stable_mask: filtered("package.use.stable.mask", warnings)?,
            package_use_stable_force: filtered("package.use.stable.force", warnings)?,
        });
    }
    // The user profile closes the stack with the node's own EAPI gating
    // the `package.use*` wildcard syntax (Portage reads the profile lists
    // over `profiles_complex`, which ends at the user node).
    if let Some(user) = user_node {
        let rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::PackageUse);
        let user_filtered = |file: &str,
                             warnings: &mut Vec<ProfileWarning>|
         -> Result<Vec<(String, Vec<String>)>, Error> {
            Ok(parse_package_use_lines(
                &filter_atom_lines(
                    &read_config_lines_with_sources(&user.path.join(file))?,
                    &rules,
                    dict,
                    WARN_RANK_USE,
                    warnings,
                ),
                false,
            ))
        };
        levels.push(UseMaskForceLevel {
            use_mask: read_config_lines(&user.path.join("use.mask"))?,
            use_force: read_config_lines(&user.path.join("use.force"))?,
            use_stable_mask: read_config_lines(&user.path.join("use.stable.mask"))?,
            use_stable_force: read_config_lines(&user.path.join("use.stable.force"))?,
            package_use_mask: user_filtered("package.use.mask", warnings)?,
            package_use_force: user_filtered("package.use.force", warnings)?,
            package_use_stable_mask: user_filtered("package.use.stable.mask", warnings)?,
            package_use_stable_force: user_filtered("package.use.stable.force", warnings)?,
        });
    }
    config.use_mask_force_levels = levels;
    Ok(())
}
