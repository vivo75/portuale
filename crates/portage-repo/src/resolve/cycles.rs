//! Circular dependencies: hard-cycle detection, restart triggers, virtual
//! cycles and USE-fix suggestions.

use crate::*;

/// Whether a fresh single-node self-loop closed by a kept `||` self-branch
/// may retry with the branch demoted. Multi-node rings, inline self-deps
/// and installed instances keep the report-no-retry path.
// Portage: dep_check.py, preferred_in_graph (pass-1 self choice)
pub(crate) fn cycle_restartable(
    entries: &[GraphEntry],
    kept: &[HashSet<usize>],
    cycle: &[String],
    root: &Path,
) -> bool {
    let [node_cpv] = cycle else {
        return false;
    };
    let Some((cat, pkg, _)) = split_cpv(node_cpv) else {
        return false;
    };
    // Fresh only: the demoted retry must never pull installed subgraphs.
    if !installed_candidates(root, &cat, &pkg).is_empty() {
        return false;
    }
    let Some(owner_idx) = entries.iter().position(|e| {
        e.category == cat && e.package == pkg && merge_bound_cpv(e).as_deref() == Some(node_cpv)
    }) else {
        return false;
    };
    let Some(owner_cpv) = merge_bound_cpv(&entries[owner_idx]) else {
        return false;
    };
    entries[owner_idx].deps.iter().enumerate().any(|(ei, d)| {
        d.alt.is_some()
            && kept[owner_idx].contains(&ei)
            && d.category == cat
            && d.package == pkg
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

/// Whether the owner's kept `deps` edges name and match its own merge-bound
/// instance. Self-loops touching an installed instance stay invisible:
/// reporting one would flip an outcome-correct resolve into a spurious
/// circular abort.
pub(crate) fn owner_self_loop(
    entries: &[GraphEntry],
    kept: &[std::collections::HashSet<usize>],
    oi: usize,
    root: &Path,
) -> bool {
    let Some(owner_cpv) = merge_bound_cpv(&entries[oi]) else {
        return false;
    };
    if !installed_candidates(root, &entries[oi].category, &entries[oi].package).is_empty() {
        return false;
    }
    entries[oi].deps.iter().enumerate().any(|(ei, d)| {
        (d.alt.is_none() || kept[oi].contains(&ei))
            && d.category == entries[oi].category
            && d.package == entries[oi].package
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

/// Splits `category/package-version` into its three parts (the last `-`
/// boundary whose right half verifies as a version).
pub fn split_cpv(cpv: &str) -> Option<(String, String, String)> {
    let (cat, rest) = cpv.split_once('/')?;
    let parts: Vec<&str> = rest.split('-').collect();
    for i in 1..parts.len() {
        let version = parts[i..].join("-");
        if portage_versions::ververify(&version) {
            return Some((cat.to_string(), parts[..i].join("-"), version));
        }
    }
    None
}
