//! Build and merge the plan from source, after saving it as the resume list.

use crate::ebuild_merge::MergeOptions;
use crate::ebuild_package::PackageOptions;
use crate::emerge_build::StatusMode;
use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;

/// A merge run that failed: the exit code, and the entries that did merge before
/// the failure (empty when nothing landed) for the world and post-merge steps.
pub(crate) struct FailedMerge {
    pub(crate) code: ExitCode,
    pub(crate) merged: Vec<GraphEntry>,
    /// A `pkg_postinst` failure still merged its files, so the post-merge
    /// steps run for it although it has no world entry.
    pub(crate) postinst_landed: bool,
}

impl FailedMerge {
    fn plain(code: ExitCode) -> Option<Self> {
        Some(Self {
            code,
            merged: Vec::new(),
            postinst_landed: false,
        })
    }
}

/// The entries of `entries` that landed before a failure: those whose
/// resume item is no longer left over. Entries whose `pkg_postinst` failed
/// are excluded: their merge landed, but Portage records no world entry
/// for them (`world_atom` is skipped on the postinst-failure path).
// Portage: _emerge/EbuildMerge.py, _merge_exit
fn merged_entries(
    entries: &[GraphEntry],
    unmerged: &[crate::mtimedb::ResumeCpv],
    postinst_failed: &[(String, String)],
) -> Vec<GraphEntry> {
    entries
        .iter()
        .filter(|e| {
            emerge_build::resume_cpv(e).is_some_and(|c| !unmerged.contains(&c))
                && !postinst_failed.contains(&(e.category.clone(), e.package.clone()))
        })
        .cloned()
        .collect()
}

/// The values [`run_source_merge_plan`] reads from its caller.
pub(crate) struct RunSourceMergePlanArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    /// `--jobs` after the interactive-package fallback.
    pub(crate) jobs: usize,
    pub(crate) entries: &'a Vec<GraphEntry>,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) buildpkg: Option<&'a PackageOptions>,
    pub(crate) status_mode: StatusMode,
}

/// Build and merge the plan from source, after saving it as the resume list.
pub(crate) fn run_source_merge_plan(args: RunSourceMergePlanArgs<'_>) -> Option<FailedMerge> {
    let RunSourceMergePlanArgs {
        opts,
        jobs,
        entries,
        root,
        repos,
        portage_tmpdir,
        merge_options,
        buildpkg,
        status_mode,
    } = args;
    let &EmergeOpts {
        oneshot,
        onlydeps,
        ref atom_args,
        keep_going,
        ref buildpkg_exclude,
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
    let resume_opts = crate::mtimedb::ResumeOpts {
        oneshot,
        onlydeps,
        quiet,
        quiet_build,
    };
    let _ = crate::mtimedb::write_resume_list(root, atom_args, &resume_mergelist, &resume_opts);
    let merge_outcome = emerge_build::run_source_merge(emerge_build::RunSourceMergeArgs {
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
    });
    if let Err(e) = merge_outcome {
        // On failure, record every still-unmerged package for `emerge --resume`;
        // a failure with nothing unmerged leaves the per-merge tail alone.
        // Portage: Scheduler.py, _save_resume_list
        let unmerged = unmerged_for_resume_save(root, entries, &e.report);
        let resume_opts = crate::mtimedb::ResumeOpts {
            oneshot,
            onlydeps,
            quiet,
            quiet_build,
        };
        if !unmerged.is_empty()
            && let Err(w) =
                crate::mtimedb::write_resume_list(root, atom_args, &unmerged, &resume_opts)
        {
            eprintln!("emerge: warning: could not save the resume list: {w}");
        }
        // A failure already shown (the `Failed to emerge` block, the elog
        // summary and the failed-package list) adds no `emerge:` line.
        // Portage: Scheduler.py, merge
        if !emerge_build::is_reported_failure(&e.message) {
            eprintln!("emerge: {}", e.message);
        }
        return Some(FailedMerge {
            code: ExitCode::from(1),
            merged: merged_entries(entries, &unmerged, &e.report.postinst_failed),
            postinst_landed: !e.report.postinst_failed.is_empty(),
        });
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
}

/// Merge a plan with binary packages: on a remote host, or here after saving the resume list.
pub(crate) fn run_binary_merge_plan(args: RunBinaryMergePlanArgs<'_>) -> Option<FailedMerge> {
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
    } = args;
    let &EmergeOpts {
        keep_going,
        oneshot,
        onlydeps,
        ref atom_args,
        ref buildpkg_exclude,
        quiet,
        quiet_build,
        ..
    } = opts;
    // Remote execution (`mrg --remote-hostname` + `--getbinpkgonly`): the same
    // entries run through the remote plan instead. Binary-only is enforced
    // (exit 2); resume/world bookkeeping is skipped (the client owns it).
    // The `$PKGDIR` is the placed config's own, not the process env's.
    if let Some(mut rctx) = crate::remote::take_remote_exec() {
        if let Err(e) = crate::remote::check_binary_plan(entries) {
            eprintln!("emerge: {e}");
            return FailedMerge::plain(ExitCode::from(2));
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
            return FailedMerge::plain(ExitCode::from(1));
        }
        // No post-merge notice here by construction: the remote plan merges onto
        // the remote host (the local vdb is never touched), so Portage's
        // vdb-changed gate evaluates false; and this path is never `--pretend`.
        // Portage: post_emerge.py
        return FailedMerge::plain(ExitCode::from(0));
    }
    // Persist the full mergelist BEFORE the first merge runs, so a SIGKILL
    // mid-merge still leaves `--resume` something to replay. Unconditional:
    // an all-noop plan records the empty list. Silent on write failure.
    // Portage: Scheduler.py, merge
    let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
        .iter()
        .filter_map(emerge_build::resume_cpv)
        .collect();
    let resume_opts = crate::mtimedb::ResumeOpts {
        oneshot,
        onlydeps,
        quiet,
        quiet_build,
    };
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
        // Portage's full tail and needs no `emerge:` line; the up-front save
        // already leaves the tail for `--resume`.
        // Portage: Scheduler.py, _failed_pkg_msg
        if crate::emerge_getbinpkg::is_binpkg_digest_failure(&e.message)
            || crate::emerge_getbinpkg::is_binpkg_missing_failure(&e.message)
        {
            return FailedMerge::plain(ExitCode::from(1));
        }
        // On failure, record every still-unmerged package (source or binary)
        // for `emerge --resume`; a failure with nothing unmerged leaves the
        // per-merge shrink's tail alone.
        // Portage: Scheduler.py, _save_resume_list
        let unmerged = unmerged_for_resume_save(root, entries, &e.report);
        let resume_opts = crate::mtimedb::ResumeOpts {
            oneshot,
            onlydeps,
            quiet,
            quiet_build,
        };
        if !unmerged.is_empty()
            && let Err(w) =
                crate::mtimedb::write_resume_list(root, atom_args, &unmerged, &resume_opts)
        {
            eprintln!("emerge: warning: could not save the resume list: {w}");
        }
        if !emerge_build::is_reported_failure(&e.message) {
            eprintln!("emerge: {}", e.message);
        }
        return Some(FailedMerge {
            code: ExitCode::from(1),
            merged: merged_entries(entries, &unmerged, &e.report.postinst_failed),
            postinst_landed: !e.report.postinst_failed.is_empty(),
        });
    }
    None
}
