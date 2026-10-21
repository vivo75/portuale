//! After a merge: the info-dir update, the preserved-libs and news notices.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;

/// The values [`show_post_merge_notices`] reads from its caller.
pub(crate) struct ShowPostMergeNoticesArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) color: Colorizer,
    pub(crate) repos: &'a [RepoConfig],
}

/// After a merge: the info-dir update, the preserved-libs and news notices.
pub(crate) fn show_post_merge_notices(args: ShowPostMergeNoticesArgs<'_>) -> Option<ExitCode> {
    let ShowPostMergeNoticesArgs {
        opts,
        entries,
        config,
        root,
        color,
        repos,
    } = args;
    let &EmergeOpts {
        buildpkgonly,
        quiet,
        verbose,
        ..
    } = opts;
    // `elog` is no longer batched here: Portage runs it per-package inside
    // the merge, so `process_merge_elog` is called from the merge paths
    // themselves (see `elog.rs`).
    if !buildpkgonly {
        // Regenerate any stale GNU info directory index once the merge
        // changed the vdb, before the preserved-libs notice below.
        // Portage: post_emerge.py, post_emerge
        if !entries.is_empty() {
            let noinfo = config_features_list(config).iter().any(|t| t == "noinfo");
            if let Err(e) = crate::info_files::post_merge_info_update(root, &color, quiet, noinfo) {
                eprintln!("emerge: {e}");
                return Some(ExitCode::from(1));
            }
        }
        // Warn about any library the merge/unmerge just preserved, pointing
        // at `emerge @preserved-rebuild`, once the merge changed the vdb.
        // Portage: post_emerge.py, post_emerge
        crate::preserved_libs::show_preserved_libs_notice(root, &color, quiet, verbose);
        // Print the GLEP 42 count notice a second time once the merge
        // changed the vdb. A merge failure prints it too when the vdb
        // changed; see `merge_failure_news_notice` on the failure arms.
        // Portage: post_emerge.py, post_emerge
        if !entries.is_empty() {
            display_news_notice_if_any(repos, root, config, &color);
        }
    }
    None
}

/// The values [`run_autoclean_after_merge`] reads from its caller.
pub(crate) struct RunAutocleanAfterMergeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) ask: bool,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    pub(crate) color: &'a Colorizer,
}

/// After a successful non-`--pretend`/non-`--buildpkgonly` merge with a
/// user-set `AUTOCLEAN=yes`: the `--clean` selection system-wide, which
/// either reports no outdated packages or unmerges them, with no
/// `CLEAN_DELAY` countdown. The outcome is discarded: the merge already
/// succeeded, and a clean that finds nothing exits 1 by convention.
/// Portage: _emerge/actions.py, action_build; _emerge/unmerge.py, unmerge
pub(crate) fn run_autoclean_after_merge(args: RunAutocleanAfterMergeArgs<'_>) {
    let RunAutocleanAfterMergeArgs {
        root,
        config_root,
        config,
        repos,
        shell,
        debug,
        ask,
        quiet,
        noinfo,
        color,
    } = args;
    println!(">>> Auto-cleaning packages...");
    // Only for a non-`/` ROOT, like the manual removal paths do in
    // Portage (this line is an auto-clean-only addition: the manual
    // `--clean` path still does not print it).
    // Portage: _emerge/unmerge.py, _unmerge_display
    if root != Path::new("/") {
        println!(
            "\n{}",
            color.c(
                "darkgreen",
                &format!(">>> Using system located in ROOT tree {}/", root.display())
            )
        );
    }
    let _ = run_prune_nodeps_or_clean(RunPruneNodepsOrCleanArgs {
        targets: &[],
        root,
        config_root,
        pretend: false,
        ask,
        shell,
        debug,
        color,
        is_clean: true,
        autoclean: true,
        quiet,
        noinfo,
        repos,
        config,
    });
}

/// The values [`record_world_after_merge`] reads from its caller.
pub(crate) struct RecordWorldAfterMergeArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config_root: &'a Path,
    pub(crate) selected_set_args: &'a [String],
}

/// Add the merged arguments to the world and world_sets files (not for --oneshot).
pub(crate) fn record_world_after_merge(args: RecordWorldAfterMergeArgs<'_>) -> Option<ExitCode> {
    let RecordWorldAfterMergeArgs {
        opts,
        root,
        entries,
        system_atoms,
        repos,
        config_root,
        selected_set_args,
    } = args;
    let &EmergeOpts {
        buildpkgonly,
        ref atom_args,
        oneshot,
        onlydeps,
        ..
    } = opts;
    // Record each directly-requested plain target in the world file.
    // `--buildpkgonly` builds but never merges, so a package that isn't
    // installed can't be a world member yet; `--oneshot`/`--onlydeps`
    // are handled inside.
    // Portage: Scheduler._world_atom, saveNomergeFavorites
    if !buildpkgonly {
        if let Err(e) = update_world_file(UpdateWorldFileArgs {
            root,
            target_atoms: atom_args,
            entries,
            system_atoms,
            repos,
            oneshot,
            onlydeps,
        }) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(1));
        }
        // A directly-given `@name` set arg is recorded in `world_sets`
        // (same suppression set as the world file). Non-candidate sets
        // are skipped through the same `usersets_world_candidate` filter
        // as the deferral arm.
        // Portage: saveNomergeFavorites
        let record_sets: &[String] = if usersets_world_candidate(config_root) {
            selected_set_args
        } else {
            &[]
        };
        if let Err(e) = update_world_sets_file(root, record_sets, oneshot, onlydeps) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(1));
        }
    }
    None
}

/// Arguments of [`finish_failed_merge`].
pub(crate) struct FinishFailedMergeArgs<'a> {
    pub(crate) failed: FailedMerge,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) root: &'a Path,
    pub(crate) system_atoms: &'a Vec<String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
    pub(crate) config_root: &'a Path,
    pub(crate) selected_set_args: &'a [String],
    pub(crate) color: Colorizer,
}

/// After a failed merge run: what did land (a `--keep-going` run, or a failure
/// after earlier packages) is recorded in `world`, then the info index and the
/// GLEP 42 notice run when the vdb changed, like the end of a good run. A
/// failure that merged nothing is silent; one whose only landed package failed
/// its `pkg_postinst` skips the world record but still runs the info index and
/// the notice.
// Portage: Scheduler.py (world update per merged package), post_emerge.py
pub(crate) fn finish_failed_merge(args: FinishFailedMergeArgs<'_>) -> ExitCode {
    let FinishFailedMergeArgs {
        failed,
        opts,
        root,
        system_atoms,
        repos,
        config,
        config_root,
        selected_set_args,
        color,
    } = args;
    if failed.merged.is_empty() && !failed.postinst_landed {
        return failed.code;
    }
    if !failed.merged.is_empty()
        && let Some(code) = record_world_after_merge(RecordWorldAfterMergeArgs {
            opts,
            root,
            entries: &failed.merged,
            system_atoms,
            repos,
            config_root,
            selected_set_args,
        })
    {
        return code;
    }
    if !opts.buildpkgonly {
        let noinfo = config_features_list(config).iter().any(|t| t == "noinfo");
        if let Err(e) = crate::info_files::post_merge_info_update(root, &color, opts.quiet, noinfo)
        {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    merge_failure_news_notice(true, repos, root, config, &color);
    failed.code
}
