//! The profile's `packages` (system and @profile sets) and `package.provided`.

use crate::resolve::*;

/// The profile's `packages` (system and @profile sets) and `package.provided`.
pub(crate) fn fold_package_sets(
    chain: &[PathBuf],
    config: &mut Config,
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
) -> Result<(), Error> {
    // @system: every level's `packages` stacked with `-atom` removal; only
    // `*`-prefixed lines (stripped) contribute. Portage: PackagesSystemSet
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        packages_sources.push(read_config_lines(&level.join("packages"))?);
    }
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
