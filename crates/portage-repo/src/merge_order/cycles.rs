//! Cycle search and the circular-dependency report.

use crate::merge_order::*;

/// Arguments of [`find_smallest_cycle`].
pub(crate) struct FindSmallestCycleArgs<'a> {
    pub(crate) g: &'a Digraph,
    pub(crate) frontier: Option<&'a mut SerializeFrontier>,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) range: &'a PriorityRange,
    pub(crate) asap: &'a [usize],
    pub(crate) prefer_asap: bool,
    /// Nodes temporarily ineligible for harvesting (owners waiting on their
    /// unadmitted removal); empty everywhere else, including all unit tests.
    pub(crate) ineligible: &'a HashSet<usize>,
}

/// The smallest `gather_deps` closure among the currently-mergeable nodes,
/// searched from the lowest `ignore_priority` rung upward.
/// Portage: depgraph.py, find_smallest_cycle
pub(crate) fn find_smallest_cycle(
    args: FindSmallestCycleArgs<'_>,
) -> Option<(HashSet<usize>, Option<Ignore>)> {
    let FindSmallestCycleArgs {
        g,
        frontier,
        entries,
        range,
        asap,
        prefer_asap,
        ineligible,
    } = args;
    let mergeable: HashSet<usize> = leaves_via(frontier, g, range.ig_medium())
        .into_iter()
        .filter(|i| !ineligible.contains(i))
        .collect();
    if mergeable.is_empty() {
        return None;
    }
    let mut cand: Vec<usize> = if prefer_asap && !asap.is_empty() {
        asap.iter()
            .copied()
            .filter(|i| mergeable.contains(i))
            .collect()
    } else {
        mergeable.iter().copied().collect()
    };
    // Sort nodes for deterministic results (cp, then version).
    cand.sort_by(|&a, &b| {
        (entries[a].category.as_str(), entries[a].package.as_str())
            .cmp(&(entries[b].category.as_str(), entries[b].package.as_str()))
            .then_with(|| {
                let ver = |i: usize| entry_version(&entries[i]).unwrap_or_default().to_string();
                portage_versions::vercmp(&ver(a), &ver(b))
                    .unwrap_or(0)
                    .cmp(&0)
            })
    });
    let mut best: Option<(HashSet<usize>, Option<Ignore>)> = None;
    for i in range.medium_post..=range.medium_soft {
        let ig = range.ig(i);
        for &node in &cand {
            if !g.has_parents(node) {
                continue;
            }
            let Some(cl) = gather_deps(g, node, ig, &mergeable) else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| cl.len() < b.len()) {
                best = Some((cl, ig));
            }
        }
        // Exit with the lowest possible priority.
        if best.is_some() {
            break;
        }
    }
    best
}

/// Harvest one node at a time from the induced subgraph, always taking a leaf
/// at the lowest `ignore_priority` that produces one, preferring an installed leaf.
/// Portage: depgraph.py, _serialize_tasks
pub(crate) fn harvest_cycle(g: &Digraph, sub: &HashSet<usize>) -> Vec<usize> {
    let ladder: Vec<Option<Ignore>> = NORMAL
        .ignore
        .iter()
        .chain(SATISFIED.ignore.iter())
        .filter(|f| f.is_some())
        .copied()
        .collect();
    let mut remaining = sub.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = Vec::new();
        for ig in &ladder {
            leaves = g
                .order
                .iter()
                .copied()
                .filter(|i| {
                    remaining.contains(i)
                        && !g.children[*i].iter().any(|(c, prios)| {
                            remaining.contains(c)
                                && ig.is_none_or(|f| prios.iter().any(|p| !f.ignores(p)))
                        })
                })
                .collect();
            if !leaves.is_empty() {
                break;
            }
        }
        if leaves.is_empty() {
            // Fall back to the last node in `.order` within the subgraph.
            leaves = vec![
                *g.order
                    .iter()
                    .rev()
                    .find(|i| remaining.contains(i))
                    .expect("remaining is non-empty"),
            ];
        }
        let installed_leaves: Vec<usize> =
            leaves.iter().copied().filter(|&i| g.installed[i]).collect();
        let pick = *installed_leaves.first().unwrap_or(&leaves[0]);
        remaining.remove(&pick);
        out.push(pick);
    }
    out
}

/// The `--pretend` subset of Portage's `_serialize_tasks` leaf-selection loop:
/// the `ignore_priority` ladder, `asap_nodes`, `find_smallest_cycle` and the
/// `drop_satisfied` escalation.
/// Portage: depgraph.py, _serialize_tasks
///
/// Every node's shortest child-to-node paths (all ties), each a recorded cycle.
/// Portage: portage.util.digraph, get_cycles
///
/// `ig` is the survival filter; pass `None` for the unfiltered graph.
/// Nodes iterate in `order` (Portage's digraph insertion order); children in
/// edge order. Callers pass the scan order, not necessarily `g.order`.
pub(crate) fn elementary_cycles(
    g: &Digraph,
    ig: Option<Ignore>,
    order: &[usize],
) -> Vec<Vec<usize>> {
    use std::collections::VecDeque;
    /// BFS shortest path: first visit wins per node, returned on reaching `end`.
    fn shortest_path(
        g: &Digraph,
        start: usize,
        end: usize,
        ig: Option<Ignore>,
    ) -> Option<Vec<usize>> {
        let mut paths: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut queue: VecDeque<(Option<usize>, usize)> = VecDeque::from([(None, start)]);
        let mut enqueued: HashSet<usize> = HashSet::from([start]);
        while let Some((parent, n)) = queue.pop_front() {
            let mut path = match parent {
                None => Vec::new(),
                Some(p) => paths.get(&p).cloned().unwrap_or_default(),
            };
            path.push(n);
            paths.insert(n, path);
            if n == end {
                return paths.remove(&end);
            }
            let mut fresh: Vec<usize> = g.children[n]
                .iter()
                .filter(|(c, prios)| {
                    !enqueued.contains(c) && ig.is_none_or(|f| prios.iter().any(|p| !f.ignores(p)))
                })
                .map(|(c, _)| *c)
                .collect();
            fresh.sort_unstable();
            for c in fresh {
                enqueued.insert(c);
                queue.push_back((Some(n), c));
            }
        }
        None
    }

    let mut all: Vec<Vec<usize>> = Vec::new();
    for &node in order {
        // Record every path tied at the shortest length seen for this node.
        let mut min_len: Option<usize> = None;
        let mut cands: Vec<Vec<usize>> = Vec::new();
        for (child, prios) in &g.children[node] {
            if !ig.is_none_or(|f| prios.iter().any(|p| !f.ignores(p))) {
                continue;
            }
            let Some(path) = shortest_path(g, *child, node, ig) else {
                continue;
            };
            if min_len.is_none_or(|m| path.len() <= m) {
                if min_len.is_none_or(|m| path.len() < m) {
                    min_len = Some(path.len());
                }
                cands.push(path);
            }
        }
        if let Some(m) = min_len {
            all.extend(cands.into_iter().filter(|p| p.len() == m));
        }
    }
    all
}

/// Leaf-drain over an explicit drain set, falling back to the lowest-order
/// remaining node when nothing is a leaf.
/// Portage: circular_dependency_handler, _prepare_reduced_merge_list.
/// The drain order is Portage's merge-order bias: nodes sorted by parent
/// count, descending (`depgraph.py`, `_merge_order_bias`; Python's sort is
/// stable, so ties keep insertion order). The bias decides the fallback
/// pick for pure cycles (backlog #517 (b)).
pub(crate) fn reduced_merge_order(
    g: &Digraph,
    drain: &HashSet<usize>,
    late: &[bool],
) -> Vec<usize> {
    // A node is a leaf when it has no still-remaining child in the drain set.
    let mut remaining: HashSet<usize> = drain.clone();
    // Bias order, computed once: parent count, descending, stable over the
    // digraph insertion order (`sort_by_key` is stable).
    let mut biased: Vec<usize> = g.order.clone();
    biased.sort_by_key(|&i| std::cmp::Reverse(g.parents.get(i).map_or(0, Vec::len)));
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = biased
            .iter()
            .copied()
            .filter(|i| {
                remaining.contains(i) && !g.children[*i].iter().any(|(c, _)| remaining.contains(c))
            })
            .collect();
        if leaves.is_empty() {
            // Lowest-order remaining node; target-rooted instances go first.
            let pos: HashMap<usize, usize> =
                biased.iter().enumerate().map(|(p, &i)| (i, p)).collect();
            leaves = remaining
                .iter()
                .copied()
                .min_by_key(|&i| {
                    (
                        late.get(i).copied().unwrap_or(false),
                        pos.get(&i).copied().unwrap_or(usize::MAX),
                    )
                })
                .into_iter()
                .collect();
        }
        for i in leaves {
            remaining.remove(&i);
            out.push(i);
        }
    }
    out
}

/// The `medium_soft` rung of the satisfied range (the cycle handler's filter).
pub(crate) fn satisfied_medium_soft_rung() -> Option<Ignore> {
    SATISFIED.ig_medium_soft()
}

/// All elementary cycles (entry indices) plus the reduced display order.
///
/// The drain set is the cycle members plus everything transitively requiring
/// them. Built on demand: only the hard-cycle report consumes either number.
pub(crate) fn cycle_report(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
    onlydeps: bool,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let g = build_digraph(entries, top_level_atoms, root, circular);
    // Portage's digraph insertion order: DFS pre-order from the targets
    // (backlog #517 (a)); under `--onlydeps` the targets are not inserted
    // up front, so the seeds are their direct children (backlog #518).
    let mut seeds: Vec<usize> = Vec::new();
    for atom in top_level_atoms {
        let Some(parsed) = crate::parse_dep_atom(atom, "cycle_report-targets") else {
            continue;
        };
        let target = entries
            .iter()
            .position(|e| e.category == parsed.category && e.package == parsed.package);
        let Some(t) = target else { continue };
        if onlydeps {
            for (c, _) in &g.children[t] {
                if !seeds.contains(c) {
                    seeds.push(*c);
                }
            }
        }
        if !seeds.contains(&t) {
            seeds.push(t);
        }
    }
    let mut scan: Vec<usize> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        let mut stack: Vec<usize> = Vec::new();
        for &seed in seeds.iter().chain(g.order.iter()) {
            if !seen.insert(seed) {
                continue;
            }
            stack.push(seed);
            while let Some(u) = stack.pop() {
                scan.push(u);
                let mut kids: Vec<usize> = Vec::new();
                for (c, _) in &g.children[u] {
                    if seen.insert(*c) {
                        kids.push(*c);
                    }
                }
                kids.reverse();
                stack.extend(kids);
            }
        }
    }
    let cycles = elementary_cycles(&g, satisfied_medium_soft_rung(), &scan);
    let members: HashSet<usize> = cycles.iter().flatten().copied().collect();
    let mut cp_indices: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        cp_indices
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(i);
    }
    let mut drain: HashSet<usize> = members.clone();
    let mut stack: Vec<usize> = members.iter().copied().collect();
    while let Some(i) = stack.pop() {
        for owner in &entries[i].required_by {
            if let Some(idxs) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str())) {
                for &j in idxs {
                    if drain.insert(j) {
                        stack.push(j);
                    }
                }
            }
        }
    }
    let late: Vec<bool> = entries.iter().map(|e| e.targets_running_root).collect();
    let display = reduced_merge_order(&g, &drain, &late);
    (cycles, display)
}
