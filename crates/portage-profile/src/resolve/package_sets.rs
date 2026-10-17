//! The profile's `packages` (system and @profile sets) and `package.provided`.

use crate::resolve::*;

/// The profile's `packages` (system and @profile sets) and `package.provided`.
pub(crate) struct FoldPackageSetsArgs<'a> {
    pub(crate) chain: &'a [PathBuf],
    pub(crate) user_node: Option<&'a UserProfileNode>,
    pub(crate) config: &'a mut Config,
    pub(crate) all_repos: &'a [(String, PathBuf)],
    pub(crate) config_root: &'a Path,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// The profile's `packages` (system and @profile sets) and `package.provided`.
pub(crate) fn fold_package_sets(args: FoldPackageSetsArgs<'_>) -> Result<(), Error> {
    let FoldPackageSetsArgs {
        chain,
        user_node,
        config,
        all_repos,
        config_root,
        warnings,
    } = args;
    // @system: every level's `packages` stacked with `-atom` removal; only
    // `*`-prefixed lines (stripped) contribute. Portage: PackagesSystemSet
    //
    // Each node's file validates first with its own rules (a leading `*`
    // is stripped before parsing, kept after); `package.provided` below is
    // a CPV list Portage validates differently (S8 family): untouched here.
    let repo_rules = repo_rules_for(all_repos, warnings);
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        let rules = rules_for_node(level, all_repos, &repo_rules)?;
        packages_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("packages"))?,
            &rules,
            AtomLineKind::File {
                packages_file: true,
            },
            WARN_RANK_PACKAGES,
            warnings,
        ));
    }
    // The user profile's `packages` stacks last (it is Portage's last
    // profile node), with the node's own EAPI, repo and build-id on, no
    // wildcards (a leading `*` is still stripped before parsing, kept
    // after). Absent without the node.
    // Portage: config.py:655-666 (the profiles_complex loop covers the
    // user node)
    if let Some(user) = user_node {
        let rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::Mask);
        packages_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&user.path.join("packages"))?,
            &rules,
            AtomLineKind::File {
                packages_file: true,
            },
            WARN_RANK_PACKAGES,
            warnings,
        ));
    }
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    // @profile: same files and stacking, but only levels whose repo declares
    // `profile-set` in layout.conf qualify, and only unstarred lines count.
    // The user node always qualifies: Portage hard-codes `profile-set`
    // in its formats tuple. Portage: ProfilePackageSet.load (_sets/profiles.py)
    let mut profile_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        let qualifies = repo_containing(level, all_repos)
            .map(|(_, location)| {
                repo_profile_formats(&location)
                    .iter()
                    .any(|format| format == "profile-set")
            })
            .unwrap_or(false);
        if qualifies {
            profile_sources.push(read_config_lines(&level.join("packages"))?);
        }
    }
    if let Some(user) = user_node {
        let rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::Mask);
        profile_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&user.path.join("packages"))?,
            &rules,
            AtomLineKind::File {
                packages_file: true,
            },
            WARN_RANK_PACKAGES,
            warnings,
        ));
    }
    config.profile_packages = stack_mask_lines(&profile_sources)
        .into_iter()
        .filter(|line| !line.starts_with('*'))
        .collect();

    // package.provided: every level's file plus the user-level one, same
    // `-atom` stacking. Portage: config.py package.provided
    let mut pprovided_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        pprovided_sources.push(read_config_lines(&level.join("package.provided"))?);
    }
    pprovided_sources.push(read_config_lines(
        &config_root.join("etc/portage/profile/package.provided"),
    )?);
    config.package_provided = stack_mask_lines(&pprovided_sources);
    Ok(())
}
