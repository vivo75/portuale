//! The parallel build scheduler (`--jobs`).

use crate::emerge_build::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// Arguments of [`build_one_source_entry`].
pub(crate) struct BuildOneSourceEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub(crate) capture_log: bool,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a StatusDisplay,
    /// The up-front `pkg_pretend` pass already ran: start at `setup`.
    pub(crate) skip_pretend: bool,
}

/// The `install` phase chain and any `--buildpkg` binpkg for one source
/// entry, without the vdb merge. Returns the located ebuild path for
/// `merge_one_built_entry` to reuse. Safe to run from a scheduler worker
/// thread: every build gets its own `${PORTAGE_BUILDDIR}` and the
/// environment is passed explicitly. With `capture_log` the `install`
/// output goes to `${T}/build.log`, whose tail is folded into the error
/// on failure.
// Portage: EbuildBuild.py, EbuildBuild; EbuildBinpkg.py, EbuildBinpkg
pub(crate) fn build_one_source_entry(args: BuildOneSourceEntryArgs<'_>) -> Result<PathBuf, String> {
    let BuildOneSourceEntryArgs {
        entry,
        repos,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        capture_log,
        progress,
        display,
        skip_pretend,
    } = args;
    let (cp, version) = scheduler_cp_version(entry)?;
    let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, &version) else {
        return Err(format!(
            "{cp}-{version}: could not locate its own ebuild file \
             (repo layout changed since resolution?)"
        ));
    };
    let path = ebuild_path(&candidate, &entry.category, &entry.package, &version);
    // The build half prints the per-package line when the build starts.
    let color = progress_color();
    display.status(&emerging_line(entry, &version, progress, root, &color));
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();

    let log_path = capture_log.then(|| {
        build_log_path(
            portage_tmpdir,
            &entry.category,
            &entry.package,
            &version,
            &resolved_features(options),
        )
    });
    if let Some(lp) = &log_path {
        // Truncate, don't delete: with `PORTAGE_LOGDIR` set `lp` is a
        // symlink, and deleting it would strand the new log in `${T}`.
        // `set_len(0)` follows the link to the file.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(lp) {
            let _ = f.set_len(0);
        }
    }
    // A captured parallel build runs through `bash`, whose stdout+stderr
    // redirect to `build.log` cleanly at the OS level.
    let shell = if log_path.is_some() {
        ebuild_phases::ShellBackend::Bash
    } else {
        options.shell
    };
    let build_env = entry_build_env(options, entry, repos);
    // The `clean` phase runs before every build, unconditionally.
    let clean_status = ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        build_env: &build_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: ebuild_phases::PhaseLog::capture(log_path.as_deref()),
    })?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    let status = ebuild_phases::run_commands_logged(ebuild_phases::RunCommandsLoggedArgs {
        ebuild_path: &path,
        commands: &["install"],
        root,
        portage_tmpdir,
        distdir: &options.distdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: ebuild_phases::PhaseLog::capture(log_path.as_deref()),
        build_env: &build_env,
        // Same merge-path `EbuildExecuter` skip as the serial merge
        // above (backlog #456); the background capture only decides
        // where the phases that do run are logged.
        // Portage: _emerge/EbuildExecuter.py, _can_skip_source_phases
        skip_noop_src_phases: true,
        // Set once the up-front `pkg_pretend` pass has replayed this
        // entry's output: the build starts at `setup`.
        skip_pretend,
    })?;
    if status != 0 {
        return Err(format!("{cp}-{version}: build failed ({status})"));
    }
    if let Some(package_options) = buildpkg {
        println!(">>> Building package for {cp}-{version}...");
        // The binpkg's `USE` field is the same resolved flag set the
        // `install` phase just ran with, reused verbatim so the two can
        // never drift apart.
        let use_flags = build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // Re-derive the binpkg-affecting tokens from the per-entry folded
        // list; `None` (no match) keeps the already-resolved value.
        let mut per_entry_package_options = package_options.clone();
        let per_entry_features = entry_resolved_features(
            options,
            entry,
            &std::env::var("FEATURES").unwrap_or_default(),
        );
        if let Some(features) = &per_entry_features {
            per_entry_package_options.set_resolved_features(features);
        }
        // The layout stays run-wide; the `BUILD_ID` export follows this
        // entry's own token.
        let per_entry_binpkg_multi_instance = per_entry_features
            .as_deref()
            .map(|features| {
                features
                    .split_whitespace()
                    .any(|t| t == "binpkg-multi-instance")
            })
            .unwrap_or(per_entry_package_options.binpkg_multi_instance);
        let status =
            ebuild_package::package_after_install(ebuild_package::PackageAfterInstallArgs {
                ebuild_path: &path,
                root,
                portage_tmpdir,
                options: &per_entry_package_options,
                use_flags,
                per_entry_binpkg_multi_instance,
            })?;
        if status != 0 {
            return Err(format!("{cp}-{version}: binpkg build failed ({status})"));
        }
    }
    Ok(path)
}

/// Arguments of [`merge_one_built_entry`].
pub(crate) struct MergeOneBuiltEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) ebuild_path: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) progress: mrg_director::MergeProgress,
    pub(crate) display: &'a StatusDisplay,
}

/// The vdb merge for a source entry whose `install` phase already ran.
/// Always serialized; only the build above runs in parallel. Reuses
/// `run_qmerge` (same-slot replace of an upgraded/reinstalled version
/// included).
pub(crate) fn merge_one_built_entry(args: MergeOneBuiltEntryArgs<'_>) -> Result<(), String> {
    let MergeOneBuiltEntryArgs {
        entry,
        repos,
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        progress,
        display,
    } = args;
    let (cp, version) = scheduler_cp_version(entry)?;
    // `pkg_preinst`/`pkg_postinst` see this entry's resolved `USE` too.
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry, repos);
    if let Some((protect, mask)) = reloaded_config_protect(options) {
        per_entry.config_protect = protect;
        per_entry.config_protect_mask = mask;
    }
    // The merge half's gates read the per-entry folded list, not the
    // run-wide one; `None` (no match) keeps the run-wide value.
    if let Some(features) = entry_resolved_features(
        options,
        entry,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        per_entry.set_resolved_features(&features);
    }
    // The merge half runs under the same resolved root the build half
    // used (resolved again here to keep the `portage_tmpdir` contract).
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    // Only reached once the build half captured `install` output here, so
    // `pkg_preinst`/`pkg_postinst` output appends to the same continuous log.
    per_entry.log_file = Some(build_log_path(
        portage_tmpdir,
        &entry.category,
        &entry.package,
        &version,
        &resolved_features(options),
    ));
    // The merge half prints its per-package line before the serialized
    // vdb merge.
    let color = progress_color();
    display.status(&installing_line(entry, &version, progress, root, &color));
    let outcome = ebuild_merge::run_qmerge(ebuild_path, root, portage_tmpdir, &per_entry)?;
    if outcome.postinst_failed {
        // The merge landed: `>>> Completed` prints like
        // `PackageMerge._install_exit`, and the world record and the
        // post-merge clean below are both skipped for this entry (the
        // `_postinst_failure` builddir rule).
        // Portage: _emerge/PackageMerge.py, _install_exit; vartree.py
        display.status(&completed_line(entry, &version, progress, root, &color));
        return Err(postinst_failed_error(&cp, &version, outcome.status));
    }
    if outcome.status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({})", outcome.status));
    }
    // The `clean` phase runs after a successful merge unless
    // `FEATURES=noclean`. `run_qmerge` itself deliberately does not clean
    // (`doebuild qmerge` implies noclean), so this is where the split gets
    // the post-merge behavior.
    if !ebuild_merge::feature_enabled(&per_entry, "noclean") {
        ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
            ebuild_path,
            root,
            portage_tmpdir,
            build_env: &per_entry.build_env,
            debug: per_entry.debug,
            config_root: &per_entry.config_root,
            shell: per_entry.shell,
            log_file: ebuild_phases::PhaseLog::capture(per_entry.log_file.as_deref()),
        })?;
    }
    // The `Completed (N of M) cpv::repo` line.
    if let Some(record) = &per_entry.world_recorder {
        for line in record(entry) {
            display.status(&line);
        }
    }
    display.status(&completed_line(entry, &version, progress, root, &color));
    Ok(())
}

/// Marks every (transitive) dependent of `idx` as skipped.
// Portage: Scheduler.py, _calc_resume_list
pub(crate) fn scheduler_skip_dependents(
    idx: usize,
    entries: &[GraphEntry],
    cp_to_idx: &std::collections::HashMap<(String, String), usize>,
    skip: &mut std::collections::HashSet<usize>,
    skipped: &mut Vec<String>,
) {
    let mut queue = vec![idx];
    while let Some(x) = queue.pop() {
        for r_cp in &entries[x].required_by {
            if let Some(&r_idx) = cp_to_idx.get(r_cp)
                && skip.insert(r_idx)
            {
                skipped.push(format!(
                    "{}/{}",
                    entries[r_idx].category, entries[r_idx].package
                ));
                queue.push(r_idx);
            }
        }
    }
}

/// Runs up to `jobs` package *builds* concurrently, dispatching a build
/// only once every dependency in the merge set is already merged, and
/// serializing the vdb merge step. `jobs == usize::MAX` (a bare `--jobs`)
/// means "as many as the graph allows".
///
/// The `--load-average` gate reads the 1-minute load average, or `0.0` when
/// unreadable (which disables the throttle rather than stalling).
pub(crate) fn system_loadavg_1min() -> f64 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}

/// Arguments of [`run_build_scheduler`].
pub(crate) struct RunBuildSchedulerArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) keep_going: bool,
    pub(crate) buildpkg: Option<&'a ebuild_package::PackageOptions>,
    pub(crate) buildpkg_exclude: &'a [String],
    /// The `-jN` dispatch policy: a capped or uncapped scheduler is one
    /// caller-side value, never a loop change here.
    pub(crate) policy: &'a dyn mrg_director::SchedulerPolicy,
    /// The run's status display.
    pub(crate) display: &'a StatusDisplay,
}

/// Arguments of [`pretend_one_entry`].
pub(crate) struct PretendOneEntryArgs<'a> {
    pub(crate) entry: &'a GraphEntry,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) log_path: &'a Path,
}

/// One entry's `pkg_pretend`, captured to `log_path`. Mirrors the build
/// half's setup (own tmpdir, own env, `clean` first) but runs only the
/// `pretend` chain: the build later starts at `setup`.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one
pub(crate) fn pretend_one_entry(args: PretendOneEntryArgs<'_>) -> Result<(), String> {
    let PretendOneEntryArgs {
        entry,
        repos,
        root,
        portage_tmpdir,
        options,
        log_path,
    } = args;
    let (cp, version) = scheduler_cp_version(entry)?;
    let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, &version) else {
        return Err(format!(
            "{cp}-{version}: could not locate its own ebuild file \
             (repo layout changed since resolution?)"
        ));
    };
    let path = ebuild_path(&candidate, &entry.category, &entry.package, &version);
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    let build_env = entry_build_env(options, entry, repos);
    // An `IDEPEND` dep pretends in the running root, like it builds there.
    let entry_root = entry_merge_root(entry, root);
    let root = entry_root.as_path();
    // A captured pretend always has a log, so it runs through `bash` like
    // a captured build does.
    let shell = ebuild_phases::ShellBackend::Bash;
    let clean_status = ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        build_env: &build_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: ebuild_phases::PhaseLog::Capture(log_path),
    })?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    let status = ebuild_phases::run_commands_logged(ebuild_phases::RunCommandsLoggedArgs {
        ebuild_path: &path,
        commands: &["pretend"],
        root,
        portage_tmpdir,
        distdir: &options.distdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: ebuild_phases::PhaseLog::Capture(log_path),
        build_env: &build_env,
        skip_noop_src_phases: false,
        skip_pretend: false,
    })?;
    if status != 0 {
        return Err(format!("{cp}-{version}: pkg_pretend failed ({status})"));
    }
    Ok(())
}

/// Whether the up-front `pkg_pretend` pass must run for this entry: a
/// `New`/`Upgrade`/`Downgrade`/`Reinstall` from either source or binary.
/// `AlreadyInstalled`/`Uninstall`/`NoVisibleCandidate` never pretend.
/// Portage: Scheduler.py, `_pkg_needs_pretend` (the EAPI/`DEFINED_PHASES`
/// half is evaluated by the phase run itself for the hookless case... see
/// `source_entry_needs_pretend` for the source gate actually applied).
pub(crate) fn pretend_needs_run(entry: &GraphEntry) -> bool {
    matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Reinstall { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
    )
}

/// Portage's `_pkg_needs_pretend` EAPI/`DEFINED_PHASES` half for a source
/// entry: EAPI 4+ with `pretend` in `DEFINED_PHASES` (read from the
/// md5-cache, the same rung `list_candidates` trusts). Fail-open: an
/// unlocatable ebuild or unreadable cache keeps the entry in the pass, so
/// `pretend_one_entry` reports the real error instead of silently
/// skipping. Without this gate the pass prints `>>> Running pre-merge
/// checks` (and runs `clean`) for entries Portage never pretends, which
/// breaks the serial status-line blanks pmtest pins for pretend-less
/// packages.
// Portage: _emerge/Scheduler.py, `_pkg_needs_pretend`
pub(crate) fn source_entry_needs_pretend(entry: &GraphEntry, repos: &[RepoConfig]) -> bool {
    let Some(version) = entry_version(entry) else {
        return false;
    };
    let candidates = match portage_repo::list_candidates(repos, &entry.category, &entry.package) {
        Ok(c) => c,
        Err(_) => return true,
    };
    let Some(candidate) = candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
        .cloned()
    else {
        return true;
    };
    let pf = format!("{}-{version}", entry.package);
    let meta = match portage_repo::repo_aux_metadata(&candidate.repo_location, &entry.category, &pf)
    {
        Ok(m) => m,
        Err(_) => return true,
    };
    let eapi_ok = !matches!(
        meta.get("EAPI").map(String::as_str),
        Some("0" | "1" | "2" | "3")
    );
    let pretend_defined = meta
        .get("DEFINED_PHASES")
        .is_some_and(|d| d.split_whitespace().any(|p| p == "pretend"));
    eapi_ok && pretend_defined
}

/// One source entry plus everything the pretend pass needs for it: the
/// gate's repos, the run's roots and the merge options.
pub(crate) struct SourcePretendEntry<'a> {
    pub entry: &'a GraphEntry,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a ebuild_merge::MergeOptions,
}

impl PretendEntry for SourcePretendEntry<'_> {
    fn entry(&self) -> &GraphEntry {
        self.entry
    }
    fn needs_pretend(&self) -> bool {
        scheduler_needs_build(self.entry) && source_entry_needs_pretend(self.entry, self.repos)
    }
    fn entry_tmpdir(&self) -> Result<PathBuf, String> {
        let Some(version) = self.version() else {
            return Err(format!(
                "{}/{}: no version to pretend",
                self.entry.category, self.entry.package
            ));
        };
        entry_portage_tmpdir(self.options, self.entry, version, self.portage_tmpdir)
    }

    fn run_pretend(&self) -> PretendRun {
        let (cp, version) = match scheduler_cp_version(self.entry) {
            Ok(found) => found,
            Err(e) => {
                return PretendRun {
                    log: None,
                    result: Err(e),
                };
            }
        };
        let t_base = match self.entry_tmpdir() {
            Ok(t_base) => t_base,
            Err(e) => {
                return PretendRun {
                    log: None,
                    result: Err(e),
                };
            }
        };
        run_staged_pretend(RunStagedPretendArgs {
            entry: self.entry,
            cp: &cp,
            version: &version,
            portage_tmpdir: self.portage_tmpdir,
            t_base: &t_base,
            run: &|staging| {
                pretend_one_entry(PretendOneEntryArgs {
                    entry: self.entry,
                    repos: self.repos,
                    root: self.root,
                    portage_tmpdir: self.portage_tmpdir,
                    options: self.options,
                    log_path: staging,
                })
            },
        })
    }
}

/// What [`note_landed`] tells the dispatch loop after one built entry lands.
enum Landed {
    /// Keep scheduling.
    Continue,
    /// Stop the pass now (a non-`--keep-going` failure killed the rest).
    Stop,
}

/// Arguments of [`land_built_entry`].
pub(crate) struct LandBuiltEntryArgs<'a> {
    pub(crate) idx: usize,
    pub(crate) path: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a ebuild_merge::MergeOptions,
    pub(crate) progress: &'a [mrg_director::MergeProgress],
    pub(crate) display: &'a StatusDisplay,
}

/// The serialized vdb merge for one finished build, on the main thread
/// while workers keep building. The caller already freed the build slot
/// (`build_finished` fires at build exit, queued or not). Returns the
/// failure, if any.
// Portage: _emerge/Scheduler.py, _build_exit
fn land_built_entry(args: LandBuiltEntryArgs<'_>) -> Option<(String, bool)> {
    let LandBuiltEntryArgs {
        idx,
        path,
        entries,
        repos,
        root,
        portage_tmpdir,
        options,
        progress,
        display,
    } = args;
    let entry = &entries[idx];
    let entry_progress = progress[idx];
    let entry_root = entry_merge_root(entry, root);
    let err = merge_one_built_entry(MergeOneBuiltEntryArgs {
        entry,
        repos,
        ebuild_path: path,
        root: &entry_root,
        portage_tmpdir,
        options,
        progress: entry_progress,
        display,
    })
    .err();
    if err.is_some() {
        // The build slot is already free, so only the `failed` event
        // fires here.
        display.job_failed();
    }
    err.map(|e| {
        let postinst = is_postinst_failure(&e);
        (e, postinst)
    })
}

/// Arguments of [`note_landed`].
pub(crate) struct NoteLandedArgs<'a, 'b> {
    pub(crate) idx: usize,
    pub(crate) failure: Option<(String, bool)>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) cp_to_idx: &'a HashMap<(String, String), usize>,
    pub(crate) root: &'a Path,
    pub(crate) keep_going: bool,
    pub(crate) registry: &'a std::sync::Mutex<ebuild_phases::SchedulerRegistry>,
    pub(crate) report: &'a FailReport<'b>,
    pub(crate) ledger: &'a mut Ledger,
    pub(crate) unreported: &'a mut Vec<String>,
    pub(crate) merged: &'a mut HashSet<usize>,
    pub(crate) skip: &'a mut HashSet<usize>,
    pub(crate) gone: &'a mut HashSet<usize>,
    pub(crate) pass_report: &'a mut MergePassReport,
    pub(crate) wait_queue: &'a mut VecDeque<(usize, PathBuf)>,
    pub(crate) display: &'a StatusDisplay,
    pub(crate) halted: &'a mut bool,
}

/// The shared bookkeeping for one landed build: success merges into the
/// schedule and the resume list, failure reports and (without
/// `--keep-going`) stops the pass. A stop also clears the merge-wait
/// queue: terminated runs never drain it.
// Portage: _emerge/Scheduler.py, _task_complete
fn note_landed(args: NoteLandedArgs<'_, '_>) -> Landed {
    let NoteLandedArgs {
        idx,
        failure,
        entries,
        cp_to_idx,
        root,
        keep_going,
        registry,
        report,
        ledger,
        unreported,
        merged,
        skip,
        gone,
        pass_report,
        wait_queue,
        display,
        halted,
    } = args;
    match failure {
        None => {
            merged.insert(idx);
            // Each successful merge leaves the saved resume list
            // (removal by value, so completion order is fine).
            if let Some(cpv) = resume_cpv(&entries[idx]) {
                crate::mtimedb::remove_merged_entry(root, &cpv);
            }
            display.merge_finished();
        }
        Some((e, postinst)) => {
            if postinst {
                // The merge landed: dependents stay dispatchable
                // (like `Scheduler._task_complete`), but the saved
                // resume list keeps the entry (like `vartree`,
                // which removes it only on a clean return).
                // Portage: Scheduler.py, _do_merge_exit
                merged.insert(idx);
                // No dependents are dropped (their requirement is
                // satisfied), but no new work is scheduled in this
                // pass either.
                // Portage: Scheduler.py, _keep_scheduling,
                // _calc_resume_list
                pass_report
                    .postinst_failed
                    .push((entries[idx].category.clone(), entries[idx].package.clone()));
                *halted = true;
            }
            if !ledger.failed(report, &entries[idx], &e) {
                unreported.push(e);
            }
            if !keep_going {
                // Without `--keep-going` no new work is scheduled and
                // still-running builds are killed, not waited out.
                ebuild_phases::kill_registered_children(registry);
                wait_queue.clear();
                display.set_merge_wait(0);
                return Landed::Stop;
            }
            if !postinst {
                gone.insert(idx);
                let mut skipped = Vec::new();
                scheduler_skip_dependents(idx, entries, cp_to_idx, skip, &mut skipped);
            }
        }
    }
    Landed::Continue
}

/// Runs up to `jobs` package *builds* concurrently, dispatching a build
/// only once every blocking dependency is already merged, and serializing
/// the vdb merge step. A `pkg_pretend` pass runs before any build, its
/// outputs replayed in merge-list order; merges in the wait scope land
/// only while no build runs; inside a cycle post-merge edges do not gate
/// a build. `jobs == usize::MAX` (a bare `--jobs`) means "as many as the
/// graph allows".
///
/// Known cuts: each build's phase output goes to its own `${T}/build.log`
/// (captured), and a non-`--keep-going` failure stops scheduling new work
/// and kills still-running builds.
pub(crate) fn run_build_scheduler(
    args: RunBuildSchedulerArgs<'_>,
) -> Result<MergePassReport, MergePassError> {
    let RunBuildSchedulerArgs {
        entries,
        repos,
        root,
        portage_tmpdir,
        options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        policy,
        display,
    } = args;
    use std::sync::mpsc;

    let n = entries.len();
    let cp_to_idx: HashMap<(String, String), usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| ((e.category.clone(), e.package.clone()), i))
        .collect();

    // idx -> the indices (present in this graph) it depends on,
    // reconstructed from the reverse `required_by` edges.
    let deps: Vec<HashSet<usize>> = {
        let mut deps = vec![HashSet::new(); n];
        for (d_idx, d) in entries.iter().enumerate() {
            for r_cp in &d.required_by {
                if let Some(&r_idx) = cp_to_idx.get(r_cp) {
                    deps[r_idx].insert(d_idx);
                }
            }
        }
        deps
    };
    // Inside a cycle post-merge edges do not gate a build; everything
    // else dispatches exactly as before.
    // Portage: _emerge/Scheduler.py, _find_cyclic_nodes
    let adj: Vec<Vec<usize>> = deps
        .iter()
        .map(|ds| {
            let mut v: Vec<usize> = ds.iter().copied().collect();
            v.sort_unstable();
            v
        })
        .collect();
    let blocking = blocking_deps(entries, &deps, &cyclic_nodes(&adj));
    // Who merges only while no build runs: the scope's set, or everything
    // under `FEATURES=merge-wait` (on by default).
    // Portage: _emerge/Scheduler.py, _build_exit
    let system_atoms: Vec<String> = options
        .resolved_config
        .as_deref()
        .map(|c| c.system_packages.clone())
        .unwrap_or_default();
    let wait_set = merge_wait_set(entries, &system_atoms, options.merge_wait_scope);
    // Who else barriers: `FEATURES=merge-wait` holds every merge, but the
    // check is per package (a `package.env` entry can add or drop the
    // token for one package), falling back to the run-wide list.
    // Portage: _emerge/Scheduler.py, _build_exit
    let run_hold = features_hold_all_merges(&resolved_features(options));
    let calling_features = std::env::var("FEATURES").unwrap_or_default();
    let hold: Vec<bool> = entries
        .iter()
        .map(|e| {
            entry_resolved_features(options, e, &calling_features)
                .as_deref()
                .map_or(run_hold, features_hold_all_merges)
        })
        .collect();

    // Entries that need no build count as already merged, so their
    // dependents become dispatchable immediately.
    let mut merged: HashSet<usize> = (0..n)
        .filter(|&i| !scheduler_needs_build(&entries[i]))
        .collect();
    let needs_work = |i: usize| -> bool { scheduler_needs_build(&entries[i]) };
    let mut started: HashSet<usize> = HashSet::new();
    let mut skip: HashSet<usize> = HashSet::new();
    let mut ledger = Ledger::default();
    let mut unreported: Vec<String> = Vec::new();
    let mut gone: HashSet<usize> = HashSet::new();
    let mut pass_report = MergePassReport::default();
    let source_paths = |entry: &GraphEntry| source_paths_for(options, entry, portage_tmpdir);
    let report = FailReport {
        root,
        source_paths: &source_paths,
        features: resolved_features(options),
        quiet: options.quiet,
        background: display.mode.background,
        quiet_fail: options.quiet_fail,
    };
    // One registry for this call only, shared with the worker threads it
    // spawns (never process-wide, so another test thread's subprocess is
    // unreachable from here).
    let registry = ebuild_phases::new_scheduler_registry();
    // Every build/merge half below prints its entry's own snapshot.
    let mut progress = merge_progress_map(entries);
    // A fatal failure (`--keep-going`) ends the pass like Portage's
    // `_keep_scheduling` gate: what never ran re-plans in a resumed pass
    // with restarted counters. Only postinst failures halt this way (a
    // build failure leaves already-running work to land in the same pass,
    // matching the pinned `-j` keep-going shape).
    let mut resumed = false;
    // Finished builds whose merge waits for idle builders, in finish
    // order. Drained one at a time whenever no build runs, so a queued
    // system merge never runs beside another merge or build.
    // Portage: _emerge/Scheduler.py, _merge_wait_queue
    let mut wait_queue: VecDeque<(usize, PathBuf)> = VecDeque::new();

    // The up-front `pkg_pretend` pass runs before any build, its outputs
    // replayed in merge-list order; a failure aborts before anything builds.
    // Portage: _emerge/Scheduler.py, _run_pkg_pretend
    let failures = {
        // The replay sink is stdout: `pkg_pretend` output is important
        // enough to bypass `--quiet`/`--jobs`, like Portage. The lock
        // must drop before the loop below prints anything itself.
        let stdout = std::io::stdout();
        let mut replay = stdout.lock();
        let wrapped: Vec<SourcePretendEntry<'_>> = entries
            .iter()
            .map(|entry| SourcePretendEntry {
                entry,
                repos,
                root,
                portage_tmpdir,
                options,
            })
            .collect();
        run_pretend_pass(RunPretendPassArgs {
            entries: &wrapped,
            jobs: policy.max_jobs(),
            replay: &mut replay,
        })
    };
    if finish_failed_pretend(FinishFailedPretendArgs {
        report: &report,
        entries,
        failures,
        keep_going,
    })? {
        return Ok(pass_report);
    }

    let loop_result = std::thread::scope(|scope| -> Result<(), String> {
        let (tx, rx) = mpsc::channel::<(usize, Result<PathBuf, String>)>();
        loop {
            let mut halted = false;
            let mut in_flight = 0usize;
            let mut dispatched_this_pass = false;
            loop {
                // Merge-wait: with no builders running, land the whole
                // queued batch in finish order before anything else
                // dispatches, so a merge freed by one landing never builds
                // beside a still-queued one.
                // Portage: _emerge/Scheduler.py, _schedule_tasks
                while in_flight == 0 && !wait_queue.is_empty() {
                    let (qidx, qpath) = wait_queue.pop_front().expect("non-empty");
                    display.set_merge_wait(wait_queue.len());
                    let failure = land_built_entry(LandBuiltEntryArgs {
                        idx: qidx,
                        path: &qpath,
                        entries,
                        repos,
                        root,
                        portage_tmpdir,
                        options,
                        progress: &progress,
                        display,
                    });
                    if let Landed::Stop = note_landed(NoteLandedArgs {
                        idx: qidx,
                        failure,
                        entries,
                        cp_to_idx: &cp_to_idx,
                        root,
                        keep_going,
                        registry: &registry,
                        report: &report,
                        ledger: &mut ledger,
                        unreported: &mut unreported,
                        merged: &mut merged,
                        skip: &mut skip,
                        gone: &mut gone,
                        pass_report: &mut pass_report,
                        wait_queue: &mut wait_queue,
                        display,
                        halted: &mut halted,
                    }) {
                        return Ok(());
                    }
                }
                // `max_jobs` caps concurrency, then `should_start` gates one more
                // build on the live load average. The first build is never
                // gated, so the DAG cannot deadlock.
                while !halted && in_flight < policy.max_jobs() {
                    if !policy.should_start(in_flight, system_loadavg_1min()) {
                        break;
                    }
                    let next = (0..n).find(|&i| {
                        needs_work(i)
                            && !started.contains(&i)
                            && !skip.contains(&i)
                            && !merged.contains(&i)
                            && blocking[i].iter().all(|d| merged.contains(d))
                    });
                    let Some(idx) = next else { break };
                    started.insert(idx);
                    dispatched_this_pass = true;
                    in_flight += 1;
                    // The `>>> Jobs:` line prints at dispatch, before the
                    // worker's `>>> Emerging` line.
                    display.job_started();
                    // The worker closure is `move` and cannot borrow the map.
                    let entry_progress = progress[idx];
                    let tx = tx.clone();
                    let bp = buildpkg.filter(|opts| {
                        entry_buildpkg_wanted(
                            &entries[idx],
                            repos,
                            buildpkg_exclude,
                            opts.buildpkg_live,
                        )
                    });
                    let entry = &entries[idx];
                    let registry = registry.clone();
                    // An `IDEPEND` dep builds and merges in the running root.
                    let entry_root = entry_merge_root(entry, root);
                    scope.spawn(move || {
                        let root = entry_root.as_path();
                        // Registers every subprocess this worker spawns into
                        // `registry` for the closure's own lifetime.
                        let _guard = ebuild_phases::scope_scheduler_registry(registry);
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            build_one_source_entry(BuildOneSourceEntryArgs {
                                entry,
                                repos,
                                root,
                                portage_tmpdir,
                                options,
                                buildpkg: bp,
                                // Each build's output goes to its own
                                // `${T}/build.log` so parallel builds
                                // don't interleave on the terminal.
                                capture_log: true,
                                progress: entry_progress,
                                display,
                                // `pkg_pretend` ran up front and its
                                // output already replayed: the build
                                // starts at `setup`.
                                skip_pretend: true,
                            })
                        }))
                        .unwrap_or_else(|payload| Err(worker_panic_message(entry, payload)));
                        let _ = tx.send((idx, r));
                    });
                }

                if in_flight == 0 {
                    break;
                }

                let (idx, build_result) = rx
                    .recv()
                    .map_err(|e| format!("scheduler channel closed unexpectedly: {e}"))?;
                in_flight -= 1;

                let (failure, queued): (Option<(String, bool)>, bool) = match build_result {
                    Ok(path) => {
                        // The build slot is free before the serialized
                        // merge starts, queued or not.
                        display.build_finished();
                        // A gated merge always queues, landing FIFO once idle
                        // instead of beside a running build.
                        // Portage: _emerge/Scheduler.py, _build_exit
                        if wait_set.contains(&idx) || hold[idx] {
                            wait_queue.push_back((idx, path));
                            display.set_merge_wait(wait_queue.len());
                            (None, true)
                        } else {
                            (
                                land_built_entry(LandBuiltEntryArgs {
                                    idx,
                                    path: &path,
                                    entries,
                                    repos,
                                    root,
                                    portage_tmpdir,
                                    options,
                                    progress: &progress,
                                    display,
                                }),
                                false,
                            )
                        }
                    }
                    Err(e) => {
                        display.job_failed();
                        display.build_finished();
                        (Some((e, false)), false)
                    }
                };
                // A queued merge is neither merged nor failed yet: only a
                // landed build runs the shared bookkeeping.
                if !queued
                    && let Landed::Stop = note_landed(NoteLandedArgs {
                        idx,
                        failure,
                        entries,
                        cp_to_idx: &cp_to_idx,
                        root,
                        keep_going,
                        registry: &registry,
                        report: &report,
                        ledger: &mut ledger,
                        unreported: &mut unreported,
                        merged: &mut merged,
                        skip: &mut skip,
                        gone: &mut gone,
                        pass_report: &mut pass_report,
                        wait_queue: &mut wait_queue,
                        display,
                        halted: &mut halted,
                    })
                {
                    return Ok(());
                }
            }
            // What never ran (a halted pass's leftovers, never a skipped or
            // failed entry) re-plans in a resumed pass with restarted counters.
            let remaining: Vec<usize> = (0..n)
                .filter(|&i| {
                    needs_work(i)
                        && !started.contains(&i)
                        && !skip.contains(&i)
                        && !gone.contains(&i)
                })
                .collect();
            if remaining.is_empty() {
                break;
            }
            if !keep_going || !dispatched_this_pass {
                // Without `--keep-going` every failure returns early, so this arm
                // is unreachable; without a single dispatch all pass the leftovers
                // wait on a skipped or failed entry and join the dropped epilogue
                // below.
                break;
            }
            // Portage: Scheduler.py, _merge (resume loop), _calc_resume_list
            ledger.resuming(report.quiet);
            resumed = true;
            pass_report.resumed = true;
            display.reset_for_resume(remaining.len());
            let rerun: Vec<GraphEntry> = remaining.iter().map(|&i| entries[i].clone()).collect();
            for (k, m) in merge_progress_map(&rerun).into_iter().enumerate() {
                progress[remaining[k]] = m;
            }
            resave_resume_list(root, &remaining, entries);
        }
        Ok(())
    });
    loop_result?;

    // The resumed pass: whatever never ran because a dependency failed is
    // dropped with a notice.
    // Portage: Scheduler.py, _merge (resume loop), _calc_resume_list
    if keep_going && !skip.is_empty() {
        if !resumed {
            ledger.resuming(report.quiet);
        }
        let mut dropped: Vec<usize> = skip
            .iter()
            .copied()
            .filter(|i| !merged.contains(i))
            .collect();
        dropped.sort_unstable();
        for i in dropped {
            let mut atoms: Vec<String> = gone
                .iter()
                .chain(skip.iter())
                .flat_map(|&g| {
                    atoms_requiring(
                        &entries[i],
                        &(entries[g].category.clone(), entries[g].package.clone()),
                    )
                })
                .collect();
            atoms.sort();
            atoms.dedup();
            ledger.dropped(&report, &entries[i], &atoms);
        }
    }
    let first = (!unreported.is_empty()).then(|| unreported.join("\n"));
    match ledger.finish(&report, keep_going, first) {
        Ok(()) => Ok(pass_report),
        Err(message) => Err(MergePassError {
            message,
            report: pass_report,
        }),
    }
}

/// The failure a worker thread that panicked reports in place of its result.
/// Without it the scheduler would wait forever on a result nobody sends (its
/// own `tx` clone keeps the channel open), so a panic became a silent hang
/// (backlog #376).
pub(crate) fn worker_panic_message(
    entry: &GraphEntry,
    payload: Box<dyn std::any::Any + Send>,
) -> String {
    let what = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    format!(
        "{}/{}: build thread panicked: {what}",
        entry.category, entry.package
    )
}
