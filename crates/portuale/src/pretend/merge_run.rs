//! Build and merge the plan from source, after saving it as the resume list.

use crate::ebuild_merge::MergeOptions;
use crate::ebuild_package::PackageOptions;
use crate::emerge_build::StatusMode;
use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;

/// The values [`run_source_merge_plan`] reads from its caller.
pub(crate) struct RunSourceMergePlanArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) buildpkg: Option<&'a PackageOptions>,
    pub(crate) status_mode: StatusMode,
    pub(crate) config: &'a Config,
    pub(crate) color: Colorizer,
}

/// Build and merge the plan from source, after saving it as the resume list.
pub(crate) fn run_source_merge_plan(args: RunSourceMergePlanArgs<'_>) -> Option<ExitCode> {
    let RunSourceMergePlanArgs {
        opts,
        entries,
        root,
        repos,
        portage_tmpdir,
        merge_options,
        buildpkg,
        status_mode,
        config,
        color,
    } = args;
    let &EmergeOpts {
        oneshot,
        onlydeps,
        ref atom_args,
        keep_going,
        ref buildpkg_exclude,
        jobs,
        load_average,
        quiet_build,
        quiet,
        ..
    } = opts;
    // Plain `emerge <atom>`: real source build + merge (see
    // `emerge_build::run_source_merge`). The up-front resume save
    // is real `Scheduler.merge() -> _save_resume_list()` (#168),
    // same as the `--getbinpkg` arm above -- unconditional (an
    // all-noop plan records the empty list, #179).
    let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
    let _ = crate::mtimedb::write_resume_list(root, atom_args, &resume_mergelist, &resume_opts);
    if let Err(e) = emerge_build::run_source_merge(
        entries,
        repos,
        root,
        portage_tmpdir,
        merge_options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        jobs,
        load_average,
        // Real `Scheduler._background_mode`: capture per-package
        // build output when `--quiet-build=y` or `-q` is set (a
        // `-j` >1 run always captures regardless).
        quiet_build.unwrap_or(false) || quiet,
        status_mode,
    ) {
        // Real `Scheduler._save_resume_list`: on a merge failure,
        // record every still-unmerged package so `emerge --resume`
        // can pick up where this left off. The empty-`unmerged`
        // guard is the failure path's own (`write_resume_list`
        // itself is unconditional now, #179): a failure with
        // nothing unmerged leaves the per-merge shrink's own
        // tail alone.
        let unmerged = entries_not_merged(root, entries);
        let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
        if !unmerged.is_empty() {
            if let Err(w) =
                crate::mtimedb::write_resume_list(root, atom_args, &unmerged, &resume_opts)
            {
                eprintln!("emerge: warning: could not save the resume list: {w}");
            } else {
                eprintln!(
                    "\n * The resume list contains packages that could not be \
                             merged.\n * Use `emerge --resume` to retry, or `emerge --resume \
                             --skipfirst` to skip the first one."
                );
            }
        }
        // Backlog #196 fix round 1: same failure-path notice as
        // the `--getbinpkg` arm above (real `post_emerge.py:155`
        // regardless of retval, gated on the vdb having changed).
        merge_failure_news_notice(unmerged.len() < entries.len(), repos, root, config, &color);
        eprintln!("emerge: {e}");
        return Some(ExitCode::from(1));
    }
    None
}

/// The values [`run_binary_merge_plan`] reads from its caller.
pub(crate) struct RunBinaryMergePlanArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) config: &'a Config,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) package_options: &'a PackageOptions,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) buildpkg: Option<&'a PackageOptions>,
    pub(crate) status_mode: StatusMode,
    pub(crate) color: Colorizer,
}

/// Merge a plan with binary packages: on a remote host, or here after saving the resume list.
pub(crate) fn run_binary_merge_plan(args: RunBinaryMergePlanArgs<'_>) -> Option<ExitCode> {
    let RunBinaryMergePlanArgs {
        opts,
        entries,
        config,
        repos,
        root,
        portage_tmpdir,
        package_options,
        merge_options,
        buildpkg,
        status_mode,
        color,
    } = args;
    let &EmergeOpts {
        keep_going,
        oneshot,
        onlydeps,
        ref atom_args,
        ref buildpkg_exclude,
        ..
    } = opts;
    // Remote execution (`mrg --remote-hostname` + `--getbinpkgonly`,
    // published via the `REMOTE_EXEC` handoff): the same resolved
    // entries run through the remote plan instead. Binary-only is
    // enforced (usage error, exit 2); resume/world bookkeeping is
    // skipped (the client owns that state -- see remote.rs).
    // The download/ledger `$PKGDIR` is the placed config's own
    // `PKGDIR` (`config.pkgdir`, honouring the resolve's
    // `ConfigRootOverride`), not the process `$PKGDIR` env the
    // local plan uses -- the server stores against the config it
    // resolved against (plan §7/§8).
    if let Some(mut rctx) = crate::remote::take_remote_exec() {
        if let Err(e) = crate::remote::check_binary_plan(entries) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(2));
        }
        // Slice 6: CONFIG_PROTECT comes from the placed config
        // (`Config::resolved_incremental` folds the profile +
        // make.conf stack) unless explicitly flagged -- the
        // client merge then protects what the client config
        // protects, not the flag defaults.
        if !rctx.config_protect_explicit
            && let Some(v) = config.resolved_incremental("CONFIG_PROTECT")
            && !v.is_empty()
        {
            rctx.config_protect = v.join(" ");
        }
        if !rctx.config_protect_mask_explicit
            && let Some(v) = config.resolved_incremental("CONFIG_PROTECT_MASK")
            && !v.is_empty()
        {
            rctx.config_protect_mask = v.join(" ");
        }
        if let Err(e) = crate::remote::run_remote_plan(crate::remote::RunRemotePlanArgs {
            entries,
            config,
            repos,
            root,
            pkgdir: &std::path::PathBuf::from(&config.pkgdir),
            portage_tmpdir,
            ctx: &rctx,
            keep_going,
        }) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(1));
        }
        // Backlog #231 (b): real `post_emerge`'s tail (info regen,
        // preserved-libs notice, config-file check, the GLEP 42
        // notice at `post_emerge.py:155`) runs only once the vdb
        // changed (`post_emerge.py:112-117` early-returns
        // otherwise -- printing just the notice, and only under
        // `--pretend`). The remote plan merges onto the *remote*
        // host (`run_remote_plan` ships every unit through the
        // transport; the local `${ROOT}/var/db/pkg` is never
        // touched), so from this process's vdb nothing changed:
        // real's gate evaluates false on both the success and
        // the failure returns above, and -- being non-`--pretend`
        // here (every `--pretend` exit returned long before this
        // dispatch) -- the tail would print nothing. The early
        // return is therefore the faithful shape; no post-merge
        // notice here by construction, pinned by the
        // local-transport contract test. (The pre-resolution
        // notice already printed before the resolve, like real.)
        return Some(ExitCode::from(0));
    }
    // Real `Scheduler.merge() -> _save_resume_list()` (#168):
    // persist the full mergelist BEFORE the first merge runs, so a
    // SIGKILL mid-merge still leaves `--resume` something to replay
    // (nothing was saved here before -- only the failure path wrote
    // anything). Each successful merge below shrinks it again
    // (`run_merge_loop` / the `-j` scheduler); the failure path
    // re-saves the tail. Silent on write failure, like the
    // `rotate_resume_to_backup` above (a later failure write warns).
    // `--buildpkgonly` and remote execution are deliberately
    // untouched: no save here covers them. The write is
    // unconditional (real always assigns, #179): an all-noop
    // plan records the empty list instead of leaving a stale one.
    let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
    let _ = crate::mtimedb::write_resume_list(root, atom_args, &resume_mergelist, &resume_opts);
    // `emerge --getbinpkg`/`-g` (and `-G`, binary-only): merge
    // every resolved entry, per-entry `Binary` vs `Source` --
    // see `emerge_getbinpkg::run_merge_plan`.
    if let Err(e) = emerge_getbinpkg::run_merge_plan(emerge_getbinpkg::RunMergePlanArgs {
        entries,
        config,
        repos,
        root,
        pkgdir: &package_options.pkgdir,
        portage_tmpdir,
        merge_options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        mode: status_mode,
    }) {
        // Backlog #174: a merge-time binpkg digest failure
        // already printed real's full tail (digest block +
        // `>>> Failed to emerge ...`, real
        // `Scheduler._failed_pkg_msg`) and needs no resume-list
        // notice or `emerge:` line -- real prints neither for a
        // merge-time package failure. Backlog #187 extends the
        // same silence to the missing-file sentinel (real
        // `BinpkgVerifier`'s ENOENT arm). The up-front save above
        // (minus per-success removals) already leaves the tail
        // for `--resume`, like real's own start-of-merge save.
        if crate::emerge_getbinpkg::is_binpkg_digest_failure(&e)
            || crate::emerge_getbinpkg::is_binpkg_missing_failure(&e)
        {
            return Some(ExitCode::from(1));
        }
        // Real `Scheduler._save_resume_list`: on a merge
        // failure, record every still-unmerged package (source
        // *or* binary -- `entries_not_merged`'s own
        // `ResumeEntryKind` tag) so `emerge --resume` can pick
        // up where this left off. Previously missing entirely
        // for this `--getbinpkg` path -- a failed mixed-source-
        // and-binary run left nothing for `--resume` to find.
        // The empty-`unmerged` guard is the failure path's own:
        // `write_resume_list` itself is unconditional now (real
        // `_save_resume_list` always assigns, #179), but a
        // failure with nothing unmerged must leave the
        // per-merge shrink's own tail alone, not overwrite it
        // with an empty list.
        let unmerged = entries_not_merged(root, entries);
        let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
        if !unmerged.is_empty() {
            if let Err(w) =
                crate::mtimedb::write_resume_list(root, atom_args, &unmerged, &resume_opts)
            {
                eprintln!("emerge: warning: could not save the resume list: {w}");
            } else {
                eprintln!(
                    "\n * The resume list contains packages that could not be \
                             merged.\n * Use `emerge --resume` to retry, or `emerge --resume \
                             --skipfirst` to skip the first one."
                );
            }
        }
        // Backlog #196 fix round 1: real `post_emerge.py:155`
        // prints the GLEP 42 notice regardless of retval -- gated
        // on the vdb having changed (`:112-117`), which here is
        // `unmerged.len() < entries.len()` (at least one entry
        // landed; the same structural approximation as the
        // success path's `!entries.is_empty()` below).
        merge_failure_news_notice(unmerged.len() < entries.len(), repos, root, config, &color);
        eprintln!("emerge: {e}");
        return Some(ExitCode::from(1));
    }
    None
}
