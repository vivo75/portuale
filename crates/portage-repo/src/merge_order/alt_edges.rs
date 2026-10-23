//! Which `||` branches contribute edges.

use crate::merge_order::*;
use crate::parse_dep_atom;

/// Per `(key, group)`, the `deps` indices of the `||` branches not picked.
///
/// Picks the first branch (written order) all of whose atoms match a
/// merge-bound node, else an installed entry, else anything; nothing is
/// suppressed when no branch fully resolves.
/// Portage: dep_check.py, dep_zapdeps
pub(crate) fn suppressed_alt_edges(
    entries: &[GraphEntry],
    pre: &DigraphPrelude<'_>,
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> Vec<HashSet<usize>> {
    let installed_by_cp = installed_candidates_by_cp(root);
    let children_of = |e: &GraphEntry| {
        circular.get(&(
            e.targets_running_root,
            e.category.clone(),
            e.package.clone(),
        ))
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let children = children_of(e);
            let mut groups: HashMap<(u8, u32), Vec<(u32, usize)>> = HashMap::new();
            for (ei, edge) in e.deps.iter().enumerate() {
                if let Some((g, b)) = edge.alt
                    && g != u32::MAX
                {
                    groups.entry((edge.key, g)).or_default().push((b, ei));
                }
            }
            let mut suppressed = HashSet::new();
            for members in groups.values() {
                let mut all_graph: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_inst: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_any: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                // Demote branches matching an in-cycle child, so the kept branch
                // stays the one the walk took instead of re-closing the cycle.
                let mut demoted: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                for &(b, ei) in members {
                    let edge = &e.deps[ei];
                    let (mut graph_m, mut inst_m, mut any_m) = (false, false, false);
                    if let Some(idxs) = pre
                        .cp_indices
                        .get(&(edge.category.as_str(), edge.package.as_str()))
                    {
                        for &j in idxs {
                            // Skip the owner's own not-yet-merged node: a self-match
                            // would add a phantom self-edge that stalls its position.
                            // Portage: dep_check.py, dep_zapdeps (circular_self)
                            if j == i && !pre.installed[j] {
                                continue;
                            }
                            if pre.edge_matches(&edge.atom, j) {
                                any_m = true;
                                if pre.installed[j] {
                                    inst_m = true;
                                } else {
                                    graph_m = true;
                                }
                            }
                        }
                    }
                    *all_graph.entry(b).or_insert(true) &= graph_m;
                    *all_inst.entry(b).or_insert(true) &= inst_m;
                    *all_any.entry(b).or_insert(true) &= any_m;
                    if let Some(children) = children
                        && let Some(atom) = parse_dep_atom(&edge.atom, "alt_edges_edge_atom")
                        && atom.blocker == portage_dep::Blocker::None
                        && !installed_by_cp
                            .get(&(atom.category.clone(), atom.package.clone()))
                            .is_some_and(|cands| {
                                let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
                                portage_dep::match_from_list(&edge.atom, &refs)
                                    .is_some_and(|m| !m.is_empty())
                            })
                        && children.iter().any(|child| {
                            let candidate = crate::circular_child_candidate(child);
                            portage_dep::match_from_list(&edge.atom, &[candidate.as_str()])
                                .is_some_and(|m| !m.is_empty())
                        })
                    {
                        demoted.insert(b, true);
                    }
                }
                let is_demoted = |b: &u32| demoted.get(b).is_some_and(|d| *d);
                let rank = |tiers: &std::collections::BTreeMap<u32, bool>| {
                    tiers
                        .iter()
                        .filter(|(b, _)| !is_demoted(b))
                        .find(|(_, v)| **v)
                        .map(|(b, _)| *b)
                };
                let pick = rank(&all_graph)
                    .or_else(|| rank(&all_inst))
                    .or_else(|| rank(&all_any))
                    // Every branch demoted: fall back to the first-listed one.
                    .or_else(|| {
                        members
                            .iter()
                            .map(|(b, _)| *b)
                            .filter(|b| is_demoted(b))
                            .min()
                    });
                if let Some(pick) = pick {
                    for &(b, ei) in members {
                        if b != pick {
                            suppressed.insert(ei);
                        }
                    }
                }
            }
            suppressed
        })
        .collect()
}

/// The `deps` indices of every `||` alternative this run kept, per entry.
///
/// Feeds the satisfied-blocker wait predicate; shares `suppressed_alt_edges`
/// with the digraph build so the self-branch exclusion cannot drift.
pub fn kept_alt_branches(
    entries: &[GraphEntry],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> Vec<HashSet<usize>> {
    let pre = digraph_prelude(entries, root);
    let suppressed = suppressed_alt_edges(entries, &pre, root, circular);
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.deps
                .iter()
                .enumerate()
                .filter(|(ei, edge)| edge.alt.is_some() && !suppressed[i].contains(ei))
                .map(|(ei, _)| ei)
                .collect()
        })
        .collect()
}

/// For every entry, the entry index each `deps` edge resolves to (`None` when
/// unresolvable, self-pointing, or on a suppressed `||` branch).
///
/// Shares `DigraphPrelude::select_dep_target` with `build_digraph` so the
/// ranking cannot drift between the scheduling graph and the tree display.
/// Portage: depgraph.py, _create_graph
pub fn resolved_dep_targets(
    entries: &[GraphEntry],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> Vec<Vec<Option<usize>>> {
    let pre = digraph_prelude(entries, root);
    let suppressed = suppressed_alt_edges(entries, &pre, root, circular);
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.deps
                .iter()
                .enumerate()
                .map(|(ei, _edge)| {
                    if suppressed[i].contains(&ei) {
                        return None;
                    }
                    pre.select_dep_target(SelectDepTargetArgs {
                        entries,
                        from: i,
                        from_deps: &e.deps,
                        ei,
                        suppressed: &suppressed[i],
                        merge_bound_only: true,
                    })
                })
                .collect()
        })
        .collect()
}
