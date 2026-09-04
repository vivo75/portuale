//! Cycle search and the circular-dependency report.

use crate::merge_order::*;

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

/// The `medium_soft` rung of the satisfied range (the cycle handler's filter).
pub(crate) fn satisfied_medium_soft_rung() -> Option<Ignore> {
    SATISFIED.ig_medium_soft()
}
