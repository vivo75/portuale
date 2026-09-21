// Real `emerge --buildpkgonly` execution, WITHOUT `--pretend`: actually
// builds a binary package for every entry pretend.rs's own dry-run gate
// already proved is safe to build. `GraphResult::buildpkgonly_deps_unsatisfied`
// being `false` (the gate `pretend.rs` already checks before calling
// anything here) means no needs-building entry's own `required_by` set
// includes another needs-building entry -- i.e. none of them depend on
// each other at all, real `--buildpkgonly`'s whole point (every real
// dependency must already be satisfied by something already installed) --
// so there is no cross-entry build ordering to compute here, unlike a
// real merge's own topological sort.
//
// Reuses `ebuild_package::run_package` (task #105-#109) as-is: "package"
// IS the real, unmodified `doebuild()` action `--buildpkgonly` itself is
// built on (see `resolve_pretend_graph`'s own doc comment -- real
// `--buildpkgonly` is a resolution-time depgraph check, not a distinct
// execution mode of its own). `run_package`'s own `install` chain now
// really fetches a nonempty `SRC_URI` too (see `ebuild_phases::
// fetch_sources`/`crate::fetch`'s own module doc comments) -- this
// module used to refuse any entry with a real `SRC_URI` outright (no
// fetch machinery existed yet); that refusal is gone now that fetching
// is real, and a fetch/digest failure simply surfaces as an ordinary
// `run_package` error like any other build failure would.
//
// `GraphEntry` doesn't carry the winning candidate's own repo location
// (see its own doc comment -- deliberately not threaded through the
// whole graph-resolution/Python-mirror pair, which has no real-execution
// need for it at all). `locate_candidate` re-derives it via
// `portage_repo::list_candidates`, the same repo/version lookup
// `resolve_pretend_graph` already did internally to pick this entry's
// winning version in the first place.
//
// KNOWN, DOCUMENTED GAPS (same "narrow v1, document the cut" pattern as
// every other real-execution slice in portuale):
//   - A `CandidateSource::Binary` entry (would only appear via
//     `--usepkg`) is skipped outright -- it's already a binary, there is
//     nothing to build.
//   - Builds run strictly in `entries` order, which `resolve_pretend_graph`
//     now returns in real portage's dependency-first *merge* order
//     (`topological_merge_order`) -- so even if the `--buildpkgonly` gate
//     above were relaxed, a dep would still build before its dependent.
//   - A build failure aborts immediately, unless real `--keep-going` is
//     given (now real, see `run_buildpkgonly`'s own doc comment) -- no
//     cleanup of any already-built packages either way, portuale's
//     own single-invocation-at-a-time CLI usage never needs partial-
//     build cleanup.

use crate::ebuild_merge;
use crate::ebuild_package::{self, PackageOptions};
use crate::ebuild_phases;
use portage_repo::{Candidate, CandidateSource, GraphEntry, PretendOutcome, RepoConfig};
use std::path::{Path, PathBuf};

/// The version this entry would actually build at, or `None` for an
/// outcome real `--buildpkgonly` never builds anything for
/// (`AlreadyInstalled`/`NoVisibleCandidate` -- the latter can't reach
/// here at all, since it aborts the whole resolution before any
/// `GraphEntry` exists for it).
fn entry_version(outcome: &PretendOutcome) -> Option<&str> {
    match outcome {
        PretendOutcome::New { version } => Some(version),
        PretendOutcome::Upgrade { to, .. } => Some(to),
        PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::Reinstall { version, .. } => Some(version),
        // #72 B3: a removal builds nothing.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// Re-finds the winning candidate for `category/package` at exactly
/// `version` -- the same repo/version lookup `resolve_pretend_graph`
/// already did internally to pick this entry's winning version in the
/// first place, just not retained on `GraphEntry` (see the module doc
/// comment). When more than one repo has this exact version (a real, if
/// rare, overlay-shadows-main-repo case), prefers the highest-priority
/// repo, the same tie-break `resolve_pretend`'s own candidate selection
/// already uses elsewhere in this crate.
fn locate_candidate(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> Option<Candidate> {
    let candidates = portage_repo::list_candidates(repos, category, package).ok()?;
    candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
        .cloned()
}

/// Real doebuild()'s own `<repo>/<category>/<package>/<package>-<version>.ebuild`
/// path convention.
fn ebuild_path(candidate: &Candidate, category: &str, package: &str, version: &str) -> PathBuf {
    candidate
        .repo_location
        .join(category)
        .join(package)
        .join(format!("{package}-{version}.ebuild"))
}

/// Actually builds a binary package (never merges) for every entry in
/// `entries` that real `--buildpkgonly` would build -- see the module
/// doc comment for the full scope. Without `keep_going`, returns the
/// *first* failure encountered (message already includes which package
/// failed) and stops there, matching portuale's own long-established
/// default. With real `--keep-going` (real `main.py`'s own `y_or_n`
/// option, narrowed by portuale's own CLI transcription to the bare/
/// `y` form -- see `pretend.rs`'s own `keep_going` doc comment), every
/// entry is still attempted regardless of earlier failures -- safe here
/// specifically because the gate `pretend.rs` already checks before
/// calling this at all (`GraphResult::buildpkgonly_deps_unsatisfied`)
/// guarantees no entry depends on another, so unlike real portage's own
/// general `--keep-going` (which must also skip every *dependent* of a
/// failed package, tracked via real `Scheduler.py`'s own mergelist
/// recalculation), there is nothing here that a failure could ever
/// invalidate for a later entry. Failures are collected and returned
/// together at the end as a single combined error listing every one --
/// `Ok(())` only once every entry has a real binary package on disk.
pub fn run_buildpkgonly(
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
    keep_going: bool,
) -> Result<(), String> {
    // The run-wide half of real `config.environ()`, once for the whole
    // run (`--buildpkgonly` has no `MergeOptions`; `entry_phase_env_tail`
    // adds the per-entry half). #37 S2.
    let run_wide = run_wide_phase_env(config);
    let mut failures = Vec::new();
    for entry in entries {
        if entry.source == CandidateSource::Binary {
            continue;
        }
        let Some(version) = entry_version(&entry.outcome) else {
            continue;
        };
        let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, version)
        else {
            let failure = format!(
                "{}/{}-{version}: could not locate its own ebuild file \
                 (repo layout changed since resolution?)",
                entry.category, entry.package
            );
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        };
        let path = ebuild_path(&candidate, &entry.category, &entry.package, version);
        println!(
            ">>> Building binary for {}/{}-{version}...",
            entry.category, entry.package
        );
        // Real `config.environ()` per entry: the run-wide base plus this
        // entry's resolved `USE`/`IUSE_EFFECTIVE`/`USE_EXPAND` and
        // `SLOT`/repo identity (#37 S2). The `install` chain and the
        // `install_qa_check` misc-functions call after it see the same
        // env the `-b` merge path does, and `package_after_install` gets
        // the real `USE` for the `Packages` index / `metadata/USE`.
        let mut build_env = run_wide.clone();
        build_env.extend(entry_phase_env_tail(
            Some(config),
            repos,
            entry,
            Some(&candidate),
        ));
        let use_flags = build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // Real `_emerge/EbuildBuild._start_pre_clean` before the build
        // and `_buildpkgonly_success_hook_exit` (`EbuildBuild.py:525-535`)
        // after the package: `--buildpkgonly` pre-cleans like every other
        // build and post-cleans unconditionally -- the phase itself, not
        // a `noclean` gate, is what honors `keeptemp`/`keepwork` (real's
        // `_clean_exit` treats a failed clean as a failed build).
        // Backlog #42.
        let clean_failure = |phase: &str| -> Option<String> {
            match ebuild_phases::run_clean(
                &path,
                root,
                portage_tmpdir,
                &build_env,
                options.debug,
                &options.config_root,
                options.shell,
                None,
            ) {
                Ok(0) => None,
                Ok(status) => Some(format!(
                    "{}/{}-{version}: {phase} clean failed ({status})",
                    entry.category, entry.package
                )),
                Err(e) => Some(format!(
                    "{}/{}-{version}: {phase} clean failed: {e}",
                    entry.category, entry.package
                )),
            }
        };
        let failure = match clean_failure("pre") {
            Some(failure) => Some(failure),
            None => match ebuild_package::run_package(
                &path,
                root,
                portage_tmpdir,
                options,
                &build_env,
                use_flags,
            ) {
                Ok(0) => clean_failure("post"),
                Ok(_) => Some(format!(
                    "{}/{}-{version}: build failed",
                    entry.category, entry.package
                )),
                Err(e) => Some(format!(
                    "{}/{}-{version}: {e}",
                    entry.category, entry.package
                )),
            },
        };
        if let Some(failure) = failure {
            if keep_going {
                failures.push(failure);
                continue;
            }
            return Err(failure);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} package(s) failed to build (--keep-going):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
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
#[allow(clippy::too_many_arguments)]
pub fn run_source_merge(
    entries: &[GraphEntry],
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
    keep_going: bool,
    buildpkg: Option<&ebuild_package::PackageOptions>,
    buildpkg_exclude: &[String],
    jobs: usize,
    load_average: Option<f64>,
    // Real `Scheduler._background_mode`: redirect each package's build
    // output to `${T}/build.log` instead of the terminal. Always true for
    // `jobs` >1 (a `-j` run must not interleave); otherwise driven by
    // `--quiet-build=y` / `-q`. When set, the single-job path runs the
    // same captured-build-then-serialized-merge split the scheduler uses.
    capture_log: bool,
) -> Result<(), String> {
    if jobs > 1 {
        // The `-jN` dispatch policy, one of the director's two
        // `SchedulerPolicy` implementations: a bare `-j` (mapped to
        // `usize::MAX` by the CLI layer) runs uncapped (real
        // `max_jobs is True`), a numbered `--jobs=N` runs capped --
        // both keep the `--load-average` gate for additional builds.
        if jobs == usize::MAX {
            let policy = mrg_director::UnlimitedPolicy::new(load_average);
            return run_build_scheduler(
                entries,
                repos,
                root,
                portage_tmpdir,
                options,
                keep_going,
                buildpkg,
                buildpkg_exclude,
                &policy,
            );
        }
        let policy = mrg_director::LoadAwarePolicy::new(jobs, load_average);
        return run_build_scheduler(
            entries,
            repos,
            root,
            portage_tmpdir,
            options,
            keep_going,
            buildpkg,
            buildpkg_exclude,
            &policy,
        );
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
    };
    run_merge_loop(entries, keep_going, |entry| {
        let bp = buildpkg.filter(|opts| {
            entry_buildpkg_wanted(entry, repos, buildpkg_exclude, opts.buildpkg_live)
        });
        if capture_log && scheduler_needs_build(entry) {
            let path =
                build_one_source_entry(entry, repos, root, portage_tmpdir, options, bp, true)?;
            merge_one_built_entry(entry, repos, &path, root, portage_tmpdir, options)
        } else {
            engine.merge_entry(entry)
        }
    })
}

/// Real `--buildpkg-exclude`'s own `InternalPackageSet.findAtomForPackage`
/// check: does `entry`'s resolved cpv (+ slot) match any of `atoms`
/// (each an ordinary package atom)?
pub(crate) fn entry_matches_any(entry: &GraphEntry, atoms: &[String]) -> bool {
    if atoms.is_empty() {
        return false;
    }
    let Some(version) = entry_version(&entry.outcome) else {
        return false;
    };
    let slot = entry.slot.as_deref().unwrap_or("0");
    let sub_slot = entry.sub_slot.as_deref().unwrap_or(slot);
    let cpv_slot = format!(
        "{}/{}-{version}:{slot}/{sub_slot}",
        entry.category, entry.package
    );
    atoms.iter().any(|atom| {
        portage_dep::match_from_list(atom, &[cpv_slot.as_str()]).is_some_and(|m| !m.is_empty())
    })
}

/// Real `Package.binpkg_wanted`'s own `"live" not in self.properties`
/// half (`_emerge/Package.py:621-637`): this candidate's own evaluated
/// `PROPERTIES` (real USE-conditional-reduced against the *build*, not
/// the resolve-time, USE set -- `entry.use_flags_display`'s own enabled
/// subset is the same set `entry_build_env`'s `USE=` export already
/// uses) contains the bare token `live`. `PROPERTIES` has no `||`-group
/// semantics (same reasoning `portage_repo::evaluated_metadata_tokens`'
/// own doc comment gives), so a flat `use_reduce` is faithful.
fn entry_is_live(candidate: &Candidate, entry: &GraphEntry) -> bool {
    if candidate.properties.trim().is_empty() {
        return false;
    }
    let use_flags: std::collections::HashSet<String> = entry
        .use_flags_display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.clone())
        .collect();
    let tokens: Vec<String> = candidate
        .properties
        .split_whitespace()
        .map(String::from)
        .collect();
    portage_use_reduce::use_reduce_flat(&tokens, &use_flags, portage_use_reduce::MatchMode::Normal)
        .map(|flat| flat.iter().any(|t| t == "live"))
        .unwrap_or(false)
}

/// Real `Package.binpkg_wanted(exclude)` (`_emerge/Package.py:621-637`),
/// narrowed to the `buildpkg` (not `buildsyspkg`) half portuale's own
/// `--buildpkg`/`FEATURES=buildpkg` already models: `--buildpkg-exclude`
/// (`entry_matches_any`) always wins outright; otherwise a
/// `PROPERTIES=live` build is skipped unless `FEATURES=buildpkg-live`
/// (real default -- `buildpkg_live` -- is on). A candidate this can't
/// even locate falls through to `true` (not live, by construction) --
/// the real build path a moment later raises its own clear "could not
/// locate its own ebuild file" error instead of this filter silently
/// swallowing it.
pub(crate) fn entry_buildpkg_wanted(
    entry: &GraphEntry,
    repos: &[RepoConfig],
    buildpkg_exclude: &[String],
    buildpkg_live: bool,
) -> bool {
    if entry_matches_any(entry, buildpkg_exclude) {
        return false;
    }
    if buildpkg_live {
        return true;
    }
    let Some(version) = entry_version(&entry.outcome) else {
        return true;
    };
    match locate_candidate(repos, &entry.category, &entry.package, version) {
        Some(candidate) => !entry_is_live(&candidate, entry),
        None => true,
    }
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
    mut merge_one: F,
) -> Result<(), String>
where
    F: FnMut(&GraphEntry) -> Result<(), String>,
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

    for entry in entries {
        let cp = (entry.category.clone(), entry.package.clone());
        if skip.contains(&cp) {
            skipped.push(format!("{}/{}", entry.category, entry.package));
            continue;
        }
        if let Err(e) = merge_one(entry) {
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

/// One entry of `run_source_merge`'s own loop -- also the `Source`-entry
/// arm of `emerge_getbinpkg::run_merge_plan` (`emerge --getbinpkg`'s
/// mixed source+binary merge). `AlreadyInstalled` is a silent no-op; a
/// `Binary` entry is a hard error here (the mixed dispatcher routes
/// those to `merge_binpkg` before ever calling this).
pub(crate) fn merge_one_source_entry(
    entry: &GraphEntry,
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
    buildpkg: Option<&ebuild_package::PackageOptions>,
) -> Result<(), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        // #72 B3: a blocker-removal task is not a merge. Executing it is
        // a documented non-goal (`docs/02.072-uninstall_merge_rows.md`
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

    println!(">>> Emerging ({cp}-{version})...");
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
    // Real `_emerge/EbuildBuild._start_pre_clean` (`EbuildBuild.py:207-
    // 229`) and `Scheduler.py:969-981`/`:1119-1139`: the `clean` phase
    // runs before every build, unconditionally (`noclean` only skips the
    // *post*-merge clean). Without it a stale `.installed` marker or an
    // already-`instprep`ped image from an earlier build in the same
    // `${PORTAGE_BUILDDIR}` silently skips `install` (backlog #42, found
    // in #38 S4).
    let clean_status = ebuild_phases::run_clean(
        &path,
        root,
        portage_tmpdir,
        &per_entry.build_env,
        per_entry.debug,
        &per_entry.config_root,
        per_entry.shell,
        per_entry.log_file.as_deref(),
    )?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    let status = ebuild_merge::run_merge(&path, root, portage_tmpdir, &per_entry, buildpkg)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
    }
    println!(">>> {cp}-{version} merged.");
    Ok(())
}

/// `[("USE", "<space-joined enabled IUSE flags>")]` for `entry` -- the
/// build-phase env every `emerge <atom>` source build/merge passes so
/// `bin/ebuild.sh`'s `use()` sees the resolved flags. Empty vec (no
/// `USE` entry) when the entry declares no enabled flags, so
/// `phase_env_vars`' own `USE=""` stands. `GraphEntry::use_flags_display`
/// is already the package's IUSE, enabled-resolved and bare-name-sorted.
fn build_use_env(entry: &GraphEntry) -> Vec<(String, String)> {
    let enabled: Vec<&str> = entry
        .use_flags_display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    if enabled.is_empty() {
        Vec::new()
    } else {
        vec![("USE".to_string(), enabled.join(" "))]
    }
}

/// The per-package `package.env` build vars that match `entry`'s cpv --
/// real `_grab_pkg_env` folding a matching `/etc/portage/package.env`
/// entry's env file into `configdict["pkg"]`, with real's acceptance set
/// (`match_package_env_vars`). Incrementals fold onto `options.build_env`
/// (the run-wide resolved env) instead of replacing it, matching
/// `regenerate()`'s layer stacking.
fn entry_package_env_vars(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
) -> Vec<(String, String)> {
    if options.package_env_vars.is_empty() {
        return Vec::new();
    }
    let Some(version) = entry_version(&entry.outcome) else {
        return Vec::new();
    };
    let slot = entry.slot.as_deref().unwrap_or("0");
    let sub_slot = entry.sub_slot.as_deref().unwrap_or(slot);
    let cpv_slot = format!(
        "{}/{}-{version}:{slot}/{sub_slot}",
        entry.category, entry.package
    );
    let profile_only_variables = options
        .resolved_config
        .as_deref()
        .and_then(|config| config.resolved_incremental("PROFILE_ONLY_VARIABLES"))
        .unwrap_or_default();
    crate::ebuild_phases::match_package_env_vars(
        &options.package_env_vars,
        &cpv_slot,
        &profile_only_variables,
        &options.build_env,
        &portage_profile::config_env_all(),
    )
}

/// `SLOT`, `PORTAGE_REPO_NAME`, `PORTAGE_REPO_REVISIONS` for `entry` --
/// the per-entry identity real `doebuild_environment()` sets
/// (`doebuild.py:483`) and `EbuildPhase._setup_repo_revisions` builds
/// (`EbuildPhase.py:73-108`). `SLOT` is the ebuild's own `SLOT`
/// (`slot/sub_slot` when a sub-slot is declared, bare slot otherwise,
/// matching the ebuild global real `config.environ()` carries), never
/// empty (`entry.slot` unset -> `"0"`; the `.keep_<cp>-` bug is exactly
/// an empty slot). `PORTAGE_REPO_REVISIONS` is `"{}"` until portuale
/// tracks a repo revision (real `json.dumps({}, sort_keys=True)`).
/// `PORTAGE_REPO_NAME` is omitted only when no repo is known at all.
fn entry_identity_env(entry: &GraphEntry, candidate: Option<&Candidate>) -> Vec<(String, String)> {
    let slot = entry
        .slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.slot.as_str()))
        .unwrap_or("0");
    let sub_slot = entry
        .sub_slot
        .as_deref()
        .or_else(|| candidate.map(|c| c.sub_slot.as_str()))
        .unwrap_or(slot);
    let slot_value = if sub_slot.is_empty() || sub_slot == slot {
        slot.to_string()
    } else {
        format!("{slot}/{sub_slot}")
    };
    let repo_name = entry
        .repo_name
        .as_deref()
        .or_else(|| candidate.map(|c| c.repo_name.as_str()))
        .unwrap_or("");
    let mut env = vec![
        ("SLOT".to_string(), slot_value),
        ("PORTAGE_REPO_REVISIONS".to_string(), "{}".to_string()),
    ];
    if !repo_name.is_empty() {
        env.push(("PORTAGE_REPO_NAME".to_string(), repo_name.to_string()));
    }
    env
}

/// The run-wide half of real `config.environ()` for a build:
/// `portage_profile::phase_environ(config, None)` plus the dynamic
/// `doebuild_environment()` value that needs portuale's compressor table,
/// `PORTAGE_COMPRESSION_COMMAND` (`doebuild.py:697-750`, set for every
/// build regardless of `BINPKG_FORMAT`). `BINPKG_COMPRESS*` are
/// `environ_filter`ed, so they are read from the calling env / config
/// directly; `MAKEOPTS` from the (already-defaulted) export set.
pub(crate) fn run_wide_phase_env(config: &portage_profile::Config) -> Vec<(String, String)> {
    let mut env = portage_profile::phase_environ(config, None);
    let lookup = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var(key).ok())
            .or_else(|| config.other_vars.get(key).cloned())
    };
    if let Some(cmd) = ebuild_package::phase_compression_command(lookup) {
        env.push(("PORTAGE_COMPRESSION_COMMAND".to_string(), cmd));
    }
    env
}

/// The package metadata keys real `config.setcpv` loads into
/// `configdict["pkg"]` (`_setcpv_aux_keys`, `config.py:171-189`) that
/// survive `environ_filter` and stay exported in the saved environment:
/// `bin/ebuild.sh` unsets `IUSE`/`REQUIRED_USE`/`PROPERTIES`/`RESTRICT`/
/// `INHERITED`/`EAPI` before sourcing the ebuild (`:634-635`), so only
/// these three keep real's `declare -x`; `SLOT`/`EAPI`/`INHERITED` are
/// computed elsewhere. Values from the md5-cache (real `aux_get`; an
/// omitted key is `""`).
const EXPORTED_PKG_METADATA: [&str; 3] = ["DEFINED_PHASES", "KEYWORDS", "LICENSE"];

fn entry_metadata_env(entry: &GraphEntry, candidate: &Candidate) -> Vec<(String, String)> {
    let pf = format!("{}-{}", entry.package, candidate.version);
    // H4: the repo-metadata decision point (#41's `repo_aux_metadata`:
    // repo md5-cache -> depcachedir -> depend phase) is reached through
    // the `RepoCache` slot on this production path, the same
    // interchangeable-traits seam the scheduler/merge/news/fetch/
    // packages/binpkg slots use. `Md5Cache::metadata` delegates to the
    // identical function the direct call used, so this is
    // behaviour-neutral.
    let cache = mrg_director::Md5Cache::new(&candidate.repo_location, &candidate.repo_name);
    let Ok(metadata) = mrg_director::RepoCache::metadata(&cache, &entry.category, &pf) else {
        return Vec::new();
    };
    EXPORTED_PKG_METADATA
        .iter()
        .map(|k| (k.to_string(), metadata.get(*k).cloned().unwrap_or_default()))
        .collect()
}

/// The per-entry tail of [`entry_build_env`]: resolved `USE` (or the
/// legacy enabled-IUSE-only `USE` when no config is in scope), the
/// `IUSE_EFFECTIVE`/`USE_EXPAND` rows, and the `SLOT`/repo identity.
/// Shared with `run_buildpkgonly`, whose `PackageOptions` carries no
/// `MergeOptions`.
///
/// With `config` set (`emerge <atom>` and friends, #37 S2) the `USE` is
/// real `PORTAGE_USE`: the resolver's full effective set
/// (`candidate_effective_use_flags`, which includes the implicit profile
/// flags no package declares in `IUSE`) narrowed by `portage_use` to
/// `IUSE ∪ IUSE_EFFECTIVE`. Without a config (standalone `ebuild <file>
/// merge`/`qmerge`, tests) the previous enabled-IUSE-only `USE` stands.
fn entry_phase_env_tail(
    config: Option<&portage_profile::Config>,
    repos: &[RepoConfig],
    entry: &GraphEntry,
    candidate: Option<&Candidate>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let Some(config) = config else {
        env.extend(build_use_env(entry));
        return env;
    };
    if let Some(candidate) = candidate {
        env.extend(entry_metadata_env(entry, candidate));
        let enabled = portage_repo::candidate_effective_use_flags(
            repos,
            config,
            &entry.category,
            &entry.package,
            &candidate.version,
        );
        let iuse: Vec<String> = candidate
            .iuse
            .split_whitespace()
            .map(String::from)
            .collect();
        env.extend(portage_profile::phase_environ_pkg(
            config,
            Some(portage_profile::PhaseUse {
                iuse: &iuse,
                enabled: &enabled,
            }),
        ));
    }
    env.extend(entry_identity_env(entry, candidate));
    env
}

/// The full per-entry build-phase env: the run-wide resolved env the
/// caller stashed on `options.build_env` (`portage_profile::
/// phase_environ(config, None)`), then any per-package `package.env`
/// build vars on top of those, then [`entry_phase_env_tail`].
fn entry_build_env(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
    repos: &[RepoConfig],
) -> Vec<(String, String)> {
    let candidate = entry_version(&entry.outcome)
        .and_then(|version| locate_candidate(repos, &entry.category, &entry.package, version));
    let mut env = options.build_env.clone();
    env.extend(entry_package_env_vars(options, entry));
    env.extend(entry_phase_env_tail(
        options.resolved_config.as_deref(),
        repos,
        entry,
        candidate.as_ref(),
    ));
    env
}

/// `(cat/pkg, version)` for an entry the scheduler will build -- the
/// same outcome→version mapping `merge_one_source_entry` does inline.
fn scheduler_cp_version(entry: &GraphEntry) -> Result<(String, String), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        // #72 B3: unreachable via `scheduler_needs_build` (a removal is
        // never scheduled); kept explicit so no wildcard hides it.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => {
            return Err(format!("{cp}: not a buildable entry"));
        }
    };
    Ok((cp, version))
}

/// Whether the scheduler must actually run a build for this entry (a
/// `New`/`Upgrade`/`Downgrade`/`Reinstall` from source). `AlreadyInstalled`
/// and `Binary` entries are treated as already satisfied.
fn scheduler_needs_build(entry: &GraphEntry) -> bool {
    entry.source != CandidateSource::Binary
        && matches!(
            entry.outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
                | PretendOutcome::Reinstall { .. }
        )
}

/// A minimal source `GraphEntry` for `emerge --resume` (`pretend.rs`):
/// the saved `mtimedb` resume list only records `cat/pkg-ver`, so the
/// display/USE/blocker fields are all empty. `required_by` is empty too --
/// the resume mergelist is already in dependency-first merge order, so
/// `run_merge_loop`'s `--keep-going` dependent-drop has nothing to key on
/// (and `--resume` doesn't pass `--keep-going` anyway). `source` is the
/// resumed mergelist entry's own recorded `ResumeEntryKind` (real's own
/// `type` tag): `CandidateSource::Binary` always resolves from the local
/// `$PKGDIR` (`remote_binary: false`) -- see `mtimedb.rs`'s own module
/// doc comment for why re-deriving "was this fetched remotely" isn't
/// attempted.
pub(crate) fn resume_entry(
    category: &str,
    package: &str,
    version: &str,
    source: CandidateSource,
) -> GraphEntry {
    GraphEntry {
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: None,
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

/// Real portage's `PORTAGE_LOG_FILE` (`PORTAGE_LOGDIR` unset →
/// `${T}/build.log`, i.e. `${PORTAGE_BUILDDIR}/temp/build.log`; with
/// `FEATURES=compress-build-logs`, `${T}/build.log.gz` --
/// `prepare_build_dirs.py`'s own `f"build.log{compress_log_ext}"`).
/// The resolved `FEATURES` list for build-path decisions: `MergeOptions::
/// features` when the caller resolved a config (#37 S2), else the raw
/// process env (standalone `ebuild <file>` / tests, where `features` is
/// empty). The same precedence `ebuild_phases::features_string` uses for
/// the phase-execution gates.
fn resolved_features(options: &ebuild_merge::MergeOptions) -> String {
    if options.features.is_empty() {
        std::env::var("FEATURES").unwrap_or_default()
    } else {
        options.features.clone()
    }
}

fn build_log_path(
    portage_tmpdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    features: &str,
) -> PathBuf {
    let builddir = portage_tmpdir
        .join("portage")
        .join(category)
        .join(format!("{package}-{version}"));
    // Real `PORTAGE_LOGDIR`/`PORTAGE_LOG_FILE_SEP`/`FEATURES=split-log`
    // -- the same "env var, not full config resolution" shortcut this
    // whole CLI boundary already uses elsewhere (`DISTDIR`/`FEATURES`
    // tokens/...); read once here, right where the log path itself is
    // decided, and handed to the pure logic in
    // `ensure_portage_logdir_symlink` so that function stays directly
    // unit-testable with no env-var involvement at all.
    let logdir = std::env::var_os("PORTAGE_LOGDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());
    let sep = std::env::var("PORTAGE_LOG_FILE_SEP").unwrap_or_else(|_| ":".to_string());
    let split_log = features.split_whitespace().any(|t| t == "split-log");
    // Real `compress_log_ext` (`prepare_build_dirs.py:397-399`): the
    // `.gz` suffix applies to BOTH the `${T}` path and the
    // `PORTAGE_LOG_FILE` below, so the symlink and its target agree.
    let compress = features
        .split_whitespace()
        .any(|t| t == "compress-build-logs");
    let path = builddir.join("temp").join(if compress {
        "build.log.gz"
    } else {
        "build.log"
    });
    ensure_portage_logdir_symlink(
        &path,
        &builddir,
        category,
        &format!("{package}-{version}"),
        logdir.as_deref(),
        &sep,
        split_log,
        compress,
    );
    path
}

/// Real `prepare_build_dirs()`'s own `PORTAGE_LOGDIR`/`FEATURES=
/// split-log` handling (`prepare_build_dirs.py:368-468`): when
/// `logdir` is given (`PORTAGE_LOGDIR` set, `build_log_path`'s own
/// doc comment), the real build log lives there permanently --
/// `<logdir>/<CATEGORY><sep><PF><sep><logid_time>.log`, or under
/// `split_log`, `<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log`
/// -- and `tmpdir_log_path` (`${T}/build.log`) becomes a symlink to it
/// rather than the real file. With `compress`
/// (`FEATURES=compress-build-logs`, `prepare_build_dirs.py:397-399`'s
/// own `compress_log_ext = ".gz"`), both names gain the `.gz` suffix
/// and phase output is gzip-encoded into the file
/// (`ebuild_phases::open_log_file`'s pump thread -- real
/// `EbuildPhase._open_log`'s `gzip.GzipFile(mode="ab")`). Everything
/// downstream that opens `tmpdir_log_path` still just opens the path
/// `build_log_path` returned, symlinks transparently followed by
/// `std::fs`, so this is the only place that needs to know about
/// `PORTAGE_LOGDIR` at all. A no-op when `logdir` is `None`
/// (`PORTAGE_LOGDIR` unset or empty -- real's own `if
/// mysettings.get("PORTAGE_LOGDIR", "") == "": del it`) or when it
/// can't be created (real's own "Permission issues... Disabling
/// logging").
///
/// `logid_time`: a `.logid` marker file's own mtime (real's own
/// `os.stat(logid_path).st_mtime`), created on first use and reused
/// afterward -- so every phase of the same build (each its own fresh
/// shell, `ebuild_phases::run_one_phase`'s own doc comment) and a
/// resumed one all share one timestamp, matching real exactly.
#[allow(clippy::too_many_arguments)]
fn ensure_portage_logdir_symlink(
    tmpdir_log_path: &Path,
    builddir: &Path,
    category: &str,
    pf: &str,
    logdir: Option<&Path>,
    sep: &str,
    split_log: bool,
    compress: bool,
) {
    let Some(logdir) = logdir else {
        return;
    };
    if std::fs::create_dir_all(logdir).is_err() {
        // Real: permission issues here disable logging for this build
        // entirely (`tmpdir_log_path` stays the real file).
        return;
    }

    let logid_path = builddir.join(".logid");
    let logid_time = std::fs::metadata(&logid_path)
        .and_then(|m| m.modified())
        .or_else(|_| {
            std::fs::create_dir_all(builddir)?;
            std::fs::write(&logid_path, [])?;
            std::fs::metadata(&logid_path)?.modified()
        })
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let stamp = crate::elog::utc_stamp_at(logid_time);

    // Real `compress_log_ext`: the `.gz` goes on the real log file
    // name itself (both `split-log` and flat layouts).
    let ext = if compress { ".log.gz" } else { ".log" };
    let (log_subdir, real_log) = if split_log {
        let subdir = logdir.join("build").join(category);
        let file = subdir.join(format!("{pf}{sep}{stamp}{ext}"));
        (subdir, file)
    } else {
        let file = logdir.join(format!("{category}{sep}{pf}{sep}{stamp}{ext}"));
        (logdir.to_path_buf(), file)
    };
    if std::fs::create_dir_all(&log_subdir).is_err() {
        return;
    }

    // Real's own idempotent re-symlink check (`make_new_symlink`):
    // skip if it already points at the right place.
    let needs_new = std::fs::read_link(tmpdir_log_path)
        .map(|target| target != real_log)
        .unwrap_or(true);
    if needs_new {
        if let Some(parent) = tmpdir_log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(tmpdir_log_path);
        let _ = std::os::unix::fs::symlink(&real_log, tmpdir_log_path);
    }
}

/// Last `n` lines of `path`, or a short "(build log unavailable)" note.
/// A `.gz` path (`FEATURES=compress-build-logs`) is gunzipped first --
/// real's own failure display (`Scheduler.py`) and QA scan
/// (`doebuild.py`) both wrap a `.gz`-suffixed log in
/// `gzip.GzipFile(mode="rb")` before reading.
fn tail_of(path: &Path, n: usize) -> String {
    let is_gz = path.extension().is_some_and(|ext| ext == "gz");
    let text: Option<String> = if is_gz {
        std::fs::File::open(path).ok().and_then(|f| {
            use std::io::Read;
            let mut decoder = flate2::read::MultiGzDecoder::new(f);
            let mut s = String::new();
            decoder.read_to_string(&mut s).ok().map(|_| s)
        })
    } else {
        std::fs::read_to_string(path).ok()
    };
    match text {
        Some(s) => {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        None => "(build log unavailable)".to_string(),
    }
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
fn build_one_source_entry(
    entry: &GraphEntry,
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
    buildpkg: Option<&ebuild_package::PackageOptions>,
    capture_log: bool,
) -> Result<PathBuf, String> {
    let (cp, version) = scheduler_cp_version(entry)?;
    let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, &version) else {
        return Err(format!(
            "{cp}-{version}: could not locate its own ebuild file \
             (repo layout changed since resolution?)"
        ));
    };
    let path = ebuild_path(&candidate, &entry.category, &entry.package, &version);
    println!(">>> Emerging ({cp}-{version})...");

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
    let clean_status = ebuild_phases::run_clean(
        &path,
        root,
        portage_tmpdir,
        &build_env,
        options.debug,
        &options.config_root,
        shell,
        log_path.as_deref(),
    )?;
    if clean_status != 0 {
        return Err(format!("{cp}-{version}: clean failed ({clean_status})"));
    }
    let status = ebuild_phases::run_commands_logged(
        &path,
        &["install"],
        root,
        portage_tmpdir,
        &options.distdir,
        options.debug,
        &options.config_root,
        shell,
        log_path.as_deref(),
        &build_env,
    )?;
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
        let status = ebuild_package::package_after_install(
            &path,
            root,
            portage_tmpdir,
            package_options,
            use_flags,
        )?;
        if status != 0 {
            return Err(format!("{cp}-{version}: binpkg build failed ({status})"));
        }
    }
    Ok(path)
}

/// Real `_emerge/EbuildMerge` (always serialized -- only the build above
/// runs in parallel): the vdb merge for a source entry whose `install`
/// phase already ran. Reuses `run_qmerge` (which checks the same real
/// `${PORTAGE_BUILDDIR}/.installed` marker `install` leaves behind and
/// runs `merge_after_install`, including the same-slot replace of an
/// upgraded/reinstalled version).
fn merge_one_built_entry(
    entry: &GraphEntry,
    repos: &[RepoConfig],
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
) -> Result<(), String> {
    let (cp, version) = scheduler_cp_version(entry)?;
    // `merge_after_install`'s `pkg_preinst`/`pkg_postinst` see this
    // entry's resolved `USE` too (see `merge_one_source_entry`).
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry, repos);
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
        ebuild_phases::run_clean(
            ebuild_path,
            root,
            portage_tmpdir,
            &per_entry.build_env,
            per_entry.debug,
            &per_entry.config_root,
            per_entry.shell,
            per_entry.log_file.as_deref(),
        )?;
    }
    println!(">>> {cp}-{version} merged.");
    Ok(())
}

/// Marks every (transitive) dependent of `idx` as skipped -- real
/// `Scheduler._calc_resume_list` after a failed build under
/// `--keep-going`.
fn scheduler_skip_dependents(
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

/// KNOWN, DOCUMENTED CUTS (same "narrow v1" pattern as the rest of this
/// module): each build's own phase output is inherited straight to the
/// terminal and so interleaves under `-j` >1 (real portage captures
/// per-package build logs -- a later slice); each `run_commands` call
/// still spins up its own tokio runtime; a non-`--keep-going` failure
/// returns immediately but still waits for already-running builds to
/// finish (`thread::scope` join), it does not kill them.
#[allow(clippy::too_many_arguments)]
fn run_build_scheduler(
    entries: &[GraphEntry],
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
    keep_going: bool,
    buildpkg: Option<&ebuild_package::PackageOptions>,
    buildpkg_exclude: &[String],
    // The `-jN` dispatch policy (real `_emerge/Scheduler.py`'s jobs +
    // `--load-average` gate): the director's `SchedulerPolicy` trait,
    // so a capped (`LoadAwarePolicy`) or uncapped (`UnlimitedPolicy`)
    // scheduler is one caller-side value, never a loop change here.
    policy: &dyn mrg_director::SchedulerPolicy,
) -> Result<(), String> {
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

    let total_builds = (0..n)
        .filter(|&i| scheduler_needs_build(&entries[i]))
        .count();

    // Entries that need no build (AlreadyInstalled / Binary) count as
    // already merged, so their dependents become dispatchable immediately.
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
                        entry,
                        repos,
                        root,
                        portage_tmpdir,
                        options,
                        bp,
                        // Capture each build's phase output to its own
                        // `${T}/build.log` so parallel builds don't
                        // interleave on the terminal (real portage's
                        // `--quiet-build`, on by default under `--jobs`).
                        true,
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
                Ok(path) => merge_one_built_entry(
                    &entries[idx],
                    repos,
                    &path,
                    root,
                    portage_tmpdir,
                    options,
                )
                .err(),
                Err(e) => Some(e),
            };

            match failure {
                None => {
                    merged.insert(idx);
                    // Real `_emerge/Scheduler.py`'s `JobStatusDisplay`.
                    let done = (0..n)
                        .filter(|&i| scheduler_needs_build(&entries[i]) && merged.contains(&i))
                        .count();
                    println!(">>> Jobs: {done} of {total_builds} complete");
                }
                Some(e) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::find_repos;
    use std::fs;

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "emerge_build_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_portage_logdir_symlink_is_a_noop_without_a_logdir() {
        let tmp = tempdir();
        let t_dir = tmp.join("t");
        fs::create_dir_all(&t_dir).unwrap();
        let log_path = t_dir.join("build.log");
        fs::write(&log_path, "already here").unwrap();
        ensure_portage_logdir_symlink(
            &log_path,
            &tmp.join("builddir"),
            "dev-libs",
            "foo-1.0",
            None,
            ":",
            false,
            false,
        );
        // Real file untouched -- no PORTAGE_LOGDIR means no symlink.
        assert_eq!(fs::read_to_string(&log_path).unwrap(), "already here");
        assert!(
            fs::symlink_metadata(&log_path)
                .unwrap()
                .file_type()
                .is_file()
        );
    }

    #[test]
    fn ensure_portage_logdir_symlink_points_t_build_log_at_the_real_logdir() {
        let tmp = tempdir();
        let builddir = tmp.join("builddir");
        let t_dir = builddir.join("temp");
        fs::create_dir_all(&t_dir).unwrap();
        let log_path = t_dir.join("build.log");
        let logdir = tmp.join("logdir");

        ensure_portage_logdir_symlink(
            &log_path,
            &builddir,
            "dev-libs",
            "foo-1.0",
            Some(&logdir),
            ":",
            false,
            false,
        );

        let target = fs::read_link(&log_path).expect("build.log must be a symlink");
        assert!(
            target.starts_with(&logdir),
            "{target:?} should live under {logdir:?}"
        );
        assert_eq!(target.parent().unwrap(), logdir);
        let name = target.file_name().unwrap().to_str().unwrap();
        // Real "<CATEGORY><sep><PF><sep><logid_time>.log".
        assert!(name.starts_with("dev-libs:foo-1.0:"), "{name}");
        assert!(name.ends_with(".log"), "{name}");
        // Writing through the symlink lands in the real logdir file.
        std::fs::write(&log_path, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");

        // Idempotent: calling again doesn't churn the symlink or drop
        // the timestamp (the .logid marker file makes it stable).
        let before = fs::read_link(&log_path).unwrap();
        ensure_portage_logdir_symlink(
            &log_path,
            &builddir,
            "dev-libs",
            "foo-1.0",
            Some(&logdir),
            ":",
            false,
            false,
        );
        assert_eq!(fs::read_link(&log_path).unwrap(), before);
    }

    #[test]
    fn ensure_portage_logdir_symlink_honors_split_log_and_the_separator() {
        let tmp = tempdir();
        let builddir = tmp.join("builddir");
        let t_dir = builddir.join("temp");
        fs::create_dir_all(&t_dir).unwrap();
        let log_path = t_dir.join("build.log");
        let logdir = tmp.join("logdir");

        ensure_portage_logdir_symlink(
            &log_path,
            &builddir,
            "dev-libs",
            "foo-1.0",
            Some(&logdir),
            "-",
            true, // split_log
            false,
        );

        let target = fs::read_link(&log_path).expect("build.log must be a symlink");
        // Real "<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log".
        assert_eq!(
            target.parent().unwrap(),
            logdir.join("build").join("dev-libs")
        );
        let name = target.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("foo-1.0-"), "{name}");
        assert!(!name.contains(':'), "{name} should use the '-' separator");
    }

    #[test]
    fn ensure_portage_logdir_symlink_gains_the_gz_suffix_when_compressed() {
        // Real `prepare_build_dirs.py:397-399` (`compress_log_ext`):
        // with `FEATURES=compress-build-logs` the real log file is
        // `<...>.log.gz` in both the flat and `split-log` layouts.
        for split_log in [false, true] {
            let tmp = tempdir();
            let builddir = tmp.join("builddir");
            let t_dir = builddir.join("temp");
            fs::create_dir_all(&t_dir).unwrap();
            let log_path = t_dir.join("build.log.gz");
            let logdir = tmp.join("logdir");

            ensure_portage_logdir_symlink(
                &log_path,
                &builddir,
                "dev-libs",
                "foo-1.0",
                Some(&logdir),
                ":",
                split_log,
                true, // compress
            );

            let target = fs::read_link(&log_path).expect("build.log.gz must be a symlink");
            let name = target.file_name().unwrap().to_str().unwrap();
            assert!(name.ends_with(".log.gz"), "{name}");
            if split_log {
                // Real `<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log.gz`.
                assert!(name.starts_with("foo-1.0:"), "{name}");
                assert_eq!(
                    target.parent().unwrap(),
                    logdir.join("build").join("dev-libs")
                );
            } else {
                // Real `<logdir>/<CATEGORY><sep><PF><sep><logid_time>.log.gz`.
                assert!(name.starts_with("dev-libs:foo-1.0:"), "{name}");
                assert_eq!(target.parent().unwrap(), logdir);
            }
        }
    }

    #[test]
    fn tail_of_gunzips_a_compressed_build_log() {
        // Real `Scheduler.py` / `doebuild.py` wrap a `.gz`-suffixed log
        // in `gzip.GzipFile(mode="rb")` before reading: the failure tail
        // must decode, not print binary.
        let tmp = tempdir();
        let log_path = tmp.join("build.log.gz");
        {
            let f = fs::File::create(&log_path).unwrap();
            let mut enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            use std::io::Write;
            enc.write_all(b"line1\nline2\nline3\nline4\n").unwrap();
            enc.finish().unwrap();
        }
        assert_eq!(tail_of(&log_path, 2), "line3\nline4");
        assert_eq!(
            tail_of(&tmp.join("missing.log.gz"), 2),
            "(build log unavailable)"
        );
    }

    #[test]
    fn locate_candidate_finds_the_real_fixture_ebuild_file() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let candidate = locate_candidate(&repos, "dev-libs", "packagepkg", "1.0").unwrap();
        let path = ebuild_path(&candidate, "dev-libs", "packagepkg", "1.0");
        assert!(path.ends_with("dev-libs/packagepkg/packagepkg-1.0.ebuild"));
        assert!(path.is_file(), "{path:?} should exist");
    }

    #[test]
    fn locate_candidate_is_none_for_a_version_that_does_not_exist() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        assert!(locate_candidate(&repos, "dev-libs", "packagepkg", "99.0").is_none());
    }

    fn live_test_entry() -> GraphEntry {
        GraphEntry {
            category: "dev-libs".into(),
            package: "propertiespkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: None,
            sub_slot: None,
            repo_name: None,
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    #[test]
    fn entry_is_live_reads_a_real_properties_live_fixture() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let live_candidate = locate_candidate(&repos, "dev-libs", "propertiespkg", "1.0").unwrap();
        assert!(entry_is_live(&live_candidate, &live_test_entry()));

        let non_live_candidate = locate_candidate(&repos, "dev-libs", "packagepkg", "1.0").unwrap();
        assert!(!entry_is_live(&non_live_candidate, &live_test_entry()));
    }

    #[test]
    fn entry_buildpkg_wanted_skips_a_live_build_only_when_buildpkg_live_is_off() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let entry = live_test_entry();

        // Real default (buildpkg-live on): a live build is still packaged.
        assert!(entry_buildpkg_wanted(&entry, &repos, &[], true));
        // FEATURES=-buildpkg-live: a live build is skipped...
        assert!(!entry_buildpkg_wanted(&entry, &repos, &[], false));

        // ...but a non-live package is unaffected either way.
        let mut non_live = entry.clone();
        non_live.package = "packagepkg".to_string();
        assert!(entry_buildpkg_wanted(&non_live, &repos, &[], true));
        assert!(entry_buildpkg_wanted(&non_live, &repos, &[], false));

        // --buildpkg-exclude still wins outright, regardless of buildpkg-live.
        assert!(!entry_buildpkg_wanted(
            &entry,
            &repos,
            &["dev-libs/propertiespkg".to_string()],
            true
        ));
    }

    #[test]
    fn run_buildpkgonly_skips_already_installed_and_no_visible_candidate() {
        // Neither outcome ever has a resolvable version (see
        // entry_version), so this must return Ok(()) without attempting
        // any real execution at all -- proven by using a nonexistent
        // ROOT/PORTAGE_TMPDIR/PackageOptions that would fail loudly if
        // touched.
        let entries = vec![
            GraphEntry {
                category: "dev-libs".into(),
                package: "samepkg".into(),
                outcome: PretendOutcome::AlreadyInstalled {
                    version: "1.0".into(),
                },
                blockers: vec![],
                slot: None,
                sub_slot: None,
                repo_name: None,
                oldbest: vec![],
                use_flags_display: vec![],
                use_expand_display: vec![],
                use_expand_display_p: vec![],
                keyword_mask: None,
                new_slot: false,
                interactive: false,
                fetch_restrict: false,
                fetch_restrict_satisfied: false,
                download_files: Vec::new(),
                required_by: vec![],
                source: CandidateSource::Ebuild,
                provenance: Default::default(),
                keyword_suggestion: None,
                use_suggestion: None,
                parent_use_suggestion: None,
                targets_running_root: false,
                remote_binary: false,
                build_id: None,
                deps: Vec::new(),
            },
            GraphEntry {
                category: "dev-libs".into(),
                package: "nosuchpkg".into(),
                outcome: PretendOutcome::NoVisibleCandidate,
                blockers: vec![],
                slot: None,
                sub_slot: None,
                repo_name: None,
                oldbest: vec![],
                use_flags_display: vec![],
                use_expand_display: vec![],
                use_expand_display_p: vec![],
                keyword_mask: None,
                new_slot: false,
                interactive: false,
                fetch_restrict: false,
                fetch_restrict_satisfied: false,
                download_files: Vec::new(),
                required_by: vec![],
                source: CandidateSource::Ebuild,
                provenance: Default::default(),
                keyword_suggestion: None,
                use_suggestion: None,
                parent_use_suggestion: None,
                targets_running_root: false,
                remote_binary: false,
                build_id: None,
                deps: Vec::new(),
            },
        ];
        let bogus = PathBuf::from("/nonexistent/does/not/exist");
        let result = run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &[],
            &bogus,
            &bogus,
            &PackageOptions {
                debug: false,
                pkgdir: bogus.clone(),
                distdir: bogus.clone(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn real_buildpkgonly_builds_a_real_binary_package_end_to_end() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![GraphEntry {
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }];

        let result = run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        assert!(result.is_ok(), "{result:?}");

        let tbz2 = pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
        assert!(tbz2.is_file(), "{tbz2:?} should exist");
        let bytes = fs::read(&tbz2).unwrap();
        assert!(
            bytes.windows(8).any(|w| w == b"XPAKPACK"),
            "missing real XPAK magic bytes"
        );

        // #39: `packagepkg` installs a real file, so the
        // `_post_src_install_uid_fix` size walk must record a positive
        // `SIZE` in the archive's metadata.
        let meta = crate::binpkg::read_xpak_metadata(&tbz2).expect("xpak metadata parses");
        assert!(
            meta.get("SIZE")
                .and_then(|s| s.trim().parse::<u64>().ok())
                .is_some_and(|n| n > 0),
            "SIZE must be positive for a package with a real file, got {:?}",
            meta.get("SIZE")
        );

        let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
        assert!(packages.contains("CPV: dev-libs/packagepkg-1.0"));
    }

    fn source_entry(package: &str, outcome: PretendOutcome) -> GraphEntry {
        GraphEntry {
            category: "dev-libs".into(),
            package: package.into(),
            outcome,
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    /// #37 S2: with a resolved config in scope, `entry_build_env` threads
    /// the full effective `USE` (implicit profile flags included), the
    /// per-package `USE_EXPAND` values, and the entry's `SLOT`/repo
    /// identity -- the S0 oracle's exact row set
    /// (`TEST/findings/l2.md` "S0 recon (#37)").
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn entry_build_env_resolves_full_use_expand_and_entry_identity() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let mut config = portage_profile::Config::default();
        // The profile layers `effective_use_flags` replays: implicit
        // arch/elibc/kernel flags no package declares in IUSE.
        config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
        config.iuse_effective = [
            "abi_x86_64",
            "amd64",
            "elibc_glibc",
            "kernel_linux",
            "riscv",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        config.use_expand = ["ABI_X86"].iter().map(|s| s.to_string()).collect();
        config
            .other_vars
            .insert("FEATURES".to_string(), "sandbox".to_string());
        let mut options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        options.build_env = portage_profile::phase_environ(&config, None);
        options.resolved_config = Some(std::sync::Arc::new(config));

        let mut entry = source_entry(
            "archusepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let env = entry_build_env(&options, &entry, &repos);
        // Later pairs win in both backends (`Command::envs` / successive
        // `export`s), so read the *last* pair: the run-wide base carries
        // empty `USE_EXPAND` placeholders that the per-entry rows override.
        fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
            env.iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
        }
        // `archusepkg` declares `IUSE="amd64 riscv"`; the implicit flags
        // come from the resolver's full effective set, not from IUSE.
        assert_eq!(
            get(&env, "USE"),
            Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
        );
        assert_eq!(
            get(&env, "IUSE_EFFECTIVE"),
            Some("abi_x86_64 amd64 elibc_glibc kernel_linux riscv")
        );
        assert_eq!(get(&env, "ABI_X86"), Some("64"));
        assert_eq!(get(&env, "SLOT"), Some("0"));
        assert_eq!(get(&env, "PORTAGE_REPO_REVISIONS"), Some("{}"));
        assert_eq!(get(&env, "PORTAGE_REPO_NAME"), Some("testrepo"));
        // The run-wide half rides along, folded `FEATURES` included.
        assert_eq!(get(&env, "FEATURES"), Some("sandbox"));
        // Real `environ_filter`/`AA`-pop rows stay out (S0 findings).
        for absent in ["O", "AA", "SRC_URI", "PORTAGE_USE"] {
            assert_eq!(get(&env, absent), None, "{absent}");
        }

        // A declared sub-slot renders `slot/sub_slot`; a missing
        // `entry.repo_name` falls back to the candidate's own repo.
        entry.sub_slot = Some("5".to_string());
        entry.repo_name = None;
        let env = entry_build_env(&options, &entry, &repos);
        assert_eq!(get(&env, "SLOT"), Some("0/5"));
        assert!(
            get(&env, "PORTAGE_REPO_NAME").is_some_and(|v| !v.is_empty()),
            "repo fallback missing"
        );
    }

    /// #37 S2 end-to-end: a real source merge with a resolved config
    /// threads the full effective `USE`, the resolved (folded)
    /// `FEATURES` -- which must win over `phase_env_vars`' raw process-env
    /// base -- and the entry's `SLOT` into the phase, under **both**
    /// backends. `build-info/USE` is what real `__dyn_install` writes from
    /// the phase `${USE}`; the `.keep_*` marker is written by the external
    /// `ebuild-helpers/keepdir` subprocess, which is why an unexported
    /// `SLOT` used to produce a bare `-` (S0's `l2-env-*` findings).
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn source_merge_with_resolved_config_threads_use_and_slot_into_the_phase() {
        for (shell, label) in [
            (ebuild_phases::ShellBackend::Bash, "bash"),
            (ebuild_phases::ShellBackend::Brush, "brush"),
        ] {
            let config_root = fixtures_root();
            let repos = find_repos(&config_root).unwrap();
            let root = tempdir();
            let portage_tmpdir = tempdir();
            let mut config = portage_profile::Config::default();
            config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
            config.iuse_effective = ["abi_x86_64", "amd64", "elibc_glibc", "kernel_linux"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut config_env = config.clone();
            config_env.other_vars.insert(
                "FEATURES".to_string(),
                "resolved features token".to_string(),
            );
            let mut options = ebuild_merge::MergeOptions {
                distdir: tempdir(),
                config_root: config_root.clone(),
                shell,
                ..ebuild_merge::MergeOptions::default()
            };
            options.build_env = portage_profile::phase_environ(&config_env, None);
            options.resolved_config = Some(std::sync::Arc::new(config));
            // The `${T}`/`build-info` files asserted below are the
            // subject; keep the builddir the way real `FEATURES=noclean`
            // does (the default post-merge clean is pinned separately).
            options.features = "noclean".to_string();

            let entries = vec![source_entry(
                "phaseenvpkg",
                PretendOutcome::New {
                    version: "1.0".into(),
                },
            )];
            run_source_merge(
                &entries,
                &repos,
                &root,
                &portage_tmpdir,
                &options,
                false,
                None,
                &[],
                1,
                None,
                false,
            )
            .unwrap_or_else(|e| panic!("{label}: source merge succeeds: {e}"));

            let keep = root.join("var/lib/phaseenvtest/.keep_dev-libs_phaseenvpkg-0");
            assert!(
                keep.exists(),
                "{label}: keepdir marker missing: {}",
                keep.display()
            );
            let t_dir = portage_tmpdir.join("portage/dev-libs/phaseenvpkg-1.0/temp");
            assert_eq!(
                fs::read_to_string(t_dir.join("phase-env-use.txt")).unwrap(),
                "USE=abi_x86_64 amd64 elibc_glibc kernel_linux\n",
                "{label}: USE"
            );
            assert_eq!(
                fs::read_to_string(t_dir.join("phase-env-slot.txt")).unwrap(),
                "SLOT=0\n",
                "{label}: SLOT"
            );
            assert_eq!(
                fs::read_to_string(t_dir.join("phase-env-features.txt")).unwrap(),
                "FEATURES=resolved features token\n",
                "{label}: the resolved FEATURES must win over the raw process-env base"
            );
            let build_use = fs::read_to_string(
                portage_tmpdir.join("portage/dev-libs/phaseenvpkg-1.0/build-info/USE"),
            )
            .expect("build-info/USE should be written by the real __dyn_install");
            assert_eq!(
                build_use.trim(),
                "abi_x86_64 amd64 elibc_glibc kernel_linux",
                "{label}: build-info/USE"
            );

            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(&portage_tmpdir);
        }
    }

    /// #37 S2: `--buildpkgonly` threads the same resolved env as the
    /// `-b` merge path -- the archive's `metadata/USE`/`metadata/FEATURES`
    /// (real `__dyn_install` writes both into `build-info` from the phase
    /// env) and the `Packages` index `USE` field carry the effective
    /// flags, not the raw harness env / empty standalone env.
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn buildpkgonly_with_resolved_config_writes_resolved_use_and_features() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();
        let mut config = portage_profile::Config::default();
        config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
        config.iuse_effective = ["abi_x86_64", "amd64", "elibc_glibc", "kernel_linux"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        config
            .other_vars
            .insert("FEATURES".to_string(), "sandbox".to_string());

        let entries = vec![source_entry(
            "phaseenvpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )];
        run_buildpkgonly(
            &entries,
            &config,
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                binpkg_format: "gpkg".to_string(),
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        )
        .expect("--buildpkgonly succeeds");

        let archive = pkgdir.join("dev-libs/phaseenvpkg-1.0.gpkg.tar");
        let meta = crate::binpkg::read_gpkg_metadata(&archive)
            .expect("portuale's gpkg reader parses the real writer's output");
        assert_eq!(
            meta.get("USE").map(String::as_str),
            Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
        );
        assert_eq!(meta.get("FEATURES").map(String::as_str), Some("sandbox"));
        assert_eq!(meta.get("SLOT").map(String::as_str), Some("0"));
        // #39: real `_post_src_install_write_metadata` always writes
        // `IUSE` (empty for a no-IUSE ebuild), the profile-computed
        // `IUSE_EFFECTIVE`, and `_post_src_install_uid_fix`'s own
        // `${D}`-walk `SIZE`.
        assert_eq!(meta.get("IUSE").map(String::as_str), Some(""));
        assert_eq!(
            meta.get("IUSE_EFFECTIVE").map(String::as_str),
            Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
        );
        // `phaseenvpkg` installs only a `keepdir` (the `.keep_*` marker
        // lands at merge time), so its image holds no regular file and
        // real's own walk writes `0` -- the member must be *present*.
        assert!(
            meta.get("SIZE")
                .and_then(|s| s.parse::<u64>().ok())
                .is_some(),
            "SIZE must be the real installed-size walk, got {:?}",
            meta.get("SIZE")
        );

        let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
        assert!(
            packages.contains("USE: abi_x86_64 amd64 elibc_glibc kernel_linux"),
            "Packages index USE missing the resolved flags:
{packages}"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
        let _ = fs::remove_dir_all(&pkgdir);
    }

    #[test]
    fn run_source_merge_builds_and_merges_a_new_package_end_to_end() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        // `samepkg` is packagepkg's RDEPEND -- already installed, so it's
        // an AlreadyInstalled entry that must be skipped silently.
        let entries = vec![
            source_entry(
                "samepkg",
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".into(),
                },
            ),
            source_entry(
                "packagepkg",
                PretendOutcome::New {
                    version: "1.0".into(),
                },
            ),
        ];

        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            1,
            None,
            false,
        )
        .expect("source merge succeeds");

        assert_eq!(
            fs::read_to_string(root.join("usr/share/packagepkg/hello.txt"))
                .unwrap()
                .trim(),
            "hello from packagepkg"
        );
        let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
        assert!(vdb.join("CONTENTS").is_file());
        assert_eq!(
            fs::read_to_string(vdb.join("RDEPEND")).unwrap().trim(),
            "dev-libs/samepkg"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    /// Backlog #42 regression: `merge_one_source_entry` (the serial
    /// `emerge` source path) pre-cleans before every build, exactly like
    /// real `_emerge/EbuildBuild._start_pre_clean`. With a stale
    /// `.installed` marker and a tampered `${D}` left by an earlier build
    /// in the same `${PORTAGE_BUILDDIR}` (kept here via `noclean`), the
    /// next run must discard them and rebuild -- previously `install`
    /// printed "already installed; skipping" and merged the stale image,
    /// which is how a `-B` after a source merge shipped stripped
    /// binaries (#38 S4).
    #[test]
    fn source_merge_pre_cleans_a_stale_installed_image() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let entry = source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );

        // First merge with noclean: the build state survives.
        let options = ebuild_merge::MergeOptions {
            features: "noclean".to_string(),
            ..ebuild_merge::MergeOptions::default()
        };
        merge_one_source_entry(&entry, &repos, &root, &portage_tmpdir, &options, None)
            .expect("first merge succeeds");
        let builddir = portage_tmpdir.join("portage/dev-libs/packagepkg-1.0");
        assert!(
            builddir.join(".installed").exists(),
            "noclean must keep the first build's state"
        );

        // Tamper: replace the real image file with a stale one, keeping
        // the `.installed` marker -- the exact state the missing
        // pre-clean treated as "already built".
        let image = builddir.join("image/usr/share/packagepkg");
        std::fs::remove_file(image.join("hello.txt")).unwrap();
        std::fs::write(image.join("stale.txt"), "stale\n").unwrap();

        // Second merge, no noclean: the pre-clean drops the stale
        // `.installed`/image, so `install` really re-runs.
        let options = ebuild_merge::MergeOptions {
            features: "sandbox".to_string(),
            ..ebuild_merge::MergeOptions::default()
        };
        merge_one_source_entry(&entry, &repos, &root, &portage_tmpdir, &options, None)
            .expect("second merge succeeds");
        assert!(root.join("usr/share/packagepkg/hello.txt").is_file());
        assert!(
            !root.join("usr/share/packagepkg/stale.txt").exists(),
            "the stale image must not be merged"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    /// Backlog #42: real `_buildpkgonly_success_hook_exit` runs the
    /// `clean` phase after a successful `--buildpkgonly` package, so the
    /// builddir is gone afterwards (the phase itself, not a `noclean`
    /// gate, honors `keepwork`).
    #[test]
    fn buildpkgonly_post_cleans_the_builddir() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )];
        run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        )
        .expect("--buildpkgonly succeeds");

        assert!(pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let builddir = portage_tmpdir.join("portage/dev-libs/packagepkg-1.0");
        assert!(
            !builddir.join(".installed").exists(),
            "the buildpkgonly tail must clean .installed"
        );
        assert!(
            !builddir.join("image").exists(),
            "the buildpkgonly tail must clean the image"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
        let _ = fs::remove_dir_all(&pkgdir);
    }

    #[test]
    fn capture_log_also_captures_pkg_preinst_and_pkg_postinst_output() {
        // Real `Scheduler._background_mode`: under `capture_log` (always
        // true for `--jobs` >1, opt-in via `--quiet-build`/`-q`
        // otherwise), a package's own `install` phase output already
        // went to its own `build.log` -- but `pkg_preinst`/`pkg_postinst`
        // (run separately, not part of `install`'s own `actionmap_deps`
        // chain -- `merge_after_install`'s own doc comment) previously
        // had no log file threaded to them at all, so their own output
        // always leaked straight to the terminal regardless of
        // `capture_log`. `hookoutputpkg`'s `pkg_preinst`/`pkg_postinst`
        // each echo an observable marker for this test to look for in
        // the captured log instead.
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        let entries = vec![source_entry(
            "hookoutputpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )];
        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            // The `${T}/build.log` this test reads is removed by the real
            // post-merge clean; keep it the way real `FEATURES=noclean`
            // does (the clean itself is pinned separately, #42).
            features: "noclean".to_string(),
            ..ebuild_merge::MergeOptions::default()
        };
        run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            1,
            None,
            true, // capture_log
        )
        .expect("source merge succeeds");

        let log = build_log_path(&portage_tmpdir, "dev-libs", "hookoutputpkg", "1.0", "");
        let log_text =
            fs::read_to_string(&log).unwrap_or_else(|e| panic!("{}: {e}", log.display()));
        assert!(
            log_text.contains("HOOKOUTPUTPKG-PREINST-MARKER"),
            "pkg_preinst output missing from the captured log:\n{log_text}"
        );
        assert!(
            log_text.contains("HOOKOUTPUTPKG-POSTINST-MARKER"),
            "pkg_postinst output missing from the captured log:\n{log_text}"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn run_build_scheduler_builds_two_leaves_in_parallel_then_the_parent() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        // schedparent RDEPENDs schedleaf-a + schedleaf-b (both leaves,
        // buildable, not installed). required_by wires the DAG so the
        // scheduler builds the leaves first (concurrently under jobs=2)
        // and schedparent only after both have merged.
        let leaf_a = source_entry(
            "schedleaf-a",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let mut leaf_a = leaf_a;
        leaf_a.required_by = vec![("dev-libs".into(), "schedparent".into())];
        let mut leaf_b = source_entry(
            "schedleaf-b",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        leaf_b.required_by = vec![("dev-libs".into(), "schedparent".into())];
        let parent = source_entry(
            "schedparent",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let entries = vec![leaf_a, leaf_b, parent];

        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            2,
            None,
            false,
        )
        .expect("parallel source merge succeeds");

        for pkg in ["schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"] {
            assert!(
                root.join(format!("var/db/pkg/dev-libs/{pkg}/CONTENTS"))
                    .is_file(),
                "{pkg} should be merged"
            );
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn system_loadavg_1min_reads_a_plausible_value() {
        // Linux CI: /proc/loadavg exists and its first field is a
        // non-negative float. Anywhere it can't be read, the throttle is
        // simply disabled (0.0), never a stall.
        let la = system_loadavg_1min();
        assert!(la >= 0.0 && la.is_finite(), "{la}");
    }

    #[test]
    fn run_build_scheduler_with_a_high_load_average_never_throttles() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        let mut leaf_a = source_entry(
            "schedleaf-a",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        leaf_a.required_by = vec![("dev-libs".into(), "schedparent".into())];
        let mut leaf_b = source_entry(
            "schedleaf-b",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        leaf_b.required_by = vec![("dev-libs".into(), "schedparent".into())];
        let parent = source_entry(
            "schedparent",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let entries = vec![leaf_a, leaf_b, parent];

        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        // A load average of 1e9 can never be exceeded -> the throttle is
        // a no-op and every package still builds and merges.
        run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            2,
            Some(1e9),
            false,
        )
        .expect("high --load-average must not stall the scheduler");
        for pkg in ["schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"] {
            assert!(
                root.join(format!("var/db/pkg/dev-libs/{pkg}/CONTENTS"))
                    .is_file()
            );
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn run_build_scheduler_keep_going_skips_a_failed_builds_dependents() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        // schedbad's src_install dies; schedbaddep RDEPENDs it (so it must
        // be skipped); schedok is independent and must still merge.
        let mut bad = source_entry(
            "schedbad",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        bad.required_by = vec![("dev-libs".into(), "schedbaddep".into())];
        let baddep = source_entry(
            "schedbaddep",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let ok = source_entry(
            "schedok",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let entries = vec![bad, baddep, ok];

        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        let err = run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            true,
            None,
            &[],
            2,
            None,
            false,
        )
        .expect_err("a failed build must make the whole run fail under --keep-going");
        assert!(err.contains("schedbad-1.0"), "{err}");
        assert!(err.contains("schedbaddep"), "{err}");

        assert!(
            root.join("var/db/pkg/dev-libs/schedok-1.0/CONTENTS")
                .is_file(),
            "the independent schedok must still merge"
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/schedbaddep-1.0").exists(),
            "schedbaddep depends on the failed schedbad and must be skipped"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn a_hard_failure_kills_still_running_builds_instead_of_waiting_them_out() {
        // Real `Scheduler._keep_scheduling`/`_terminate_tasks`
        // (`PollScheduler.py:106-126`): once any package fails without
        // `--keep-going`, real portage sends kill signals to whatever's
        // still running rather than letting it finish -- its own result
        // would be discarded regardless. `schedbad` (fails almost
        // instantly, in `install`, the last phase) and `schedslow`
        // (sleeps 20s in `compile`, well before `install`) are
        // independent leaves under `jobs=2`, so both start together;
        // `schedbad` fails long before `schedslow`'s own sleep would
        // ever finish on its own.
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();

        let bad = source_entry(
            "schedbad",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let slow = source_entry(
            "schedslow",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        let entries = vec![bad, slow];

        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        let started = std::time::Instant::now();
        let err = run_source_merge(
            &entries,
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false, // keep_going
            None,
            &[],
            2,
            None,
            false,
        )
        .expect_err("schedbad's own failure must fail the whole run");
        assert!(err.contains("schedbad-1.0"), "{err}");
        // Real generously bounded: well under schedslow's own 20s sleep,
        // proving the scheduler didn't just wait it out.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "run_source_merge took {:?}, schedslow's sleep should have been killed",
            started.elapsed()
        );

        let marker = portage_tmpdir
            .join("portage/dev-libs/schedslow-1.0/temp/schedslow-slept-to-completion");
        assert!(
            !marker.exists(),
            "schedslow's own sleep must have been killed, not left to finish"
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn entry_matches_any_checks_the_resolved_cpv_against_buildpkg_exclude_atoms() {
        let e = {
            let mut e = source_entry(
                "packagepkg",
                PretendOutcome::New {
                    version: "1.2".into(),
                },
            );
            e.slot = Some("0".into());
            e.sub_slot = Some("0".into());
            e
        };
        assert!(!entry_matches_any(&e, &[]));
        assert!(entry_matches_any(&e, &["dev-libs/packagepkg".to_string()]));
        assert!(entry_matches_any(
            &e,
            &[">=dev-libs/packagepkg-1".to_string()]
        ));
        assert!(entry_matches_any(
            &e,
            &["dev-libs/packagepkg:0".to_string()]
        ));
        assert!(!entry_matches_any(&e, &["dev-libs/other".to_string()]));
        assert!(!entry_matches_any(
            &e,
            &["dev-libs/packagepkg:1".to_string()]
        ));
        assert!(!entry_matches_any(
            &e,
            &["<dev-libs/packagepkg-1".to_string()]
        ));
        // A non-mergeable outcome never matches.
        let ai = source_entry(
            "packagepkg",
            PretendOutcome::AlreadyInstalled {
                version: "1.2".into(),
            },
        );
        assert!(!entry_matches_any(
            &ai,
            &["dev-libs/packagepkg".to_string()]
        ));
    }

    #[test]
    fn run_source_merge_rejects_a_binary_entry() {
        // The error is raised before any real execution, so a bogus
        // ROOT/tmpdir that would fail loudly if touched is safe here.
        let bogus = PathBuf::from("/nonexistent/does/not/exist");
        let options = ebuild_merge::MergeOptions::default();

        let mut binary = source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        );
        binary.source = CandidateSource::Binary;
        let err = run_source_merge(
            &[binary],
            &[],
            &bogus,
            &bogus,
            &options,
            false,
            None,
            &[],
            1,
            None,
            false,
        )
        .unwrap_err();
        assert!(err.contains("binary package"), "{err}");
    }

    #[test]
    fn run_merge_loop_without_keep_going_stops_at_the_first_failure() {
        let a = source_entry(
            "aaa",
            PretendOutcome::New {
                version: "1".into(),
            },
        );
        let b = source_entry(
            "bbb",
            PretendOutcome::New {
                version: "1".into(),
            },
        );
        let mut seen: Vec<String> = Vec::new();
        let err = run_merge_loop(&[a, b], false, |e| {
            seen.push(e.package.clone());
            Err(format!("{} boom", e.package))
        })
        .unwrap_err();
        assert_eq!(err, "aaa boom");
        assert_eq!(seen, vec!["aaa".to_string()]);
    }

    #[test]
    fn run_merge_loop_keep_going_skips_the_failed_packages_transitive_dependents() {
        // dep <- mid <- top   (top depends on mid depends on dep);
        // `other` is independent. `dep` fails, so `mid` and `top` are
        // dropped, `other` still merges, and the combined Err names all.
        let mut dep = source_entry(
            "dep",
            PretendOutcome::New {
                version: "1".into(),
            },
        );
        dep.required_by = vec![("dev-libs".into(), "mid".into())];
        let mut mid = source_entry(
            "mid",
            PretendOutcome::New {
                version: "1".into(),
            },
        );
        mid.required_by = vec![("dev-libs".into(), "top".into())];
        let top = source_entry(
            "top",
            PretendOutcome::New {
                version: "1".into(),
            },
        );
        let other = source_entry(
            "other",
            PretendOutcome::New {
                version: "1".into(),
            },
        );

        let mut merged: Vec<String> = Vec::new();
        let err = run_merge_loop(&[dep, mid, top, other], true, |e| {
            if e.package == "dep" {
                return Err("dep boom".into());
            }
            merged.push(e.package.clone());
            Ok(())
        })
        .unwrap_err();

        assert_eq!(merged, vec!["other".to_string()]);
        assert!(
            err.contains("1 package(s) failed to merge (--keep-going):"),
            "{err}"
        );
        assert!(err.contains("  dep boom"), "{err}");
        assert!(err.contains("2 dependent package(s) not merged:"), "{err}");
        assert!(err.contains("  dev-libs/mid"), "{err}");
        assert!(err.contains("  dev-libs/top"), "{err}");
    }

    #[test]
    fn run_source_merge_upgrade_replaces_the_installed_version() {
        // Merge binpkgrmpkg-1.0 (New), then 2.0 (Upgrade) -- 2.0's files
        // land, 1.0's own file is unmerged, 1.0's vdb entry is gone, and
        // 1.0's pkg_prerm/pkg_postrm run from its own saved vdb env (the
        // fixture's five hooks each append `<phase>-<PVR>` to a ROOT log).
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };

        run_source_merge(
            &[source_entry(
                "binpkgrmpkg",
                PretendOutcome::New {
                    version: "1.0".into(),
                },
            )],
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            1,
            None,
            false,
        )
        .expect("1.0 merges");
        run_source_merge(
            &[source_entry(
                "binpkgrmpkg",
                PretendOutcome::Upgrade {
                    from: "1.0".into(),
                    to: "2.0".into(),
                },
            )],
            &repos,
            &root,
            &portage_tmpdir,
            &options,
            false,
            None,
            &[],
            1,
            None,
            false,
        )
        .expect("2.0 upgrade merges");

        assert!(
            root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS")
                .is_file()
        );
        assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists());
        assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
        assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());
        assert_eq!(
            fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n\
             setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
        );
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&portage_tmpdir);
    }

    #[test]
    fn real_buildpkgonly_refuses_a_real_src_uri_with_no_manifest_entry() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![GraphEntry {
            category: "dev-libs".into(),
            package: "fetchpkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }];

        let result = run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                // Pinned to "bzip2" (near-universal base package) rather
                // than real Default's "zstd", so these tests don't
                // depend on the test-running host actually having zstd
                // installed -- real xpak/tbz2 building is codec-
                // agnostic either way.
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        // `fetchpkg`'s own fixture has a real, nonempty SRC_URI but no
        // Manifest entry at all -- refused before any network access is
        // even attempted (see `crate::fetch::fetch_src_uri`'s own doc
        // comment: unverifiable content is worse than a loud failure).
        let err = result.expect_err("an unverifiable SRC_URI must be refused");
        assert!(err.contains("no Manifest entry"), "{err}");
        assert!(
            !pkgdir.join("dev-libs/fetchpkg-1.0.tbz2").exists(),
            "must not have built anything"
        );
    }

    fn buildpkgonly_entry(category: &str, package: &str, version: &str) -> GraphEntry {
        GraphEntry {
            category: category.into(),
            package: package.into(),
            outcome: PretendOutcome::New {
                version: version.into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    /// Without real `--keep-going`, a failing entry stops the whole run
    /// immediately -- a later, independently-buildable entry in the same
    /// list never even gets attempted.
    #[test]
    fn real_buildpkgonly_without_keep_going_stops_at_the_first_failure() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        // fetchpkg (no Manifest entry, always fails) listed *before*
        // packagepkg (builds cleanly) -- proves packagepkg is never even
        // attempted once fetchpkg fails.
        let entries = vec![
            buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
            buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
        ];

        let result = run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            false,
        );
        let err = result.expect_err("fetchpkg must still fail");
        assert!(err.contains("no Manifest entry"), "{err}");
        assert!(
            !pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists(),
            "packagepkg must never be attempted once fetchpkg fails without --keep-going"
        );
    }

    /// With real `--keep-going`, a failing entry does *not* stop the
    /// run -- packagepkg still gets built despite fetchpkg's own
    /// failure, and the final error names both entries.
    #[test]
    fn real_buildpkgonly_with_keep_going_builds_past_a_failure() {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let pkgdir = tempdir();

        let entries = vec![
            buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
            buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
        ];

        let result = run_buildpkgonly(
            &entries,
            &portage_profile::Config::default(),
            &repos,
            &root,
            &portage_tmpdir,
            &PackageOptions {
                debug: false,
                pkgdir: pkgdir.clone(),
                distdir: tempdir(),
                shell: PackageOptions::default().shell,
                binpkg_compress: "bzip2".to_string(),
                ..PackageOptions::default()
            },
            true,
        );
        let err = result.expect_err("fetchpkg still fails overall");
        assert!(err.contains("dev-libs/fetchpkg-1.0"), "{err}");
        assert!(err.contains("no Manifest entry"), "{err}");

        assert!(
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file(),
            "packagepkg must still be built with --keep-going, despite fetchpkg's own failure"
        );
    }
}
