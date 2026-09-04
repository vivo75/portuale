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

/// Resolved USE (enabled) plus declared IUSE for a conflict instance: merge
/// entries read their display flags, shadowed merges the tree candidate,
/// installed instances the vdb record.
pub(crate) fn direct_solve_instance_use(
    input: &DirectSolveInput<'_>,
    category: &str,
    package: &str,
    inst: &SlotConflictInstance,
) -> (HashSet<String>, HashSet<String>) {
    if inst.installed {
        let enabled = read_vdb_flag_set(input.root, category, package, &inst.version, "USE");
        let declared = list_candidates(input.repos, category, package)
            .ok()
            .and_then(|cs| {
                cs.iter().find(|c| c.version == inst.version).map(|c| {
                    c.iuse
                        .split_whitespace()
                        .map(|t| t.trim_start_matches(['+', '-']).to_string())
                        .collect()
                })
            })
            .unwrap_or_else(|| enabled.clone());
        return (enabled, declared);
    }
    if let Some(entry) = input.entries.iter().find(|e| {
        e.category.as_str() == category
            && e.package.as_str() == package
            && merge_bound_version(&e.outcome) == Some(&inst.version)
    }) {
        let enabled: HashSet<String> = entry
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        let declared: HashSet<String> = entry
            .use_flags_display
            .iter()
            .map(|(f, _)| f.clone())
            .collect();
        return (enabled, declared);
    }
    let (declared, enabled) =
        slot_conflict_flag_sets(input.repos, input.config, category, package, &inst.version);
    (enabled, declared)
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
