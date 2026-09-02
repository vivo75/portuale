//! Lookups over the entries a run will merge.

use crate::*;

/// The version a merge-bound outcome would install, or `None` for the outcomes
/// that merge nothing. Exactly the four outcomes `PassState::resolved_slots` indexes.
pub(crate) fn merge_bound_version(outcome: &PretendOutcome) -> Option<&String> {
    match outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => None,
    }
}

/// The `category/package-version` a merge-bound entry would install, or `None` for
/// entries that never participate in a build-time cycle.
pub(crate) fn merge_bound_cpv(entry: &GraphEntry) -> Option<String> {
    let version = merge_bound_version(&entry.outcome)?;
    Some(format!("{}/{}-{version}", entry.category, entry.package))
}

/// Lowest-index merge-bound entry per `(category, package)` -- the merge list is
/// already in dependency order, so the first is the instance an edge belongs to.
/// Shared by cycle nodes and owner lookup, so the two cannot disagree.
pub(crate) fn merge_bound_index(entries: &[GraphEntry]) -> HashMap<(&str, &str), usize> {
    let mut cp_index: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if merge_bound_cpv(e).is_some() {
            cp_index
                .entry((e.category.as_str(), e.package.as_str()))
                .or_insert(i);
        }
    }
    cp_index
}

/// First merge-bound index per (root, cp): the edge-resolution view (Portage's
/// package-keyed digraph never merges two roots' instances). `merge_bound_index`
/// stays the cp-level view for render order.
pub(crate) fn merge_bound_root_index(entries: &[GraphEntry]) -> HashMap<(bool, &str, &str), usize> {
    let mut index: HashMap<(bool, &str, &str), usize> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if merge_bound_cpv(e).is_some() {
            index
                .entry((
                    e.targets_running_root,
                    e.category.as_str(),
                    e.package.as_str(),
                ))
                .or_insert(i);
        }
    }
    index
}

/// Resolve one root-qualified map endpoint to its entry index -- the exact-root
/// instance first, falling back to the cp-level first index when this graph holds
/// no such instance in that root.
pub(crate) fn root_cp_lookup(
    root_index: &HashMap<(bool, &str, &str), usize>,
    cp_index: &HashMap<(&str, &str), usize>,
    endpoint: &(bool, String, String),
) -> Option<usize> {
    root_index
        .get(&(endpoint.0, endpoint.1.as_str(), endpoint.2.as_str()))
        .or_else(|| cp_index.get(&(endpoint.1.as_str(), endpoint.2.as_str())))
        .copied()
}

/// Resolve a `deps` edge to its child instance -- the resolving-root instance when
/// one is merge-bound, else the legacy cp-level first index. The walk always resolved
/// the atom in the resolving root, so the filter only ever removes a provably-wrong
/// same-cp other-root sibling.
pub(crate) fn dep_child_lookup(
    root_index: &HashMap<(bool, &str, &str), usize>,
    cp_index: &HashMap<(&str, &str), usize>,
    owner_running_root: bool,
    dep: &DepEdge,
) -> Option<usize> {
    let want = dep_child_root(owner_running_root, dep.key);
    root_index
        .get(&(want, dep.category.as_str(), dep.package.as_str()))
        .or_else(|| cp_index.get(&(dep.category.as_str(), dep.package.as_str())))
        .copied()
}
