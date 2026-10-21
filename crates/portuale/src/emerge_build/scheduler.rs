//! The parallel build scheduler (`--jobs`).

use crate::emerge_build::*;

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
    let status = ebuild_merge::run_qmerge(ebuild_path, root, portage_tmpdir, &per_entry)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
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

/// Known cuts: each build's phase output goes to its own `${T}/build.log`
/// (captured), and a non-`--keep-going` failure stops scheduling new work
/// and kills still-running builds.
pub(crate) fn run_build_scheduler(args: RunBuildSchedulerArgs<'_>) -> Result<(), String> {
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
    use std::collections::{HashMap, HashSet};
    use std::sync::mpsc;

    let n = entries.len();
    let cp_to_idx: HashMap<(String, String), usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| ((e.category.clone(), e.package.clone()), i))
        .collect();

    // idx -> the indices (present in this graph) it depends on,
    // reconstructed from the reverse `required_by` edges.
    let mut deps: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (d_idx, d) in entries.iter().enumerate() {
        for r_cp in &d.required_by {
            if let Some(&r_idx) = cp_to_idx.get(r_cp) {
                deps[r_idx].insert(d_idx);
            }
        }
    }

    // Entries that need no build count as already merged, so their
    // dependents become dispatchable immediately.
    let mut merged: HashSet<usize> = (0..n)
        .filter(|&i| !scheduler_needs_build(&entries[i]))
        .collect();
    let mut started: HashSet<usize> = HashSet::new();
    let mut skip: HashSet<usize> = HashSet::new();
    let mut ledger = Ledger::default();
    let mut unreported: Vec<String> = Vec::new();
    let mut gone: HashSet<usize> = HashSet::new();
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
    let progress = merge_progress_map(entries);

    let loop_result = std::thread::scope(|scope| -> Result<(), String> {
        let (tx, rx) = mpsc::channel::<(usize, Result<PathBuf, String>)>();
        let mut in_flight = 0usize;
        loop {
            // `max_jobs` caps concurrency, then `should_start` gates one more
            // build on the live load average. The first build is never
            // gated, so the DAG cannot deadlock.
            while in_flight < policy.max_jobs() {
                if !policy.should_start(in_flight, system_loadavg_1min()) {
                    break;
                }
                let next = (0..n).find(|&i| {
                    scheduler_needs_build(&entries[i])
                        && !started.contains(&i)
                        && !skip.contains(&i)
                        && !merged.contains(&i)
                        && deps[i].iter().all(|d| merged.contains(d))
                });
                let Some(idx) = next else { break };
                started.insert(idx);
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
                        build_one_source_entry(
                            BuildOneSourceEntryArgs { entry, repos, root, portage_tmpdir, options, buildpkg: bp, capture_log: // Each build's output goes to its own
                        // `${T}/build.log` so parallel builds don't
                        // interleave on the terminal.
                        true, progress: entry_progress, display },
                        )
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

            let failure = match build_result {
                Ok(path) => {
                    // The build slot is free before the serialized merge
                    // starts.
                    display.build_finished();
                    let entry = &entries[idx];
                    let entry_progress = progress[idx];
                    let entry_root = entry_merge_root(entry, root);
                    let err = merge_one_built_entry(MergeOneBuiltEntryArgs {
                        entry,
                        repos,
                        ebuild_path: &path,
                        root: &entry_root,
                        portage_tmpdir,
                        options,
                        progress: entry_progress,
                        display,
                    })
                    .err();
                    if err.is_some() {
                        // The build slot is already free, so only the
                        // `failed` event fires here.
                        display.job_failed();
                    }
                    err
                }
                Err(e) => {
                    display.job_failed();
                    display.build_finished();
                    Some(e)
                }
            };

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
                Some(e) => {
                    if !ledger.failed(&report, &entries[idx], &e) {
                        unreported.push(e);
                    }
                    if !keep_going {
                        // Without `--keep-going` no new work is scheduled and
                        // still-running builds are killed, not waited out.
                        ebuild_phases::kill_registered_children(&registry);
                        return Ok(());
                    }
                    gone.insert(idx);
                    let mut skipped = Vec::new();
                    scheduler_skip_dependents(idx, entries, &cp_to_idx, &mut skip, &mut skipped);
                }
            }
        }
        Ok(())
    });
    loop_result?;

    // The resumed pass: whatever never ran because a dependency failed is
    // dropped with a notice.
    // Portage: Scheduler.py, _merge (resume loop), _calc_resume_list
    if keep_going && !skip.is_empty() {
        ledger.resuming(report.quiet);
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
    ledger.finish(&report, keep_going, first)
}

/// The failure a worker thread that panicked reports in place of its result.
/// Without it the scheduler would wait forever on a result nobody sends (its
/// own `tx` clone keeps the channel open), so a panic became a silent hang
/// (backlog #376).
pub(super) fn worker_panic_message(
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
