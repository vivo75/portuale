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

/// Real `_emerge/EbuildBuild` (+ `EbuildBinpkg`): the `install` phase
/// chain and any `--buildpkg` binpkg for one source entry, WITHOUT the
/// vdb merge. Returns the located ebuild path for `merge_one_built_entry`
/// to reuse. Safe to run from a scheduler worker thread -- every build
/// gets its own `${PORTAGE_BUILDDIR}` (`<tmpdir>/portage/<cat>/<pkg>-<ver>`)
/// and `run_commands` passes the environment explicitly (no
/// `std::env::set_var`). When `capture_log` is set, the `install` phase's
/// stdout+stderr go to `${T}/build.log` instead of the terminal (real
/// `PORTAGE_LOG_FILE`); on a build failure the tail of that log is folded
/// into the returned error so the scheduler can show it.
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
    // Real `MergeListItem._start`'s per-package line (backlog #177) --
    // the build half of the scheduler split prints it when the build
    // starts, exactly where real starts the `EbuildBuild` chain.
    let color = progress_color();
    display.status(&emerging_line(entry, &version, progress, root, &color));
    // Real per-package `PORTAGE_TMPDIR` (#99): this entry's build log,
    // pre-clean and phase chain all live under the matched value.
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
        // Real `prepare_build_dirs` truncates a stale build.log.
        // Truncate (don't delete): with `PORTAGE_LOGDIR` set, `lp` is
        // a symlink to the real log file, and deleting it would drop
        // the link and strand the new log in `${T}` instead of the
        // logdir. `set_len(0)` follows the link to the real file.
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(lp) {
            let _ = f.set_len(0);
        }
    }
    // A captured parallel build runs through real `bash` (not the
    // embedded `brush`), whose stdout+stderr redirect to `build.log`
    // cleanly at the OS level -- real portage uses real bash for builds
    // regardless of portuale's default backend.
    let shell = if log_path.is_some() {
        ebuild_phases::ShellBackend::Bash
    } else {
        options.shell
    };
    let build_env = entry_build_env(options, entry, repos);
    // Real `_emerge/EbuildBuild._start_pre_clean`: the `clean` phase runs
    // before every build, unconditionally (`noclean` only skips the
    // post-merge clean) -- see `merge_one_source_entry`'s own call for
    // the full grounding. Backlog #42.
    let clean_status = ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        build_env: &build_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell,
        log_file: log_path.as_deref(),
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
        log_file: log_path.as_deref(),
        build_env: &build_env,
    })?;
    if status != 0 {
        let mut msg = format!("{cp}-{version}: build failed ({status})");
        if let Some(lp) = &log_path {
            msg.push_str(&format!(
                "\n----- last lines of {} -----\n{}\n----------",
                lp.display(),
                tail_of(lp, 40)
            ));
        }
        return Err(msg);
    }
    if let Some(package_options) = buildpkg {
        println!(">>> Building package for {cp}-{version}...");
        // Real `Package.use.enabled` is what real `_pkgindex_entry`
        // writes as this binpkg's own `USE` field -- the same resolved
        // flag set the `install` phase itself just ran with, not the
        // ebuild's own default (`use_flags_display`-derived `USE` env
        // var `entry_build_env` already resolved for that phase, reused
        // verbatim here rather than re-derived, so the two can never
        // drift apart).
        let use_flags = build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // Real per-package `FEATURES` (backlog #130): the shared,
        // run-wide `PackageOptions` `buildpkg` carries doesn't see this
        // entry's own `package.env` `FEATURES` fold otherwise -- clone it
        // and re-derive the binpkg-affecting tokens
        // (`binpkg-multi-instance`/`buildpkg-live`/`binpkg-signing`) from
        // the same per-entry folded list `entry_resolved_features`
        // already computes for the phase env. `None` (no match) keeps
        // `package_options`'s own already-resolved value.
        let mut per_entry_package_options = package_options.clone();
        let per_entry_features = entry_resolved_features(
            options,
            entry,
            &std::env::var("FEATURES").unwrap_or_default(),
        );
        if let Some(features) = &per_entry_features {
            per_entry_package_options.set_resolved_features(features);
        }
        // Real `EbuildBinpkg._start`'s per-package `BUILD_ID` gate
        // (backlog #147 S1 ruling (i)): the layout stays run-wide,
        // the export follows this entry's own token -- the folded
        // list when one matched, the run-wide value otherwise.
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

/// Real `_emerge/EbuildMerge` (always serialized -- only the build above
/// runs in parallel): the vdb merge for a source entry whose `install`
/// phase already ran. Reuses `run_qmerge` (which checks the same real
/// `${PORTAGE_BUILDDIR}/.installed` marker `install` leaves behind and
/// runs `merge_after_install`, including the same-slot replace of an
/// upgraded/reinstalled version).
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
    // `merge_after_install`'s `pkg_preinst`/`pkg_postinst` see this
    // entry's resolved `USE` too (see `merge_one_source_entry`).
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry, repos);
    if let Some((protect, mask)) = reloaded_config_protect(options) {
        per_entry.config_protect = protect;
        per_entry.config_protect_mask = mask;
    }
    // Real per-package `FEATURES` (#98): the merge half's gates
    // (`feature_enabled`, the vdb-environment regeneration) read the
    // per-entry folded list, not the run-wide one. `set_resolved_features`
    // re-derives the merge-time tokens from it; `None` (no match) keeps
    // the run-wide value untouched.
    if let Some(features) = entry_resolved_features(
        options,
        entry,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        per_entry.set_resolved_features(&features);
    }
    // Real per-package `PORTAGE_TMPDIR` (#99): the merge half runs under
    // the same resolved root the build half used (resolved again here so
    // this function keeps its standalone `portage_tmpdir` contract).
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    // This function is only ever reached once `build_one_source_entry`
    // already captured the same package's own `install` phase to this
    // exact path (both callers only route here when `capture_log` is
    // true) -- reusing it here (`MergeOptions::log_file`'s own doc
    // comment) means `pkg_preinst`/`pkg_postinst`'s own output lands
    // appended after `install`'s, in one continuous per-package log,
    // instead of leaking straight to the terminal the way it
    // previously did under `-jN`/`--quiet-build`.
    per_entry.log_file = Some(build_log_path(
        portage_tmpdir,
        &entry.category,
        &entry.package,
        &version,
        &resolved_features(options),
    ));
    // Real `PackageMerge._start`'s per-package line (backlog #177) --
    // the merge half of the scheduler split prints it right before the
    // serialized vdb merge, exactly where real starts `EbuildMerge`.
    let color = progress_color();
    display.status(&installing_line(entry, &version, progress, root, &color));
    let status = ebuild_merge::run_qmerge(ebuild_path, root, portage_tmpdir, &per_entry)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
    }
    // Real `dblink.merge()`'s tail (`dbapi/vartree.py:6183-6198`): the
    // `clean` phase runs after a successful merge unless
    // `FEATURES=noclean` (the postinst-failure gate collapses into
    // `status != 0` above). `run_qmerge` itself deliberately does *not*
    // clean -- real `doebuild qmerge` implies noclean (see its own doc
    // comment) -- so this is where the `emerge` scheduler/captured
    // build+merge split gets the real post-merge behavior. Backlog #42.
    if !ebuild_merge::feature_enabled(&per_entry, "noclean") {
        ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
            ebuild_path,
            root,
            portage_tmpdir,
            build_env: &per_entry.build_env,
            debug: per_entry.debug,
            config_root: &per_entry.config_root,
            shell: per_entry.shell,
            log_file: per_entry.log_file.as_deref(),
        })?;
    }
    // Real `PackageMerge._install_exit`'s per-package line (backlog
    // #177): this is the `Completed (N of M) cpv::repo` real prints
    // where portuale used to print its own `merged.` line.
    display.status(&completed_line(entry, &version, progress, root, &color));
    Ok(())
}

/// Marks every (transitive) dependent of `idx` as skipped -- real
/// `Scheduler._calc_resume_list` after a failed build under
/// `--keep-going`.
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

/// Real `_emerge/Scheduler.py`'s core: run up to `jobs` package *builds*
/// (`install` phase) concurrently, dispatching a build only once every
/// dependency it has in the merge set is already merged, and serializing
/// the vdb merge step (real portage merges one package at a time to avoid
/// `collision-protect` / `CONTENTS` races). `jobs == usize::MAX` (a bare
/// `--jobs`/`-j`) means "as many as the graph allows".
///
/// Real portage's `--load-average` gate: the current system 1-minute load
/// average (Linux `/proc/loadavg`), or `0.0` if it can't be read (a
/// non-Linux host, or a sandboxed `/proc`) -- which disables the throttle
/// rather than stalling. Shared with `regen.rs`'s own `--jobs` dispatch
/// (real `PollScheduler._can_add_job` gates `MetadataRegen` the same way).
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
    /// The `-jN` dispatch policy (real `_emerge/Scheduler.py`'s jobs +
    /// `--load-average` gate): the director's `SchedulerPolicy` trait,
    /// so a capped (`LoadAwarePolicy`) or uncapped (`UnlimitedPolicy`)
    /// scheduler is one caller-side value, never a loop change here.
    pub(crate) policy: &'a dyn mrg_director::SchedulerPolicy,
    /// Backlog #197: the run's status display (the `>>> Jobs:` events
    /// below plus the blank/`>>>` rule inside the build/merge halves).
    pub(crate) display: &'a StatusDisplay,
}

/// KNOWN, DOCUMENTED CUTS (same "narrow v1" pattern as the rest of this
/// module): each build's own phase output is inherited straight to the
/// terminal and so interleaves under `-j` >1 (real portage captures
/// per-package build logs -- a later slice); each `run_commands` call
/// still spins up its own tokio runtime; a non-`--keep-going` failure
/// returns immediately but still waits for already-running builds to
/// finish (`thread::scope` join), it does not kill them.
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

    // idx -> the indices (present in this graph) it depends on. `d`'s
    // `required_by` lists the cps that depend on `d`, so the edge is
    // `d -> r` for every `r` in `d.required_by`.
    let mut deps: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    for (d_idx, d) in entries.iter().enumerate() {
        for r_cp in &d.required_by {
            if let Some(&r_idx) = cp_to_idx.get(r_cp) {
                deps[r_idx].insert(d_idx);
            }
        }
    }

    // Entries that need no build (AlreadyInstalled / Binary) count as
    // already merged, so their dependents become dispatchable immediately.
    // (Backlog #197: the `>>> Jobs:` `maxval` lives on the run's
    // `StatusDisplay`, computed by the caller -- see `run_source_merge`.)
    let mut merged: HashSet<usize> = (0..n)
        .filter(|&i| !scheduler_needs_build(&entries[i]))
        .collect();
    let mut started: HashSet<usize> = HashSet::new();
    let mut skip: HashSet<usize> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    // One registry for this whole scheduler call, shared only with the
    // worker threads *it* spawns below -- see `ebuild_phases::
    // SCHEDULER_REGISTRY`'s own doc comment for why this must not be a
    // single process-wide singleton (an unrelated `cargo test` thread's
    // subprocess must never be reachable from here).
    let registry = ebuild_phases::new_scheduler_registry();
    // Real `Scheduler._pkg_count` for this run (backlog #177): every
    // build/merge half below prints its entry's own snapshot.
    let progress = merge_progress_map(entries);

    std::thread::scope(|scope| -> Result<(), String> {
        let (tx, rx) = mpsc::channel::<(usize, Result<PathBuf, String>)>();
        let mut in_flight = 0usize;
        loop {
            // The policy's two halves, asked in the order the trait
            // contract names: `max_jobs` caps concurrency, then
            // `should_start` gates one more build on the live 1-minute
            // load average -- real `Scheduler._run` never gates the
            // first build, so the DAG cannot deadlock. Identical
            // decisions to the old inline `in_flight < jobs` +
            // load-average gate for `LoadAwarePolicy`.
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
                // Backlog #197: real `_schedule_tasks_imp`'s `running =
                // self._jobs` -- the `>>> Jobs: 0 of N complete, R
                // running` line prints at dispatch, *before* the worker
                // prints its `>>> Emerging` line (real's event order).
                display.job_started();
                // Real `Scheduler._pkg_count` (backlog #177): the
                // entry's own positional snapshot -- the worker
                // closure below is `move` and cannot borrow the map.
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
                scope.spawn(move || {
                    // Registers every real subprocess this worker
                    // thread spawns into `registry` for the closure's
                    // own lifetime -- see `ebuild_phases::
                    // SCHEDULER_REGISTRY`'s own doc comment.
                    let _guard = ebuild_phases::scope_scheduler_registry(registry);
                    let r = build_one_source_entry(
                        BuildOneSourceEntryArgs { entry, repos, root, portage_tmpdir, options, buildpkg: bp, capture_log: // Capture each build's phase output to its own
                        // `${T}/build.log` so parallel builds don't
                        // interleave on the terminal (real portage's
                        // `--quiet-build`, on by default under `--jobs`).
                        true, progress: entry_progress, display },
                    );
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
                    // Backlog #197: real `_build_exit`'s tail -- the
                    // build slot is free (`running` drops) before the
                    // serialized merge starts.
                    display.build_finished();
                    let entry = &entries[idx];
                    let entry_progress = progress[idx];
                    let err = merge_one_built_entry(MergeOneBuiltEntryArgs {
                        entry,
                        repos,
                        ebuild_path: &path,
                        root,
                        portage_tmpdir,
                        options,
                        progress: entry_progress,
                        display,
                    })
                    .err();
                    if err.is_some() {
                        // Backlog #197: real `_merge_exit` /
                        // `_do_merge_exit`'s failure arm
                        // (`Scheduler.py:1543-1561`): `failed` rises
                        // (its `>>> Jobs:` line prints). The build slot
                        // is already free from the tail above, so no
                        // `running` event fires here -- and the `Some`
                        // arm below fires none either, so each failure
                        // prints exactly real's lines.
                        display.job_failed();
                    }
                    err
                }
                Err(e) => {
                    // Backlog #197: real `_build_exit`'s failure arm
                    // (`Scheduler.py:1641-1658`) -- `failed` rises
                    // before the freed build slot (`running` drops)
                    // prints. (Real also prints its `>>> Failed to
                    // emerge ...` tail between the two; portuale's
                    // source-build failure rendering is unchanged by
                    // this slice.)
                    display.job_failed();
                    display.build_finished();
                    Some(e)
                }
            };

            match failure {
                None => {
                    merged.insert(idx);
                    // Real `Scheduler.py:1595-1602`, same as the serial
                    // loop: each successful merge leaves the saved resume
                    // list (order-free removal by value, so parallel
                    // completion order is fine).
                    if let Some(cpv) = resume_cpv(&entries[idx]) {
                        crate::mtimedb::remove_merged_entry(root, &cpv);
                    }
                    // Backlog #197: real `_merge_exit`'s `curval += 1`.
                    display.merge_finished();
                }
                Some(e) => {
                    // Backlog #197: no display events here -- the
                    // build-failure arm above already fired `job_failed`
                    // + `build_finished`, and the merge-failure arm
                    // already fired `job_failed`. Each failure prints
                    // exactly real's lines (failed-set, then the
                    // running-drop where real drops it).
                    if !keep_going {
                        // Real `_keep_scheduling`/`_terminate_tasks`
                        // (`PollScheduler.py:106-126`): once any package
                        // fails without `--keep-going`, real portage
                        // stops scheduling new work AND sends kill
                        // signals to what's already running, rather than
                        // letting it finish. `return Err(e)` here already
                        // does the first half (the outer `loop` never
                        // starts another build), but without this,
                        // `std::thread::scope`'s own implicit join would
                        // otherwise just wait for every other in-flight
                        // build to run to completion anyway -- wasted
                        // work whose result is discarded regardless.
                        ebuild_phases::kill_registered_children(&registry);
                        return Err(e);
                    }
                    failures.push(e);
                    scheduler_skip_dependents(idx, entries, &cp_to_idx, &mut skip, &mut skipped);
                }
            }
        }
        Ok(())
    })?;

    if failures.is_empty() {
        return Ok(());
    }
    let mut msg = format!(
        "{} package(s) failed to merge (--keep-going):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|f| format!("  {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if !skipped.is_empty() {
        msg.push_str(&format!(
            "\n{} dependent package(s) not merged:\n{}",
            skipped.len(),
            skipped
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Err(msg)
}
