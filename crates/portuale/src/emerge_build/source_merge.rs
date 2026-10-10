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
    /// Real `Scheduler._background_mode`: redirect each package's build
    /// output to `${T}/build.log` instead of the terminal. Always true for
    /// `jobs` >1 (a `-j` run must not interleave); otherwise driven by
    /// `--quiet-build=y` / `-q`. When set, the single-job path runs the
    /// same captured-build-then-serialized-merge split the scheduler uses.
    pub capture_log: bool,
    /// Backlog #197: the scheduler-display mode (background + Jobs
    /// visibility), computed once by the caller from the CLI flags and
    /// the mergelist length. A serial captured run (`capture_log`) is
    /// always background (see `scheduler_status_mode`); the mode only
    /// carries the display half, never the capture decision itself.
    pub mode: StatusMode,
}

/// Real `emerge <atom>` with no `--pretend` and no `--buildpkgonly`/
/// `--getbinpkgonly`: portuale's first source build-and-merge path for
/// `emerge` itself. Iterates the resolved entries (already in real
/// dependency-first merge order, so every dependency merges before its
/// dependents), and for each `New` **source** entry runs the full real
/// `install` phase chain plus the vdb merge -- `ebuild_merge::run_merge`
/// (`pretend`→`setup`→…→`install` via embedded `brush` + real
/// `SRC_URI` fetch, then `merge_tree` + `pkg_preinst`/`pkg_postinst` +
/// `env_update()`). `AlreadyInstalled` entries are skipped.
///
/// An `Upgrade`/`Downgrade`/`Reinstall` is handled too: `run_merge`
/// merges the new version, then `ebuild_merge::unmerge_replaced_same_slot`
/// (inside `merge_after_install`) unmerges the replaced same-slot
/// version -- real `dblink.treewalk()`'s own merge-then-unmerge order,
/// with that version's own `pkg_prerm`/`pkg_postrm` run from its saved
/// vdb environment.
///
/// A `Binary` entry (only reachable with `--usepkg` without
/// `--getbinpkg`) is a hard error here -- pass `--getbinpkg` for the
/// mixed path (`emerge_getbinpkg::run_merge_plan`). Failure handling
/// (stop at the first, or `--keep-going` -> drop the failed package's
/// dependents and continue) is `run_merge_loop`'s.
pub fn run_source_merge(args: RunSourceMergeArgs<'_>) -> Result<(), String> {
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
    // Backlog #197: real `Scheduler` owns one `JobStatusDisplay` per
    // merge run (`maxval` is the build-bound count here -- see
    // `StatusDisplay`'s own doc comment for the installed-noop
    // narrowing).
    let total_builds = entries.iter().filter(|e| scheduler_needs_build(e)).count();
    let display = StatusDisplay::new(mode, total_builds, progress_color());
    if jobs > 1 {
        // The `-jN` dispatch policy, one of the director's two
        // `SchedulerPolicy` implementations: a bare `-j` (mapped to
        // `usize::MAX` by the CLI layer) runs uncapped (real
        // `max_jobs is True`), a numbered `--jobs=N` runs capped --
        // both keep the `--load-average` gate for additional builds.
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
    // The serial loop merges through the director's source engine (the
    // same `SourceEngine::merge_entry` the mixed dispatcher runs per
    // source unit) rather than calling `merge_one_source_entry`
    // directly, so the merge-engine slot carries the production
    // source-merge traffic. The captured-build split stays inline: it is
    // scheduler machinery (build-half concurrency), not per-unit
    // execution.
    let engine = crate::merge_engines::SourceEngine {
        repos,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        buildpkg_exclude,
        display: &display,
    };
    // Real `Scheduler._pkg_count` for this run (backlog #177): every
    // entry below prints its own positional snapshot of these counters.
    let progress = merge_progress_map(entries);
    run_merge_loop(entries, keep_going, root, |idx, entry| {
        let entry_progress = progress[idx];
        let bp = buildpkg.filter(|opts| {
            entry_buildpkg_wanted(entry, repos, buildpkg_exclude, opts.buildpkg_live)
        });
        if capture_log && scheduler_needs_build(entry) {
            // Backlog #197: the captured serial split is background by
            // construction (see `scheduler_status_mode`), so these are
            // real's serial-background `>>> Jobs:` events: start,
            // build-end, merge-land around the same halves the `-jN`
            // scheduler runs -- and, on failure, real `_build_exit` /
            // `_merge_exit`'s failure arms (`Scheduler.py:1641-1658` /
            // `:1543-1561`), each fired exactly once like the `-jN`
            // scheduler above.
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
/// `emerge_getbinpkg::run_merge_plan`. Without `keep_going` it stops at
/// the first failure (`merge_one`'s own `Err`), portuale's long-
/// standing default. With real `--keep-going` (real `Scheduler`'s own
/// `_calc_resume_list`) it records the failure, drops every entry that
/// (transitively) depends on the failed one via the `GraphEntry`'s
/// reverse-dependency edges (`required_by`), and merges the rest --
/// then returns a combined `Err` naming what failed and what was
/// skipped (real `emerge` also exits non-zero when anything failed
/// under `--keep-going`).
pub(crate) fn run_merge_loop<F>(
    entries: &[GraphEntry],
    keep_going: bool,
    root: &Path,
    mut merge_one: F,
) -> Result<(), String>
where
    F: FnMut(usize, &GraphEntry) -> Result<(), String>,
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

    let mut skip: HashSet<(String, String)> = HashSet::new();
    let mut failures: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let cp = (entry.category.clone(), entry.package.clone());
        if skip.contains(&cp) {
            skipped.push(format!("{}/{}", entry.category, entry.package));
            continue;
        }
        if let Err(e) = merge_one(idx, entry) {
            if !keep_going {
                return Err(e);
            }
            failures.push(e);
            // Real `_calc_resume_list`: every (transitive) dependent of
            // the failed package can no longer be merged.
            let mut queue = vec![cp];
            while let Some(x) = queue.pop() {
                if let Some(deps) = dependents.get(&x) {
                    for p in deps {
                        if skip.insert(p.clone()) {
                            queue.push(p.clone());
                        }
                    }
                }
            }
        } else if let Some(cpv) = resume_cpv(entry) {
            // Real `Scheduler.py:1595-1602`: each successful merge removes
            // its own package from the up-front-saved resume list (and
            // commits), so a SIGKILL leaves exactly the tail for
            // `--resume`. A no-op "merge" (already installed) was never
            // saved, so its own removal is a silent no-op.
            crate::mtimedb::remove_merged_entry(root, &cpv);
        }
    }

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

/// One entry of `run_source_merge`'s own loop -- also the `Source`-entry
/// arm of `emerge_getbinpkg::run_merge_plan` (`emerge --getbinpkg`'s
/// mixed source+binary merge). `AlreadyInstalled` is a silent no-op; a
/// `Binary` entry is a hard error here (the mixed dispatcher routes
/// those to `merge_binpkg` before ever calling this).
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
        // #72 B3: a blocker-removal task is not a merge. Executing it is
        // a documented non-goal (`LLM/02.072-uninstall_merge_rows.md`
        // §7); the entry is skipped exactly like an installed no-op.
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

    // Real `MergeListItem._start`'s per-package line (backlog #177).
    let color = progress_color();
    display.status(&emerging_line(entry, &version, progress, root, &color));
    if buildpkg.is_some() {
        println!(">>> Building package for {cp}-{version}...");
    }
    // The resolved `USE` for this entry, so `bin/ebuild.sh`'s own `use()`
    // (and every USE-conditional in the ebuild's phases) sees the real
    // flags -- `phase_env_vars` otherwise leaves `USE=""`. Narrowing: the
    // IUSE-declared enabled flags only (`GraphEntry::use_flags_display`),
    // not the implicit/arch part of the effective set.
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry, repos);
    if let Some((protect, mask)) = reloaded_config_protect(options) {
        per_entry.config_protect = protect;
        per_entry.config_protect_mask = mask;
    }
    // Real per-package `FEATURES` (#98): this entry's gates (Rust-side
    // `feature_enabled` and the vdb-environment regeneration) read the
    // per-entry folded list. `None` (no match) keeps the run-wide value.
    if let Some(features) = entry_resolved_features(
        options,
        entry,
        &std::env::var("FEATURES").unwrap_or_default(),
    ) {
        per_entry.set_resolved_features(&features);
    }
    // Real per-package `PORTAGE_TMPDIR` (#99): this entry's build
    // directories live under the matched value, so the pre-clean and
    // the merge below share the resolved root.
    let entry_tmpdir = entry_portage_tmpdir(options, entry, &version, portage_tmpdir)?;
    let portage_tmpdir = entry_tmpdir.as_path();
    // Real `_emerge/EbuildBuild._start_pre_clean` (`EbuildBuild.py:207-
    // 229`) and `Scheduler.py:969-981`/`:1119-1139`: the `clean` phase
    // runs before every build, unconditionally (`noclean` only skips the
    // *post*-merge clean). Without it a stale `.installed` marker or an
    // already-`instprep`ped image from an earlier build in the same
    // `${PORTAGE_BUILDDIR}` silently skips `install` (backlog #42, found
    // in #38 S4).
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
    // Real `PackageMerge._start`'s per-package line (backlog #177),
    // printed from `run_merge`'s pre-merge hook: real wraps the finished
    // `EbuildBuild` in a `PackageMerge` (`Scheduler._build_exit`,
    // `Scheduler.py:1615-1621`), so `Installing` lands after the build
    // phase output (and the `--buildpkg` packaging) and before the
    // `EbuildMerge` vdb merge -- the same point the `-jN` scheduler's
    // build/merge split prints it (before the serialized `run_qmerge`).
    // A failed build never reaches the hook, so no `Installing` prints
    // for it, exactly like real queuing no merge for a failed build.
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
    // Real `PackageMerge._install_exit`'s per-package line (backlog
    // #177): this is the `Completed (N of M) cpv::repo` real prints
    // where portuale used to print its own `merged.` line.
    display.status(&completed_line(entry, &version, progress, root, &color));
    Ok(())
}
