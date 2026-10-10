//! The profile's `packages` (system and @profile sets) and `package.provided`.

use crate::resolve::*;

/// The profile's `packages` (system and @profile sets) and `package.provided`.
pub(crate) fn fold_package_sets(
    chain: &[PathBuf],
    config: &mut Config,
    all_repos: &[(String, PathBuf)],
    config_root: &Path,
) -> Result<(), Error> {
    // packages (@system): every profile level's own file, in chain
    // order, stacked with the same -atom removal semantics package.mask
    // uses (see stack_mask_lines) -- confirmed by reading
    // PackagesSystemSet.load, which calls the identical real
    // stack_lists(incremental=1) function. Only *after* stacking are the
    // "*"-prefixed lines kept (with the "*" stripped) as the real
    // @system atom list -- see the module doc comment's `packages`
    // bullet for why every other stacked line is read/stacked but never
    // contributes an atom of its own.
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        packages_sources.push(read_config_lines(&level.join("packages"))?);
    }
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    // packages (@profile): the *same* files and the same
    // stack_lists(incremental=1) stacking, but (a) only profile levels
    // whose own repo declares `profile-set` in metadata/layout.conf are a
    // source at all, and (b) the *unstarred* stacked lines are the atoms
    // -- real `ProfilePackageSet.load` (`_sets/profiles.py:13-41`):
    //   for y in self._profiles if "profile-set" in y.profile_formats
    //   ... if str(x)[:1] != "*"
    // `@system` above deliberately has neither filter. A level outside
    // every configured repo (e.g. a bare synthetic profile dir in a unit
    // test, or the user-config profile node) does not qualify --
    // see `Config::profile_packages`'s own doc comment.
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

    // package.provided (real config.py:970-1027): every profile level's
    // own file (chain order) + the user-level
    // /etc/portage/profile/package.provided, stacked with the same
    // stack_lists(incremental=1) `-atom` removal package.mask/`packages`
    // use. See `Config::package_provided`.
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
