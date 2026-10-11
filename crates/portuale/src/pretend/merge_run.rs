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
    // Plain `emerge <atom>`: source build + merge. The up-front resume save is
    // Portage `Scheduler.merge()`'s `_save_resume_list`: unconditional, so an
    // all-noop plan records the empty list.
    // Portage: Scheduler.py, merge
    let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
    let _ = crate::mtimedb::write_resume_list(root, atom_args, &resume_mergelist, &resume_opts);
    if let Err(e) = emerge_build::run_source_merge(emerge_build::RunSourceMergeArgs {
        entries,
        repos,
        root,
        portage_tmpdir,
        options: merge_options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        jobs,
        load_average, // Background mode: capture per-package build output when
        // `--quiet-build=y` or `-q` is set (a `-j` >1 run always captures).
        // Portage: Scheduler.py, _background_mode
        capture_log: quiet_build.unwrap_or(false) || quiet,
        mode: status_mode,
    }) {
        // On failure, record every still-unmerged package for `emerge --resume`;
        // a failure with nothing unmerged leaves the per-merge tail alone.
        // Portage: Scheduler.py, _save_resume_list
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
        // The GLEP 42 notice prints regardless of retval, gated on the vdb
        // having changed. Portage: post_emerge.py
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
    // Remote execution (`mrg --remote-hostname` + `--getbinpkgonly`): the same
    // entries run through the remote plan instead. Binary-only is enforced
    // (exit 2); resume/world bookkeeping is skipped (the client owns it).
    // The `$PKGDIR` is the placed config's own, not the process env's.
    if let Some(mut rctx) = crate::remote::take_remote_exec() {
        if let Err(e) = crate::remote::check_binary_plan(entries) {
            eprintln!("emerge: {e}");
            return Some(ExitCode::from(2));
        }
        // `CONFIG_PROTECT` comes from the placed config unless explicitly
        // flagged, so the client merge protects what the client config protects.
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
        // No post-merge notice here by construction: the remote plan merges onto
        // the remote host (the local vdb is never touched), so Portage's
        // vdb-changed gate evaluates false; and this path is never `--pretend`.
        // Portage: post_emerge.py
        return Some(ExitCode::from(0));
    }
    // Persist the full mergelist BEFORE the first merge runs, so a SIGKILL
    // mid-merge still leaves `--resume` something to replay. Unconditional:
    // an all-noop plan records the empty list. Silent on write failure.
    // Portage: Scheduler.py, merge
    let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
    let _ = crate::mtimedb::write_resume_list(root, atom_args, &resume_mergelist, &resume_opts);
    // Merge every resolved entry, per-entry `Binary` vs `Source`.
    // See `emerge_getbinpkg::run_merge_plan`.
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
        // A merge-time binpkg digest/missing-file failure already printed
        // Portage's full tail and needs no resume notice or `emerge:` line;
        // the up-front save already leaves the tail for `--resume`.
        // Portage: Scheduler.py, _failed_pkg_msg
        if crate::emerge_getbinpkg::is_binpkg_digest_failure(&e)
            || crate::emerge_getbinpkg::is_binpkg_missing_failure(&e)
        {
            return Some(ExitCode::from(1));
        }
        // On failure, record every still-unmerged package (source or binary)
        // for `emerge --resume`; a failure with nothing unmerged leaves the
        // per-merge shrink's tail alone.
        // Portage: Scheduler.py, _save_resume_list
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
        // The GLEP 42 notice prints regardless of retval, gated on the vdb
        // having changed (here: at least one entry landed).
        // Portage: post_emerge.py
        merge_failure_news_notice(unmerged.len() < entries.len(), repos, root, config, &color);
        eprintln!("emerge: {e}");
        return Some(ExitCode::from(1));
    }
    None
}
