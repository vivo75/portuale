//! The merge-order digraph and the serialize frontier.

use crate::merge_order::*;

// ---------------------------------------------------------------------
// the digraph
// ---------------------------------------------------------------------

/// A `portage.util.digraph` restricted to what `_serialize_tasks` reads:
/// per-node child/parent adjacency with a priority *list* per edge, plus
/// `order` (the sequence nodes were added in, which real's own
/// `leaf_nodes()` iterates and `_merge_order_bias` re-sorts in place).
pub(crate) struct Digraph {
    /// Node `i` is `entries[i]`.
    pub(crate) n: usize,
    /// `children[i]` = `(child, priorities)`, in edge-creation order.
    pub(crate) children: Vec<Vec<(usize, Vec<DepPriority>)>>,
    pub(crate) parents: Vec<Vec<usize>>,
    pub(crate) order: Vec<usize>,
    /// Real `node.installed` -- a "nomerge" node, present only to
    /// constrain and inform merge order, never emitted.
    pub(crate) installed: Vec<bool>,
    pub(crate) alive: Vec<bool>,
}

impl Digraph {
    /// Real `digraph.child_nodes(node, ignore_priority)`: a child counts
    /// when *at least one* of the edge's priorities survives the filter.
    pub(crate) fn child_nodes(&self, i: usize, ig: Option<Ignore>) -> Vec<usize> {
        self.children[i]
            .iter()
            .filter(|(c, prios)| {
                self.alive[*c] && ig.is_none_or(|f| prios.iter().any(|p| !f.ignores(p)))
            })
            .map(|(c, _)| *c)
            .collect()
    }

    pub(crate) fn is_leaf(&self, i: usize, ig: Option<Ignore>) -> bool {
        !self.children[i].iter().any(|(c, prios)| {
            self.alive[*c] && ig.is_none_or(|f| prios.iter().any(|p| !f.ignores(p)))
        })
    }

    pub(crate) fn has_parents(&self, i: usize) -> bool {
        self.parents[i].iter().any(|&p| self.alive[p])
    }

    /// Real `digraph.leaf_nodes(ignore_priority)` -- every alive leaf, in
    /// `order` sequence.
    pub(crate) fn leaf_nodes(&self, ig: Option<Ignore>) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&i| self.alive[i] && self.is_leaf(i, ig))
            .collect()
    }

    pub(crate) fn add_edge(&mut self, parent: usize, child: usize, priority: DepPriority) {
        if let Some(slot) = self.children[parent].iter_mut().find(|(c, _)| *c == child) {
            if !slot.1.contains(&priority) {
                slot.1.push(priority);
            }
            return;
        }
        self.children[parent].push((child, vec![priority]));
        self.parents[child].push(parent);
    }
}

/// Real `_SerializeFrontier`: per-node, per-level surviving-child counts
/// plus per-level ready heaps. A node is a leaf under level L iff
/// `surv[node][L] == 0`.
pub(crate) struct SerializeFrontier {
    /// Level -> filter (`None` at level 0: a leaf iff it has no children
    /// at all). The deduped union of both `PriorityRange` ladders, so
    /// every filter the loop can ask about has a level.
    pub(crate) levels: Vec<Option<Ignore>>,
    /// Per-node surviving-child count per level.
    pub(crate) surv: Vec<Vec<u32>>,
    /// `(parent, child)` -> survival bitmask over levels.
    pub(crate) edge_mask: HashMap<(usize, usize), u64>,
    /// Node -> order-index; order-index -> node. Assigned once at build
    /// from `order`; never changes afterwards.
    pub(crate) index: Vec<usize>,
    pub(crate) by_index: Vec<usize>,
    /// Per-level min-heap of order-indices currently believed leaf.
    pub(crate) ready: Vec<BinaryHeap<Reverse<usize>>>,
}

impl SerializeFrontier {
    /// The filter union both ladders query: `None` first, then every
    /// rung of `NORMAL` and `SATISFIED` in ladder order, deduplicated
    /// (Portage deduplicates by object identity too).
    pub(crate) fn build_levels() -> Vec<Option<Ignore>> {
        let mut levels: Vec<Option<Ignore>> = vec![None];
        for f in NORMAL
            .ignore
            .iter()
            .chain(SATISFIED.ignore.iter())
            .copied()
            .flatten()
        {
            if !levels.contains(&Some(f)) {
                levels.push(Some(f));
            }
        }
        levels
    }

    pub(crate) fn build(g: &Digraph) -> Self {
        let levels = Self::build_levels();
        debug_assert!(levels.len() <= 64, "edge masks are u64");
        let nlevels = levels.len();
        let n = g.n;
        let mut index = vec![0usize; n];
        let mut by_index = vec![0usize; g.order.len()];
        for (pos, &node) in g.order.iter().enumerate() {
            index[node] = pos;
            by_index[pos] = node;
        }
        let mut edge_mask: HashMap<(usize, usize), u64> = HashMap::new();
        let mut surv = vec![vec![0u32; nlevels]; n];
        for &node in &g.order {
            for (child, prios) in &g.children[node] {
                let mask = Self::compute_mask(&levels, prios);
                edge_mask.insert((node, *child), mask);
                let mut m = mask;
                while m != 0 {
                    let l = m.trailing_zeros() as usize;
                    surv[node][l] += 1;
                    m &= m - 1;
                }
            }
        }
        // Seed each level's ready heap with its initial leaves, in
        // order-index order -- ascending pushes need no heapify.
        let mut ready: Vec<BinaryHeap<Reverse<usize>>> =
            (0..nlevels).map(|_| BinaryHeap::new()).collect();
        for &node in &g.order {
            let idx = index[node];
            for l in 0..nlevels {
                if surv[node][l] == 0 {
                    ready[l].push(Reverse(idx));
                }
            }
        }
        Self {
            levels,
            surv,
            edge_mask,
            index,
            by_index,
            ready,
        }
    }

    /// Bitmask of the levels whose filter an edge with `prios`
    /// survives. Level 0 (`None`) is always set; for level L > 0 the
    /// edge survives iff some priority passes the filter -- exactly
    /// `Digraph::is_leaf`'s per-edge test, as in real `_compute_mask`.
    pub(crate) fn compute_mask(levels: &[Option<Ignore>], prios: &[DepPriority]) -> u64 {
        let mut mask = 1u64;
        for (l, f) in levels.iter().enumerate().skip(1) {
            let f = f.expect("levels past 0 always hold a filter");
            if prios.iter().any(|p| !f.ignores(p)) {
                mask |= 1 << l;
            }
        }
        mask
    }

    /// Level index for a filter, or `None` if untracked (every filter
    /// the loop uses is tracked; the fallback is defensive, mirroring
    /// real's `level_of` returning `None`).
    pub(crate) fn level_of(&self, ig: Option<Ignore>) -> Option<usize> {
        self.levels.iter().position(|&f| f == ig)
    }

    pub(crate) fn is_leaf(&self, node: usize, level: usize) -> bool {
        self.surv.get(node).is_some_and(|counts| counts[level] == 0)
    }

    /// The alive leaves under `level`, in `order` sequence -- the
    /// frontier equivalent of `Digraph::leaf_nodes`. Drains the level's
    /// heap, discarding stale entries (dead nodes and nodes that
    /// stopped being leaves), and rebuilds it from the survivors;
    /// entries pop in ascending order-index order, so the result is in
    /// `order` and the rebuilt heap needs no heapify. `alive` is the
    /// graph's liveness vector (nodes are never deleted here, only
    /// flagged, unlike real's dict-keyed graph).
    pub(crate) fn ready_nodes(&mut self, level: usize, alive: &[bool]) -> Vec<usize> {
        let mut result: Vec<usize> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        while let Some(Reverse(idx)) = self.ready[level].pop() {
            if !seen.insert(idx) {
                continue;
            }
            let Some(&node) = self.by_index.get(idx) else {
                continue;
            };
            if !alive.get(node).copied().unwrap_or(false) {
                continue;
            }
            if !self.is_leaf(node, level) {
                continue;
            }
            result.push(node);
        }
        self.ready[level] = BinaryHeap::from(
            result
                .iter()
                .map(|&node| Reverse(self.index[node]))
                .collect::<Vec<_>>(),
        );
        result
    }

    /// Account for `node` leaving the graph. `children`/`parents` are
    /// its adjacency as it stands (only the adjacency and the masks
    /// matter here, so call before or after flipping `alive`). Each
    /// removed edge decrements the parent's per-level counts wherever
    /// the edge had survived; a count hitting zero pushes the parent's
    /// index (lazily validated on pop). A removed node's own counts are
    /// poisoned so `is_leaf` stays false for it, mirroring real's
    /// `surv.pop`.
    pub(crate) fn remove(
        &mut self,
        node: usize,
        children: &[(usize, Vec<DepPriority>)],
        parents: &[usize],
    ) {
        for &parent in parents {
            let Some(mask) = self.edge_mask.remove(&(parent, node)) else {
                continue;
            };
            let Some(pcounts) = self.surv.get_mut(parent) else {
                continue;
            };
            let Some(&pidx) = self.index.get(parent) else {
                continue;
            };
            let mut m = mask;
            while m != 0 {
                let l = m.trailing_zeros() as usize;
                pcounts[l] = pcounts[l].saturating_sub(1);
                if pcounts[l] == 0 {
                    self.ready[l].push(Reverse(pidx));
                }
                m &= m - 1;
            }
        }
        for (child, _) in children {
            self.edge_mask.remove(&(node, *child));
        }
        if let Some(counts) = self.surv.get_mut(node) {
            counts.fill(u32::MAX);
        }
    }

    /// Account for `digraph.add(node, parent, priority)`: an edge from
    /// `parent` to `node` whose priority list just grew to `priorities`.
    /// Adding a priority only makes an edge survive more levels, so the
    /// parent's counts only increase; a parent leaving a level's ready
    /// set is handled lazily on pop. Not exercised by the selection
    /// loop (which never adds edges); pinned by unit test for parity
    /// with real's mutation surface.
    #[allow(dead_code)]
    pub(crate) fn add_edge(
        &mut self,
        node: usize,
        parent: Option<usize>,
        priorities: &[DepPriority],
    ) {
        let ensure = |slf: &mut Self, n: usize| {
            if n >= slf.surv.len() {
                let idx = slf.by_index.len();
                slf.by_index.push(n);
                slf.index.resize(n + 1, 0);
                slf.index[n] = idx;
                slf.surv.resize(n + 1, vec![0u32; slf.levels.len()]);
                for heap in slf.ready.iter_mut() {
                    heap.push(Reverse(idx));
                }
            }
        };
        ensure(self, node);
        let Some(parent) = parent else { return };
        ensure(self, parent);
        let new_mask = Self::compute_mask(&self.levels, priorities);
        let old_mask = self.edge_mask.get(&(parent, node)).copied().unwrap_or(0);
        if new_mask == old_mask {
            return;
        }
        self.edge_mask.insert((parent, node), new_mask);
        let mut delta = new_mask & !old_mask;
        while delta != 0 {
            let l = delta.trailing_zeros() as usize;
            self.surv[parent][l] += 1;
            delta &= delta - 1;
        }
    }
}

/// Real `PORTAGE_SERIALIZE_FRONTIER_DISABLE`: fall back to the plain
/// `leaf_nodes()`/`is_leaf()` scans (frontier never built). An escape
/// hatch for debugging and for A/B perf comparison, same as upstream.
pub(crate) fn frontier_enabled() -> bool {
    std::env::var_os("PORTAGE_SERIALIZE_FRONTIER_DISABLE").is_none()
}

/// `Digraph::leaf_nodes` through the frontier when built, plain scan
/// otherwise. The two agree by construction (same order, same filter);
/// the equivalence unit tests pin it.
pub(crate) fn leaves_via(
    frontier: Option<&mut SerializeFrontier>,
    g: &Digraph,
    ig: Option<Ignore>,
) -> Vec<usize> {
    match frontier {
        Some(fr) => match fr.level_of(ig) {
            Some(level) => fr
                .ready_nodes(level, &g.alive)
                .into_iter()
                .filter(|&i| g.alive[i])
                .collect(),
            None => g.leaf_nodes(ig),
        },
        None => g.leaf_nodes(ig),
    }
}

/// `Digraph::is_leaf` through the frontier when built, direct check
/// otherwise.
pub(crate) fn is_leaf_via(
    frontier: Option<&SerializeFrontier>,
    g: &Digraph,
    node: usize,
    ig: Option<Ignore>,
) -> bool {
    match frontier {
        Some(fr) => match fr.level_of(ig) {
            Some(level) => fr.is_leaf(node, level),
            None => g.is_leaf(node, ig),
        },
        None => g.is_leaf(node, ig),
    }
}
