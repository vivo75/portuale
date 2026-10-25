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

/// Portage's `emerge <atom>` with no `--pretend` and no `--buildpkgonly` /
/// `--getbinpkgonly`: for each `New` **source** entry runs the full `install`
/// phase chain plus the vdb merge. Entries are already in dependency-first
/// merge order. `AlreadyInstalled` entries are skipped; `Upgrade` /
/// `Downgrade` / `Reinstall` merge the new version and unmerge the replaced
/// same-slot version.
///
/// A `Binary` entry is a hard error here (the mixed dispatcher routes those
/// to `merge_binpkg`). Failure handling is `run_merge_loop`'s.
pub fn run_source_merge(args: RunSourceMergeArgs<'_>) -> Result<MergePassReport, MergePassError> {
    let RunSourceMergeArgs {
        entries,
        repos,
        root,
        portage_tmpdir,
        options,
        keep_going,
        buildpkg,
        buildpkg_exclude,
        jobs,
        load_average,
        capture_log,
        mode,
    } = args;
    // One `JobStatusDisplay` per merge run; `maxval` is the build-bound
    // count here.
    let total_builds = entries.iter().filter(|e| scheduler_needs_build(e)).count();
    let display = StatusDisplay::new(mode, total_builds, progress_color());
    // `^C` / `SIGTERM` stop the phases and exit 128+N, like Portage.
    let _exit_on_signal =
        crate::ebuild_phases::ExitOnSignalGuard::install(|code| std::process::exit(code));
    if jobs > 1 {
        // A bare `-j` (mapped to `usize::MAX` by the CLI layer) runs
        // uncapped; a numbered `--jobs=N` runs capped. Both keep the
        // `--load-average` gate.
        if jobs == usize::MAX {
            let policy = mrg_director::UnlimitedPolicy::new(load_average);
            return run_build_scheduler(RunBuildSchedulerArgs {
                entries,
                repos,
                root,
                portage_tmpdir,
                options,
                keep_going,
                buildpkg,
                buildpkg_exclude,
                policy: &policy,
                display: &display,
            });
        }
        let policy = mrg_director::LoadAwarePolicy::new(jobs, load_average);
        return run_build_scheduler(RunBuildSchedulerArgs {
            entries,
            repos,
            root,
            portage_tmpdir,
            options,
            keep_going,
            buildpkg,
            buildpkg_exclude,
            policy: &policy,
            display: &display,
        });
    }
    // The serial loop merges through the director's source engine rather
    // than calling `merge_one_source_entry` directly, so the merge-engine
    // slot carries the production source-merge traffic. The captured-build
    // split stays inline: scheduler machinery, not per-unit execution.
    let source_paths = |entry: &GraphEntry| source_paths_for(options, entry, portage_tmpdir);
    let report = FailReport {
        root,
        source_paths: &source_paths,
        features: resolved_features(options),
        quiet: options.quiet,
        background: mode.background,
        quiet_fail: options.quiet_fail,
    };
    // The up-front `pkg_pretend` pass runs even at `-j1`, one entry at a
    // time with each block replayed before the next starts, before any
    // build -- like the `-jN` scheduler above. A failure aborts before
    // anything builds, exactly like the scheduler.
    // Portage: _emerge/Scheduler.py, _run_pkg_pretend
    {
        let failures = {
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
                jobs,
                replay: &mut replay,
            })
        };
        if finish_failed_pretend(FinishFailedPretendArgs {
            report: &report,
            entries,
            failures,
            keep_going,
        })? {
            return Ok(MergePassReport::default());
        }
    }
    run_merge_loop(entries, keep_going, &report, |_, entry, entry_progress| {
        // An `IDEPEND` dep merges into the running root, like Portage.
        let entry_root = entry_merge_root(entry, root);
        let root = entry_root.as_path();
        let engine = crate::merge_engines::SourceEngine {
            repos,
            root,
            portage_tmpdir,
            options,
            buildpkg,
            buildpkg_exclude,
            display: &display,
            // The up-front pass above already replayed `pretend`.
            skip_pretend: true,
        };
        let bp = buildpkg.filter(|opts| {
            entry_buildpkg_wanted(entry, repos, buildpkg_exclude, opts.buildpkg_live)
        });
        if capture_log && scheduler_needs_build(entry) {
            // The captured serial split is background by construction, so
            // these are the serial-background `>>> Jobs:` events around
            // the same halves the `-jN` scheduler runs.
            display.job_started();
            let path = match build_one_source_entry(BuildOneSourceEntryArgs {
                entry,
                repos,
                root,
                portage_tmpdir,
                options,
                buildpkg: bp,
                capture_log: true,
                progress: entry_progress,
                display: &display,
                // The up-front pass above already replayed `pretend`
                // (backlog #545): the build starts at `setup`.
                skip_pretend: true,
            }) {
                Ok(path) => {
                    display.build_finished();
                    path
                }
                Err(e) => {
                    display.job_failed();
                    display.build_finished();
                    return Err(e);
                }
            };
            if let Err(e) = merge_one_built_entry(MergeOneBuiltEntryArgs {
                entry,
                repos,
                ebuild_path: &path,
                root,
                portage_tmpdir,
                options,
                progress: entry_progress,
                display: &display,
            }) {
                display.job_failed();
                return Err(e);
            }
            display.merge_finished();
            Ok(())
        } else {
            engine.merge_entry(entry, entry_progress)
        }
    })
}

/// The shared per-entry loop for `run_source_merge` /
/// `emerge_getbinpkg::run_merge_plan`. Without `keep_going` stops at the
/// first failure. With `--keep-going` records the failure, re-plans like
/// Portage's resume pass (the entries that (transitively) require the failed
/// one are dropped with a notice, the progress counter restarts over what is
/// left) and merges the rest. Every failure is reported through `report`.
/// A postinst failure needs no re-plan: its merge landed, so dependents
/// stay satisfiable and simply continue (still under a restarted counter,
/// like Portage's resumed pass).
// Portage: Scheduler.py, _merge (resume loop), _calc_resume_list
pub(crate) fn run_merge_loop<F>(
    entries: &[GraphEntry],
    keep_going: bool,
    report: &FailReport<'_>,
    mut merge_one: F,
) -> Result<MergePassReport, MergePassError>
where
    F: FnMut(usize, &GraphEntry, mrg_director::MergeProgress) -> Result<(), String>,
{
    use std::collections::{HashMap, HashSet};

    // cp -> the cps that depend on it (each entry's own `required_by`).
    let dependents: HashMap<(String, String), Vec<(String, String)>> = entries
        .iter()
        .map(|e| {
            (
                (e.category.clone(), e.package.clone()),
                e.required_by.clone(),
            )
        })
        .collect();
    let cp_of = |e: &GraphEntry| (e.category.clone(), e.package.clone());

    let mut ledger = Ledger::default();
    let mut unreported: Vec<String> = Vec::new();
    let mut pass_report = MergePassReport::default();
    // cps that failed or were dropped, and the dropped ones already announced.
    let mut gone: HashSet<(String, String)> = HashSet::new();
    let mut pending: Vec<usize> = (0..entries.len()).collect();
    let mut progress = merge_progress_map(entries);
    let mut pos = 0;
    while pos < pending.len() {
        let idx = pending[pos];
        let entry = &entries[idx];
        let Err(e) = merge_one(idx, entry, progress[idx]) else {
            if let Some(cpv) = resume_cpv(entry) {
                // Each successful merge removes its own package from the
                // up-front-saved resume list, so a SIGKILL leaves exactly the
                // tail for `--resume`.
                crate::mtimedb::remove_merged_entry(report.root, &cpv);
            }
            pos += 1;
            continue;
        };
        let postinst = is_postinst_failure(&e);
        if postinst {
            // The merge landed (Portage removes it from the in-memory
            // mergelist but drops no dependents: their requirement is
            // satisfied by the live vdb entry).
            // Portage: Scheduler.py, _do_merge_exit / _calc_resume_list
            pass_report.postinst_failed.push(cp_of(entry));
        }
        if !ledger.failed(report, entry, &e) {
            unreported.push(e);
        }
        if !keep_going {
            break;
        }
        // Every (transitive) dependent of a failed package can no longer be
        // merged -- except after a postinst failure, whose merge landed, so
        // the dependents stay satisfiable and simply continue.
        let mut dropped_cps: HashSet<(String, String)> = HashSet::new();
        if !postinst {
            let mut queue = vec![cp_of(entry)];
            gone.insert(cp_of(entry));
            while let Some(x) = queue.pop() {
                for p in dependents.get(&x).into_iter().flatten() {
                    if gone.insert(p.clone()) {
                        dropped_cps.insert(p.clone());
                        queue.push(p.clone());
                    }
                }
            }
        }
        let rest: Vec<usize> = pending[pos + 1..].to_vec();
        if rest.is_empty() {
            break;
        }
        ledger.resuming(report.quiet);
        pass_report.resumed = true;
        let mut survivors = Vec::new();
        for i in rest {
            let e = &entries[i];
            if dropped_cps.contains(&cp_of(e)) {
                let atoms: Vec<String> = {
                    let mut atoms: Vec<String> =
                        gone.iter().flat_map(|cp| atoms_requiring(e, cp)).collect();
                    atoms.sort();
                    atoms.dedup();
                    atoms
                };
                ledger.dropped(report, e, &atoms);
            } else if !gone.contains(&cp_of(e)) {
                survivors.push(i);
            }
        }
        // The counter restarts over what is left.
        let remaining: Vec<GraphEntry> = survivors.iter().map(|&i| entries[i].clone()).collect();
        for (k, m) in merge_progress_map(&remaining).into_iter().enumerate() {
            progress[survivors[k]] = m;
        }
        resave_resume_list(report.root, &survivors, entries);
        pending = survivors;
        pos = 0;
    }
    let first = (!unreported.is_empty()).then(|| unreported.join("\n"));
    match ledger.finish(report, keep_going, first) {
        Ok(()) => Ok(pass_report),
        Err(message) => Err(MergePassError {
            message,
            report: pass_report,
        }),
    }
}

/// Re-save the resume list for a resumed `--keep-going` pass: the failed
/// entry is gone (attempted), the survivors stay replayable. Reads the
/// up-front save's favorites and options back so only the mergelist is
/// replaced. A pass with nothing left keeps the per-merge shrink's tail.
// Portage: Scheduler.py, _merge (resume loop), _save_resume_list
pub(crate) fn resave_resume_list(root: &Path, survivors: &[usize], entries: &[GraphEntry]) {
    if survivors.is_empty() {
        return;
    }
    let Some((favorites, _, opts)) = crate::mtimedb::read_resume_list(root) else {
        return;
    };
    let mergelist: Vec<crate::mtimedb::ResumeCpv> = survivors
        .iter()
        .filter_map(|&i| resume_cpv(&entries[i]))
        .collect();
    let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
    let _ = crate::mtimedb::write_resume_list(root, &fav_refs, &mergelist, &opts);
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
    /// The up-front `pkg_pretend` pass already ran it (serial `-j1` now
    /// runs the same pass as the `-jN` scheduler): start at `setup`.
    /// Portage: _emerge/Scheduler.py, _run_pkg_pretend
    pub(crate) skip_pretend: bool,
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
        skip_pretend,
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
        log_file: ebuild_phases::PhaseLog::capture(per_entry.log_file.as_deref()),
    })?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    // `Installing` prints from `run_merge`'s pre-merge hook, after the build
    // phase output and before the vdb merge; a failed build never reaches
    // the hook, so no `Installing` prints for it.
    let installing = installing_line(entry, &version, progress, root, &color);
    let print_installing = || display.status(&installing);
    let outcome = ebuild_merge::run_merge_with_hook(ebuild_merge::RunMergeWithHookArgs {
        ebuild_path: &path,
        root,
        portage_tmpdir,
        options: &per_entry,
        buildpkg,
        before_merge: Some(&print_installing as &dyn Fn()),
        skip_pretend,
    })?;
    if outcome.postinst_failed {
        // The merge landed (files and vdb entry are live): `>>> Completed`
        // prints like `PackageMerge._install_exit`, and the world record
        // below is skipped for this entry. The clean above was already
        // skipped by the merge itself (the `_postinst_failure` builddir
        // rule); the error carries the endgame wording.
        // Portage: _emerge/PackageMerge.py, _install_exit; vartree.py
        display.status(&completed_line(entry, &version, progress, root, &color));
        return Err(postinst_failed_error(&cp, &version, outcome.status));
    }
    if outcome.status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({})", outcome.status));
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
