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
// every other real-execution slice in this pilot):
//   - A `CandidateSource::Binary` entry (would only appear via
//     `--usepkg`) is skipped outright -- it's already a binary, there is
//     nothing to build.
//   - Builds run strictly in `entries` order, which `resolve_pretend_graph`
//     now returns in real portage's dependency-first *merge* order
//     (`topological_merge_order`) -- so even if the `--buildpkgonly` gate
//     above were relaxed, a dep would still build before its dependent.
//   - A build failure aborts immediately, unless real `--keep-going` is
//     given (now real, see `run_buildpkgonly`'s own doc comment) -- no
//     cleanup of any already-built packages either way, this pilot's
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
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => None,
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
        .into_iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
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
/// failed) and stops there, matching this pilot's own long-established
/// default. With real `--keep-going` (real `main.py`'s own `y_or_n`
/// option, narrowed by this pilot's own CLI transcription to the bare/
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
    repos: &[RepoConfig],
    root: &Path,
    portage_tmpdir: &Path,
    options: &PackageOptions,
    keep_going: bool,
) -> Result<(), String> {
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
        let failure = match ebuild_package::run_package(&path, root, portage_tmpdir, options) {
            Ok(0) => None,
            Ok(_) => Some(format!(
                "{}/{}-{version}: build failed",
                entry.category, entry.package
            )),
            Err(e) => Some(format!(
                "{}/{}-{version}: {e}",
                entry.category, entry.package
            )),
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
/// `--getbinpkgonly`: the pilot's first source build-and-merge path for
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
) -> Result<(), String> {
    if jobs > 1 {
        return run_build_scheduler(
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
        );
    }
    run_merge_loop(entries, keep_going, |entry| {
        let bp = buildpkg.filter(|_| !entry_matches_any(entry, buildpkg_exclude));
        merge_one_source_entry(entry, repos, root, portage_tmpdir, options, bp)
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

/// The shared per-entry loop for `run_source_merge` /
/// `emerge_getbinpkg::run_merge_plan`. Without `keep_going` it stops at
/// the first failure (`merge_one`'s own `Err`), the pilot's long-
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
        PretendOutcome::AlreadyInstalled { .. } => return Ok(()),
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
    per_entry.build_env = entry_build_env(options, entry);
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

/// The full per-entry build-phase env: the run-wide compiler/make flags
/// the caller stashed on `options.build_env` (`pretend.rs::
/// build_config_env`), then this entry's own resolved `USE` on top.
fn entry_build_env(
    options: &ebuild_merge::MergeOptions,
    entry: &GraphEntry,
) -> Vec<(String, String)> {
    let mut env = options.build_env.clone();
    env.extend(build_use_env(entry));
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
        PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => {
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
/// (and `--resume` doesn't pass `--keep-going` anyway).
pub(crate) fn resume_entry(category: &str, package: &str, version: &str) -> GraphEntry {
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
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
    }
}

/// Real portage's `PORTAGE_LOG_FILE` (`PORTAGE_LOGDIR` unset →
/// `${T}/build.log`, i.e. `${PORTAGE_BUILDDIR}/temp/build.log`).
fn build_log_path(portage_tmpdir: &Path, category: &str, package: &str, version: &str) -> PathBuf {
    portage_tmpdir
        .join("portage")
        .join(category)
        .join(format!("{package}-{version}"))
        .join("temp")
        .join("build.log")
}

/// Last `n` lines of `path`, or a short "(build log unavailable)" note.
fn tail_of(path: &Path, n: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        Err(_) => "(build log unavailable)".to_string(),
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

    let log_path = capture_log
        .then(|| build_log_path(portage_tmpdir, &entry.category, &entry.package, &version));
    if let Some(lp) = &log_path {
        // Real `prepare_build_dirs` truncates a stale build.log.
        let _ = std::fs::remove_file(lp);
    }
    // A captured parallel build runs through real `bash` (not the
    // embedded `brush`), whose stdout+stderr redirect to `build.log`
    // cleanly at the OS level -- real portage uses real bash for builds
    // regardless of the pilot's default backend.
    let shell = if log_path.is_some() {
        ebuild_phases::ShellBackend::Bash
    } else {
        options.shell
    };
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
        &entry_build_env(options, entry),
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
        let status =
            ebuild_package::package_after_install(&path, root, portage_tmpdir, package_options)?;
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
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &ebuild_merge::MergeOptions,
) -> Result<(), String> {
    let (cp, version) = scheduler_cp_version(entry)?;
    // `merge_after_install`'s `pkg_preinst`/`pkg_postinst` see this
    // entry's resolved `USE` too (see `merge_one_source_entry`).
    let mut per_entry = options.clone();
    per_entry.build_env = entry_build_env(options, entry);
    let status = ebuild_merge::run_qmerge(ebuild_path, root, portage_tmpdir, &per_entry)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: merge failed ({status})"));
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
            if let Some(&r_idx) = cp_to_idx.get(r_cp) {
                if skip.insert(r_idx) {
                    skipped.push(format!(
                        "{}/{}",
                        entries[r_idx].category, entries[r_idx].package
                    ));
                    queue.push(r_idx);
                }
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
/// rather than stalling.
fn system_loadavg_1min() -> f64 {
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
    jobs: usize,
    // Real `--load-average`: don't start an *additional* build (beyond
    // the one always allowed, so the scheduler can't deadlock) while the
    // system 1-minute load average is above this.
    load_average: Option<f64>,
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

    std::thread::scope(|scope| -> Result<(), String> {
        let (tx, rx) = mpsc::channel::<(usize, Result<PathBuf, String>)>();
        let mut in_flight = 0usize;
        loop {
            while in_flight < jobs {
                // Real `--load-average`: hold off on an *additional* build
                // while the system is already loaded. Never blocks the
                // first build (`in_flight >= 1`), so the scheduler always
                // makes progress.
                if in_flight >= 1 {
                    if let Some(la) = load_average {
                        if system_loadavg_1min() > la {
                            break;
                        }
                    }
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
                let bp = buildpkg.filter(|_| !entry_matches_any(&entries[idx], buildpkg_exclude));
                let entry = &entries[idx];
                scope.spawn(move || {
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
                Ok(path) => {
                    merge_one_built_entry(&entries[idx], &path, root, portage_tmpdir, options).err()
                }
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
            },
        ];
        let bogus = PathBuf::from("/nonexistent/does/not/exist");
        let result = run_buildpkgonly(
            &entries,
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
        }];

        let result = run_buildpkgonly(
            &entries,
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
        }
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
        )
        .expect("high --load-average must not stall the scheduler");
        for pkg in ["schedleaf-a-1.0", "schedleaf-b-1.0", "schedparent-1.0"] {
            assert!(root
                .join(format!("var/db/pkg/dev-libs/{pkg}/CONTENTS"))
                .is_file());
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
        )
        .expect("2.0 upgrade merges");

        assert!(root
            .join("var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS")
            .is_file());
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
        }];

        let result = run_buildpkgonly(
            &entries,
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
