//! Solves slot conflicts directly, without backtracking.

use crate::*;

/// Unconditional direct solve of solvable slot conflicts (no backtracking
/// gate: the capture says `backtrack: 0/0`, zero retries, yet reconciled).
// Portage: depgraph.py, _solve_non_slot_operator_slot_conflicts
pub(crate) struct DirectSolveInput<'a> {
    pub conflicts: &'a [SlotConflict],
    pub entries: &'a [GraphEntry],
    pub top_level: &'a HashSet<(String, String)>,
    pub excluded: &'a [String],
    pub selective: bool,
    pub replace_cps: &'a BTreeSet<(String, String)>,
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub config: &'a portage_profile::Config,
}

pub(crate) struct DirectSolveOutput {
    /// Merge instances to drop, each with the surviving same-slot
    /// version that takes over its merge row (`(category, package,
    /// removed_version, kept_version)`). The row is repointed rather
    /// than deleted: Portage merges the kept instance (its row stays),
    /// while the removed one vanishes -- and portuale holds one row
    /// per cp anyway.
    pub removed: Vec<(String, String, String, String)>,
    pub skipped: Vec<SkippedUpdate>,
    /// Conflicts with >= 2 surviving instances (the rest dissolve with
    /// the removal -- their story is the skip notice, not a block).
    pub surviving: Vec<SlotConflict>,
}
