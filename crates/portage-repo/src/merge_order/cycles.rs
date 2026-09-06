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

/// The `--pretend` subset of Portage's `_serialize_tasks` leaf-selection loop:
/// the `ignore_priority` ladder, `asap_nodes`, `find_smallest_cycle` and the
/// `drop_satisfied` escalation.
/// Portage: depgraph.py, _serialize_tasks
///
/// Every node's shortest child-to-node paths (all ties), each a recorded cycle.
/// Portage: portage.util.digraph, get_cycles
///
/// `ig` is the survival filter; pass `None` for the unfiltered graph.
/// Traversal order is fixed (sorted indices) for determinism.
pub(crate) fn elementary_cycles(g: &Digraph, ig: Option<Ignore>) -> Vec<Vec<usize>> {
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
    for &node in &g.order {
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
/// Portage: circular_dependency_handler, _prepare_reduced_merge_list
pub(crate) fn reduced_merge_order(
    g: &Digraph,
    drain: &HashSet<usize>,
    late: &[bool],
) -> Vec<usize> {
    // A node is a leaf when it has no still-remaining child in the drain set.
    let mut remaining: HashSet<usize> = drain.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = g
            .order
            .iter()
            .copied()
            .filter(|i| {
                remaining.contains(i) && !g.children[*i].iter().any(|(c, _)| remaining.contains(c))
            })
            .collect();
        if leaves.is_empty() {
            // Lowest-order remaining node; target-rooted instances go first.
            let pos: HashMap<usize, usize> =
                g.order.iter().enumerate().map(|(p, &i)| (i, p)).collect();
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
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let g = build_digraph(entries, top_level_atoms, root, circular);
    let cycles = elementary_cycles(&g, satisfied_medium_soft_rung());
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
