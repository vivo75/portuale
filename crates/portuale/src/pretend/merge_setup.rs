//! The options every merge path shares: status mode, package and merge options, PORTAGE_TMPDIR.

use crate::ebuild_merge::MergeOptions;
use crate::ebuild_package::PackageOptions;
use crate::emerge_build::StatusMode;
use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// What [`merge_setup`] hands back to its caller.
pub(crate) struct MergeSetupOut {
    pub(crate) status_mode: StatusMode,
    /// `--jobs` after the interactive-package fallback.
    pub(crate) jobs: usize,
    pub(crate) package_options: PackageOptions,
    pub(crate) portage_tmpdir: PathBuf,
    pub(crate) merge_options: MergeOptions,
}

/// The options every merge path shares: status mode, package and merge options, PORTAGE_TMPDIR.
/// Also rotates the resume list to its backup, as Portage does before a merge.
pub(crate) fn merge_setup(
    opts: &EmergeOpts<'_>,
    entries: &[GraphEntry],
    config: &Config,
    root: &Path,
) -> MergeSetupOut {
    let &EmergeOpts {
        jobs,
        quiet,
        quiet_build,
        verbose,
        shell,
        debug,
        buildpkg_opt,
        merge_wait_scope,
        ..
    } = opts;
    // The status-line blank rule lives in the merge functions' `StatusDisplay`;
    // only the background predicate computes here.
    let status_mode = crate::emerge_build::scheduler_status_mode(
        jobs,
        quiet,
        quiet_build,
        entries.len(),
        verbose,
    );
    let (status_mode, jobs) = crate::emerge_build::apply_interactive_tasks(
        status_mode,
        jobs,
        entries,
        root,
        &crate::emerge_build::progress_color(),
    );
    // Compressor/package-dir resolution; see `package_options_from_env`.
    let mut package_options = package_options_from_env(
        shell,
        debug,
        Some(&config_features_string(config)),
        Some(config),
    );
    // The transient `--buildpkg`/`--buildpkg=n` flag joins/leaves the run's
    // `FEATURES` (and so the `Packages` header), like Portage's post-resolve
    // `settings.features` mutation.
    // Portage: _emerge/actions.py, action_build
    if let Some(header) = package_options.pkgindex_header.as_mut() {
        portage_profile::pkgindex_header_apply_buildpkg_opt(header, buildpkg_opt);
    }
    let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(config));
    let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
    // The compiler/make flags for every build phase's environment, from the
    // resolved config. `FEATURES` stays out (portuale models it separately).
    merge_options.build_env = crate::emerge_build::run_wide_phase_env(config);
    // Portage sets `PORTAGE_QUIET` only below the `--quiet` noise floor
    // (`doebuild.py`, `noiselimit < 0`); the phase env otherwise leaves it
    // unset so the `__vecho` markers print. Threaded here so every merge
    // phase inherits it through the build env's last-wins layering.
    if quiet {
        merge_options
            .build_env
            .push(("PORTAGE_QUIET".to_string(), "1".to_string()));
    }
    merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
    // `--merge-wait-scope` rides into the scheduler through the merge
    // options; unset means Portage's `deep` default.
    merge_options.merge_wait_scope = merge_wait_scope.unwrap_or_default();
    // `INSTALL_MASK` + the `no{man,info,doc}` `FEATURES` fold, from the
    // resolved config, not the process env.
    // Portage: bin/misc-functions.sh, preinst_mask
    (
        merge_options.install_mask,
        merge_options.install_mask_prunes_usr_share,
    ) = config_install_mask(config);
    // `UNINSTALL_IGNORE` from the resolved scalar, for the replace loop
    // (before `set_resolved_features`, which folds packdebug over it).
    merge_options.apply_uninstall_ignore(config);
    merge_options.set_resolved_features(&config_features_string(config));
    // `CONFIG_PROTECT` from the resolved fold (calling env included), not the
    // raw process env. See `MergeOptions::apply_config_protect`.
    merge_options.apply_config_protect(config);
    // A `package.env` entry matching a build-bound package layers its env
    // file's build vars over the run-wide set above.
    // Portage: _emerge/actions.py, _grab_pkg_env
    merge_options.package_env_vars = config.package_env_vars.clone();
    // Before a fresh, non-`--resume` merge starts, an existing multi-package
    // resume list from an earlier abandoned run is preserved as `resume_backup`.
    // Portage: _emerge/actions.py, action_build
    crate::mtimedb::rotate_resume_to_backup(root);
    MergeSetupOut {
        status_mode,
        jobs,
        package_options,
        portage_tmpdir,
        merge_options,
    }
}

/// The values [`show_nothing_to_merge`] reads from its caller.
pub(crate) struct ShowNothingToMergeArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) show_merge_list: bool,
    pub(crate) mergecount: usize,
    pub(crate) deferred: bool,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) selected_set_args: &'a [String],
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) color: Colorizer,
}

/// An empty plan: Portage's "Nothing to merge" notices and exit.
pub(crate) fn show_nothing_to_merge(args: ShowNothingToMergeArgs<'_>) -> Option<ExitCode> {
    let ShowNothingToMergeArgs {
        opts,
        show_merge_list,
        mergecount,
        deferred,
        root,
        config_root,
        selected_set_args,
        entries,
        system_atoms,
        repos,
        color,
    } = args;
    let &EmergeOpts {
        pretend,
        buildpkgonly,
        ref atom_args,
        ask,
        oneshot,
        onlydeps,
        ..
    } = opts;
    if nothing_to_merge(pretend, show_merge_list, mergecount) {
        if deferred {
            // The blank line still prints then the run falls through to the
            // rotation and the world-favorites record; only `Scheduler` is skipped.
            // Portage: _emerge/actions.py, action_build
            println!();
            crate::mtimedb::rotate_resume_to_backup(root);
            // Only `--buildpkgonly` still suppresses the record here
            // (`--oneshot`/`--onlydeps` already keep the deferral gate at bay;
            // `--fetchonly`/`--fetch-all-uri` are unimplemented). Under `--ask`
            // the additions list prints first with a keep-or-drop prompt.
            // Portage: depgraph.py, saveNomergeFavorites
            if !buildpkgonly {
                // Only gate-passing sets record (non-candidate sets skip).
                let record_sets: &[String] = if usersets_world_candidate(config_root) {
                    selected_set_args
                } else {
                    &[]
                };
                let fav_refs: Vec<&str> = atom_args.to_vec();
                let mut additions = match read_world_atoms(root) {
                    Ok(current) => {
                        world_file_additions(&current, &fav_refs, entries, system_atoms, repos)
                    }
                    Err(e) => {
                        eprintln!("emerge: {e}");
                        return Some(ExitCode::from(1));
                    }
                };
                match read_world_sets(root) {
                    Ok(names) => {
                        let current: Vec<String> =
                            names.into_iter().map(|n| format!("@{n}")).collect();
                        additions.extend(world_sets_additions(&current, record_sets));
                    }
                    Err(e) => {
                        eprintln!("emerge: {e}");
                        return Some(ExitCode::from(1));
                    }
                }
                if ask && !additions.is_empty() {
                    println!();
                    let mut shown = additions.clone();
                    shown.sort();
                    shown.dedup();
                    for a in &shown {
                        println!(" {} {}", color.c("GOOD", "*"), a);
                    }
                    println!();
                    if ask_yes_no(
                        &color,
                        "Would you like to add these packages to your world favorites?",
                    ) != Some(true)
                    {
                        return Some(ExitCode::SUCCESS);
                    }
                }
                // The writers re-read the world files and recompute the same
                // additions: identical (single-threaded, no intervening write)
                // and keeps one writer entry point for both arms.
                if let Err(e) = update_world_file(UpdateWorldFileArgs {
                    root,
                    target_atoms: &fav_refs,
                    entries,
                    system_atoms,
                    repos,
                    oneshot,
                    onlydeps,
                }) {
                    eprintln!("emerge: {e}");
                    return Some(ExitCode::from(1));
                }
                if let Err(e) = update_world_sets_file(root, record_sets, oneshot, onlydeps) {
                    eprintln!("emerge: {e}");
                    return Some(ExitCode::from(1));
                }
            }
            return Some(ExitCode::SUCCESS);
        }
        // `--ask` reaches here with no blank line printed yet.
        if ask {
            println!();
        }
        println!("Nothing to merge; quitting.");
        println!();
        return Some(ExitCode::SUCCESS);
    }
    None
}
