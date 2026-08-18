//! `--buildpkgonly` execution, without `--pretend`: builds a binary
//! package for every entry the pretend gate proved safe to build.
//! Reuses `ebuild_package::run_package`; `locate_candidate` re-derives
//! the winning candidate's repo via `portage_repo::list_candidates`.

use crate::ebuild_merge;
mod resume;

#[cfg(test)]
mod tests;
pub(crate) use resume::*;

use crate::ebuild_package::{self, PackageOptions};

use crate::ebuild_phases;

use portage_repo::{Candidate, CandidateSource, GraphEntry, PretendOutcome, RepoConfig};

use std::path::{Path, PathBuf};

/// The version this entry would actually build at, or `None` for an
/// outcome `--buildpkgonly` never builds anything for.
fn entry_version(outcome: &PretendOutcome) -> Option<&str> {
    match outcome {
        PretendOutcome::New { version } => Some(version),
        PretendOutcome::Upgrade { to, .. } => Some(to),
        PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::Reinstall { version, .. } => Some(version),
        // A removal builds nothing.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// Re-finds the winning candidate for `category/package` at exactly
/// `version`. When more than one repo carries it, prefers the
/// highest-priority repo, the same tie-break resolving uses.
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

/// Whether `entry`'s resolved cpv (+ slot) matches any of `atoms`.
// Portage: InternalPackageSet.py, findAtomForPackage
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

/// Whether this candidate wants a binpkg: excluded atoms always win;
/// otherwise a `PROPERTIES=live` build is skipped unless `buildpkg_live`.
// Portage: Package.py, binpkg_wanted
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

/// Whether this entry wants a binpkg: `--buildpkg-exclude` always wins;
/// otherwise a `PROPERTIES=live` build needs `buildpkg_live`. An
/// unlocatable candidate falls through to `true`; the build path reports
/// the missing ebuild itself.
// Portage: Package.py, binpkg_wanted
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
