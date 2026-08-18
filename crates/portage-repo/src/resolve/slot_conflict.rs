//! Slot conflicts: records, merging and rendering.

use crate::*;

/// A genuine slot conflict: two different atoms landing on the same
/// category/package/slot whose independently-resolved best versions are
/// incompatible. Purely informational ("report, don't enforce"): unlike
/// Portage's fatal depgraph error, portuale reports it and keeps the
/// first-resolved version. Different slots coexisting is not a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into), by whichever atom
    /// reached this slot first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
    /// Every distinct version pulled into the graph (>= 2) with its parents;
    /// `instances[0]` is always `resolved_version`.
    pub instances: Vec<SlotConflictInstance>,
}

/// One version of a slot-conflicted `category/package:slot`, plus its pullers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictInstance {
    pub version: String,
    pub sub_slot: String,
    pub repo_name: String,
    /// The `USE="…"` pairs for this instance's header line (empty renders
    /// `USE=""`); vdb-based for an installed instance.
    pub use_display: Vec<(String, String)>,
    pub parents: Vec<SlotConflictParent>,
    /// Portage's installed nomerge node: kept by a residual consumer pin,
    /// rendered `(… installed in '<root>')`. Always false for merge-vs-merge.
    pub installed: bool,
}

/// One parent that pulled a `SlotConflictInstance` into the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictParent {
    /// `cat/pkg-ver:slot/sub_slot::repo`, or empty for a top-level atom.
    pub parent_cpv: String,
    pub atom: String,
    /// The parent's own resolved `USE="…"` for its `required by` line; empty
    /// for a top-level `(Argument)` parent, vdb-based for an installed one.
    pub use_display: Vec<(String, String)>,
    /// An installed parent whose recorded atom the merge breaks.
    pub installed: bool,
}

/// `(sub_slot, repo_name, slot)` for `cat/pkg`'s `version`: the highest-priority
/// candidate with that version; all-empty when versionless.
pub(crate) fn slot_conflict_meta(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> (String, String, String) {
    match list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .cloned()
        }) {
        Some(c) => (c.sub_slot, c.repo_name, c.slot),
        None => (String::new(), String::new(), String::new()),
    }
}

/// Which conflict party is installed rather than merge-bound: `Existing` is
/// instance A (already in the graph), `Current` instance B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstalledSide {
    Existing,
    Current,
}

/// `` `cat/pkg-ver:slot/sub_slot::repo` `` for a conflicting parent; empty for
/// a top-level parent. Shared by both conflict report builders.
pub(crate) fn slot_conflict_puller_cpv(
    repos: &[RepoConfig],
    pc: &str,
    pp: &str,
    pv: &str,
) -> String {
    if pc.is_empty() {
        return String::new();
    }
    let (psub, prepo, pslot) = slot_conflict_meta(repos, pc, pp, pv);
    format!("{pc}/{pp}-{pv}:{pslot}/{psub}::{prepo}")
}

/// Records a fresh `SlotConflict`, replacing the existing record for the same
/// `(category, package, slot, existing, current)` triple.
pub(crate) fn record_slot_conflict(slot_conflicts: &mut Vec<SlotConflict>, sc: SlotConflict) {
    if let Some(prev) = slot_conflicts.iter_mut().find(|p| {
        p.category == sc.category
            && p.package == sc.package
            && p.slot == sc.slot
            && p.resolved_version == sc.resolved_version
            && p.instances.get(1).map(|i| &i.version) == sc.instances.get(1).map(|i| &i.version)
    }) {
        *prev = sc;
    } else {
        slot_conflicts.push(sc);
    }
}
