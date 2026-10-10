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

mod buildpkgonly;
mod entry_env;
mod logs;
mod resume;
mod scheduler;
mod source_merge;
mod status;

#[cfg(test)]
mod tests;

pub use buildpkgonly::*;
pub(crate) use entry_env::*;
pub(crate) use logs::*;
pub(crate) use resume::*;
pub(crate) use scheduler::*;
pub use source_merge::*;
pub(crate) use status::*;

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
