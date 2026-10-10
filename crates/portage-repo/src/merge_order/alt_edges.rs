//! Which `||` branches contribute edges.

use crate::merge_order::*;

/// Real `dep_zapdeps` (`dep_check.py`): a `|| ( … )` group resolves to
/// one alternative, not all. Portuale keeps every branch's atoms in
/// `GraphEntry::deps` and picks here, per `(key, group)`, following
/// real's `choice_bins` ordering: the first branch (written order)
/// *all* of whose atoms already match a merge-bound graph node
/// (`preferred_in_graph`, and real's line-793 promotion of the
/// all-in-graph choice ahead of an all-installed one in the same bin),
/// else the first all of whose atoms match an installed entry
/// (`preferred_installed`), else the first all of whose atoms match
/// anything at all. Returns, per entry, the `deps` indices of the
/// branches **not** picked. If *no* branch fully resolves, nothing is
/// suppressed (keep the over-inclusive stopgap).
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
                // Per branch: does *every* atom match a merge-bound node
                // / an installed entry / anything?
                let mut all_graph: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_inst: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                let mut all_any: std::collections::BTreeMap<u32, bool> =
                    std::collections::BTreeMap::new();
                // Backlog #221: per branch, real `dep_zapdeps`'
                // `circular_atom` demotion at this re-derivation's
                // coarse level -- a branch with an atom matching one of
                // the owner's recorded in-cycle children ranks below
                // every non-demoted tier, so the kept branch stays the
                // one the walk took instead of re-closing the cycle with
                // a phantom edge. Blocker atoms never match; the
                // installed exemption is version/slot-only here (no
                // `[use]` check): the full `atom_matches_installed`
                // rule needs `Config`, which no suppression signature
                // carries, for a USE-mismatch corner no fixture
                // reaches. Empty map (every pre-#221 call shape): no
                // branch demotes, byte-identical behaviour.
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
                            // Real `dep_zapdeps` treats a `||` alternative
                            // satisfied only by the package currently
                            // being resolved as unavailable -- a circular
                            // self-dependency (the `circular_self` check
                            // in lib.rs's actual resolve, b5b256f, L0
                            // finding D). Without this guard, an
                            // alternative whose only tree match is the
                            // owner's own not-yet-merged graph node (`j
                            // == i`) trivially counts as "in graph" and
                            // can win over the branch the resolver
                            // actually chose (e.g. a bootstrap branch),
                            // adding a phantom self-edge that can never
                            // be satisfied and stalls the owner's own
                            // merge-order position (#53).
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
                        && let Some(atom) = portage_dep::parse_atom(&edge.atom)
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
                    // Real's `other` bin keeps ebuild order: a group
                    // with every branch demoted still resolves, to its
                    // first-listed branch (`dep_check.py:392-401` puts
                    // `other` last in `choice_bins`, and the final
                    // selection at `:804-808` returns the first
                    // `all_available` choice of the first non-empty
                    // bin). Residue: the in-bin promotion pass
                    // (`:738-802`, upgrade/in-graph) also runs over
                    // `other`, so two demoted branches with different
                    // versions could resolve in non-ebuild order while
                    // this keeps the first-listed one -- the walk's own
                    // `promote_tied_alternative` tie-break fires there
                    // instead. No fixture shapes it (demoted branches
                    // of one group match the same recorded child at the
                    // same version in every cycle fixture).
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

/// #76 B1: the `GraphEntry::deps` indices of every `|| ( … )` alternative
/// this run **kept** -- the branch `dep_zapdeps` resolved the group to,
/// per `suppressed_alt_edges`' real `choice_bins` ranking. Exposed for
/// the satisfied-blocker wait predicate (`pretend.rs::
/// replacement_wait_index`): the serializer runs over the collapsed
/// graph, so a wait chain through a disjunctive edge is a wait exactly
/// when that edge's branch was the kept one (`TEST/findings/l0.md`
/// "#76 B0"). The #53 circular-self-branch exclusion is preserved
/// because both callers share `suppressed_alt_edges`.
///
/// `circular` is backlog #221's settling pass's recorded cycle edges
/// (empty when no backtrack restart happened): `suppressed_alt_edges`
/// demotes matching branches like the walk did, so the kept branch
/// stays the cycle-breaking one.
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

/// #82: for every entry, the entry index each of its `GraphEntry::deps`
/// edges resolves to -- `None` when the atom has no target in this
/// graph, when the target is the entry itself, or when the edge is a
/// `|| ( … )` alternative this run did not keep (`suppressed_alt_edges`,
/// the #76 B1 set, so a caller needs no second `kept_alt_branches` pass).
///
/// Exposed for `pretend.rs::print_tree`, whose tree display needs the
/// same "one package per atom" narrowing real's `_create_graph` does.
/// It shares `DigraphPrelude::select_dep_target` with `build_digraph`
/// (see that function's doc for the single `merge_bound_only`
/// difference and for why the sharing matters).
///
/// `circular` is backlog #221's settling pass's recorded cycle edges
/// (see `kept_alt_branches`).
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
