//! The per-level use.mask/use.force stack the USE-flip suggestions read.

use crate::resolve::*;

/// The per-level use.mask/use.force stack the USE-flip suggestions read.
pub(crate) fn use_mask_force_levels(
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    chain: &[PathBuf],
    config: &mut Config,
) -> Result<(), Error> {
    // Per-level view of the eight sources parsed flat above, replaying
    // Portage's interleaved per-level stacking. Level 0 is synthetic
    // (repo-level package.use.*); levels 1.. are the profile chain.
    // Portage: UseManager.getUseMask/getUseForce
    let mut levels: Vec<UseMaskForceLevel> = Vec::new();
    {
        // Level 0: repo-level `package.use.*`, same concatenation the flat
        // fields' repo-level prefix uses.
        let repo_pkg = |file: &str| -> Result<Vec<(String, Vec<String>)>, Error> {
            let mut l = read_config_lines(&main_repo_location.join("profiles").join(file))?;
            for (repo_name, repo_location) in overlay_repos {
                l.extend(scope_repo_package_use_lines(
                    &read_config_lines(&repo_location.join("profiles").join(file))?,
                    repo_name,
                ));
            }
            Ok(parse_package_use_lines(&l, false))
        };
        levels.push(UseMaskForceLevel {
            package_use_mask: repo_pkg("package.use.mask")?,
            package_use_force: repo_pkg("package.use.force")?,
            package_use_stable_mask: repo_pkg("package.use.stable.mask")?,
            package_use_stable_force: repo_pkg("package.use.stable.force")?,
            ..Default::default()
        });
    }
    for level in chain {
        levels.push(UseMaskForceLevel {
            use_mask: read_config_lines(&level.join("use.mask"))?,
            use_force: read_config_lines(&level.join("use.force"))?,
            use_stable_mask: read_config_lines(&level.join("use.stable.mask"))?,
            use_stable_force: read_config_lines(&level.join("use.stable.force"))?,
            package_use_mask: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.mask"))?,
                false,
            ),
            package_use_force: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.force"))?,
                false,
            ),
            package_use_stable_mask: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.stable.mask"))?,
                false,
            ),
            package_use_stable_force: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.stable.force"))?,
                false,
            ),
        });
    }
    config.use_mask_force_levels = levels;
    Ok(())
}
