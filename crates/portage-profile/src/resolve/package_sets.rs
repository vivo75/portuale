//! The profile's `packages` (system and @profile sets) and `package.provided`.

use crate::resolve::*;

/// The profile's `packages` (system and @profile sets) and `package.provided`.
pub(crate) fn fold_package_sets(
    chain: &[PathBuf],
    config: &mut Config,
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
    warnings: &mut Vec<ProfileWarning>,
) -> Result<(), Error> {
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
    // profile node), with `eapi=None`, repo and build-id on, no wildcards
    // (a leading `*` is still stripped before parsing, kept after).
    // Portage: config.py:655-666 (the profiles_complex loop covers the
    // user node), oracle Q2e
    packages_sources.push(filter_atom_lines(
        &read_config_lines_with_sources(&config_root.join("etc/portage/profile/packages"))?,
        &user_atom_rules(false, true),
        AtomLineKind::File {
            packages_file: true,
        },
        WARN_RANK_PACKAGES,
        warnings,
    ));
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    // @profile: same files and stacking, but only levels whose repo declares
    // `profile-set` in layout.conf qualify, and only unstarred lines count.
    // Portage: ProfilePackageSet.load (_sets/profiles.py)
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
