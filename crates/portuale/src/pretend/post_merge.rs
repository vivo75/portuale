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
    // `elog_process` is no longer batched here: real runs it
    // per-package inside `dblink.merge()` (before the post-merge
    // `clean`, `vartree.py:6160-6198`), and backlog #42's clean now
    // removes `${T}` -- so `ebuild_merge::process_merge_elog` is
    // called from `merge_after_install`/`merge_binpkg` themselves.
    // The `echo` summary still reaches stdout, the `save`/
    // `save_summary` files the logdir, and `mail`/`mail_summary`
    // the MTA exactly as before (see `elog.rs`).
    if !buildpkgonly {
        // Real `post_emerge()` (`post_emerge.py:126-130`): once the
        // merge changed the vdb, regenerate any stale GNU info
        // directory index *before* the preserved-libs notice below.
        // The vdb-changed gate is structural here: every merge
        // failure returned early above, `--pretend` never reaches
        // this branch, `buildpkgonly` merges nothing, and an empty
        // mergelist changed nothing either.
        if !entries.is_empty() {
            let noinfo = config_features_list(config).iter().any(|t| t == "noinfo");
            if let Err(e) = crate::info_files::post_merge_info_update(root, &color, quiet, noinfo) {
                eprintln!("emerge: {e}");
                return Some(ExitCode::from(1));
            }
        }
        // Real `post_emerge()` (`post_emerge.py:141-152`): once the
        // merge changed the vdb, warn about any library the merge/
        // unmerge just preserved and point the user at
        // `emerge @preserved-rebuild`.
        crate::preserved_libs::show_preserved_libs_notice(root, &color, quiet, verbose);
        // Backlog #196: real `post_emerge()` (`post_emerge.py:155`)
        // prints the GLEP 42 count notice a second time once the
        // merge changed the vdb (after the preserved-libs notice and
        // the config-file check, both mirrored above) -- same
        // FEATURES-plus-nonzero-count gates, no `--quiet` gate. The
        // vdb-changed gate is structural here: `--buildpkgonly`
        // builds but never merges (real's `_pkgs_changed` stays
        // false, so its `post_emerge` early-returns with no notice),
        // and an empty mergelist changed nothing either. A merge
        // failure prints the notice too when the vdb changed (real
        // prints regardless of retval, `post_emerge.py:104-108,155`)
        // -- see `merge_failure_news_notice` on the two failure
        // arms above.
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
    // Real `Scheduler._world_atom` / `saveNomergeFavorites`: record
    // each directly-requested plain target in the world file. Real's
    // own suppression set includes `--buildpkgonly` (which builds but
    // never merges, so a package that isn't installed can't be a
    // world member yet); `--oneshot`/`--onlydeps` are handled inside.
    if !buildpkgonly {
        if let Err(e) = update_world_file(
            root,
            atom_args,
            entries,
            system_atoms,
            repos,
            oneshot,
            onlydeps,
        ) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(1));
        }
        // Real `saveNomergeFavorites`'s `@set` half: a directly-given
        // `@name` set arg is recorded in `world_sets` (same
        // suppression set as the world file). Backlog #232 review
        // (Important-2): non-candidate sets are skipped
        // (`depgraph.py:11366`) -- the same `usersets_world_candidate`
        // filter as the deferral arm above.
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
