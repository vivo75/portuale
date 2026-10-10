//! Lookups over the entries a run will merge.

use crate::*;

/// The version a merge-bound `PretendOutcome` would install, or `None`
/// for `AlreadyInstalled`/`NoVisibleCandidate`/`Uninstall` (which merge
/// nothing). Exactly the four outcomes `PassState::resolved_slots` ever
/// indexes; a #72 B3 removal is deliberately not one of them.
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

/// The `category/package-version` a merge-bound `GraphEntry` would
/// install, or `None` for `AlreadyInstalled`/`NoVisibleCandidate` (which
/// never participate in a build-time cycle).
pub(crate) fn merge_bound_cpv(entry: &GraphEntry) -> Option<String> {
    let version = merge_bound_version(&entry.outcome)?;
    Some(format!("{}/{}-{version}", entry.category, entry.package))
}

/// Lowest-index merge-bound entry per `(category, package)` -- the merge
/// list is already in dependency order, so the first is the instance an
/// edge belongs to. Shared by `find_hard_cycles` (cycle nodes) and
/// `cycle_edge_labels` (owner lookup), so the two cannot disagree on
/// which instance an edge belongs to. (It no longer picks the rendered
/// start node: since backlog #278 that follows real's
/// `shortest_cycle[0]`, i.e. `discovery` order.)
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

/// Track X Slice C (#242): merge-bound entry indices keyed by
/// [`RootCp`] -- first index per (root, cp). `merge_bound_index` stays
/// the cp-level view (render order starts at the first scheduled
/// instance); this is the edge-resolution view (real's package-keyed
/// digraph never merges two roots' instances).
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

/// Track X Slice C (#242): resolve one root-qualified map endpoint to
/// its entry index -- the exact-root instance first, falling back to the
/// cp-level first index (the pre-slice behaviour) when this graph holds
/// no such instance in that root (same-root graphs, synthetic entries,
/// the `--root-deps` sidecar shapes the walk never root-splits).
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

/// Track X Slice C (#242): resolve a `deps` edge to its child instance
/// -- the resolving-root instance (`dep_child_root` from the owner's own
/// root and the edge's dep key) when one is merge-bound, else the legacy
/// cp-level first index. The walk always resolved the atom in the
/// resolving root, so the filter only ever removes a provably-wrong
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
