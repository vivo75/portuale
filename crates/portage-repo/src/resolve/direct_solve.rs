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

/// Whether the installed instance is excluded from every parent's match
/// list: the cp was explicitly requested, without `--exclude` and outside
/// selective mode.
// Portage: depgraph.py, _want_installed_pkg
pub(crate) fn direct_solve_arg_mode(input: &DirectSolveInput<'_>, c: &SlotConflict) -> bool {
    if !c.instances.iter().any(|i| i.installed) || input.selective {
        return false;
    }
    if !input
        .top_level
        .contains(&(c.category.clone(), c.package.clone()))
    {
        return false;
    }
    let inst_ver = c
        .instances
        .iter()
        .find(|i| i.installed)
        .map(|i| i.version.clone())
        .unwrap_or_default();
    let probe = format!("{}/{}-{inst_ver}", c.category, c.package);
    !input
        .excluded
        .iter()
        .any(|ex| matches_config_entry(ex, &probe, &c.category, &c.package))
}
