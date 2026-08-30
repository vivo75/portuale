//! The source-merge loop: build, then merge, one entry at a time.

use crate::emerge_build::*;

/// Arguments of [`run_source_merge`].
pub struct RunSourceMergeArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a ebuild_merge::MergeOptions,
    pub keep_going: bool,
    pub buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub buildpkg_exclude: &'a [String],
    pub jobs: usize,
    pub load_average: Option<f64>,
    /// Whether to redirect each package's build output to `${T}/build.log`.
    /// Always true for `jobs` > 1; otherwise driven by `--quiet-build=y` / `-q`.
    pub capture_log: bool,
    /// Scheduler-display mode, computed once by the caller from the CLI flags
    /// and the mergelist length. Carries the display half only, never the
    /// capture decision itself.
    pub mode: StatusMode,
}

/// Arguments of [`merge_one_source_entry`].
pub(crate) struct MergeOneSourceEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a StatusDisplay,
}

/// One entry of `run_source_merge`'s loop, also the `Source`-entry arm of
/// the mixed source+binary merge. `AlreadyInstalled` is a silent no-op; a
/// `Binary` entry is a hard error here.
pub(crate) fn merge_one_source_entry(args: MergeOneSourceEntryArgs<'_>) -> Result<(), String> {
    let MergeOneSourceEntryArgs {
        entry,
        repos,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        progress,
        display,
    } = args;
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        // A blocker-removal task is not a merge; skipped like an
        // installed no-op.
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::Uninstall { .. } => {
            return Ok(());
        }
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::NoVisibleCandidate => {
            return Err(format!("{cp}: no visible ebuild to merge"));
        }
    };
    if entry.source == CandidateSource::Binary {
        return Err(format!(
            "{cp}-{version}: resolved to a binary package -- pass `--getbinpkg` \
             for a mixed source+binary merge, or `--getbinpkgonly` for binary-only"
        ));
    }

    let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, &version) else {
        return Err(format!(
            "{cp}-{version}: could not locate its own ebuild file \
             (repo layout changed since resolution?)"
        ));
    };
    let path = ebuild_path(&candidate, &entry.category, &entry.package, &version);

    // The per-package line.
    let color = progress_color();
    display.status(&emerging_line(entry, &version, progress, root, &color));
    if buildpkg.is_some() {
        println!(">>> Building package for {cp}-{version}...");
    }
    let mut per_entry = options.clone();
    // This entry's resolved `USE`, so `use()` in the phases sees the flags.
    per_entry.build_env = entry_build_env(options, entry, repos);
    if let Some((protect, mask)) = reloaded_config_protect(options) {
        per_entry.config_protect = protect;
        per_entry.config_protect_mask = mask;
    }
    // This entry's gates read the per-entry folded list; `None` (no match)
    // keeps the run-wide value.
    if let Some(features) = entry_resolved_features(
        options,
        entry,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        per_entry.set_resolved_features(&features);
    }
    // This entry's build directories live under the matched value, so the
    // pre-clean and the merge below share the resolved root.
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    // The `clean` phase runs before every build, unconditionally (`noclean`
    // only skips the *post*-merge clean). Without it a stale `.installed`
    // marker or an already-prepared image from an earlier build in the same
    // `${PORTAGE_BUILDDIR}` would silently skip `install`.
    // Portage: EbuildBuild.py, _start_pre_clean
    let clean_status = ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        build_env: &per_entry.build_env,
        debug: per_entry.debug,
        config_root: &per_entry.config_root,
        shell: per_entry.shell,
        log_file: per_entry.log_file.as_deref(),
    })?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    // `Installing` prints from `run_merge`'s pre-merge hook, after the build
    // phase output and before the vdb merge; a failed build never reaches
    // the hook, so no `Installing` prints for it.
    let installing = installing_line(entry, &version, progress, root, &color);
    let print_installing = || display.status(&installing);
    let status = ebuild_merge::run_merge_with_hook(ebuild_merge::RunMergeWithHookArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        options: &per_entry,
        buildpkg,
        before_merge: Some(&print_installing as &dyn Fn()),
    })?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
    }
    // The `Completed (N of M) cpv::repo` line.
    display.status(&completed_line(entry, &version, progress, root, &color));
    Ok(())
}
