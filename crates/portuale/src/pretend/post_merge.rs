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
