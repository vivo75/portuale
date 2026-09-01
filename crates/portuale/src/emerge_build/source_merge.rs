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
    // One `JobStatusDisplay` per merge run; `maxval` is the build-bound
    // count here.
    let total_builds = entries.iter().filter(|e| scheduler_needs_build(e)).count();
    let display = StatusDisplay::new(mode, total_builds, progress_color());
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
    let engine = crate::merge_engines::SourceEngine {
        repos,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        buildpkg_exclude,
        display: &display,
    };
    // Every entry below prints its own positional snapshot of these
    // counters.
    let progress = merge_progress_map(entries);
    run_merge_loop(entries, keep_going, root, |idx, entry| {
        let entry_progress = progress[idx];
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
/// first failure. With `--keep-going` records the failure, drops every
/// entry that (transitively) depends on the failed one via `required_by`,
/// and merges the rest, then returns a combined `Err`.
// Portage: Scheduler.py, _calc_resume_list
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
            // Every (transitive) dependent of the failed package can no
            // longer be merged.
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
            // Each successful merge removes its own package from the
            // up-front-saved resume list, so a SIGKILL leaves exactly the
            // tail for `--resume`.
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
