//! The merge-order digraph and the serialize frontier.

use crate::merge_order::*;

// ---------------------------------------------------------------------
// the digraph
// ---------------------------------------------------------------------

/// A `portage.util.digraph` restricted to what `_serialize_tasks` reads:
/// per-node child/parent adjacency with a priority list per edge, plus `order`.
/// Portage: portage/util/digraph.py
pub(crate) struct Digraph {
    /// Node `i` is `entries[i]`.
    pub(crate) n: usize,
    /// `children[i]` = `(child, priorities)`, in edge-creation order.
    pub(crate) children: Vec<Vec<(usize, Vec<DepPriority>)>>,
    pub(crate) parents: Vec<Vec<usize>>,
    pub(crate) order: Vec<usize>,
    /// A "nomerge" node: constrains merge order, never emitted.
    pub(crate) installed: Vec<bool>,
    pub(crate) alive: Vec<bool>,
}

impl Digraph {
    /// A child counts when at least one of the edge's priorities survives the filter.
    /// Portage: portage/util/digraph.py, child_nodes
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

    /// Every alive leaf, in `order` sequence.
    /// Portage: portage/util/digraph.py, leaf_nodes
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

/// Per-node, per-level surviving-child counts plus per-level ready heaps.
/// A node is a leaf under level L iff `surv[node][L] == 0`.
/// Portage: _serialize_frontier.py, _SerializeFrontier
pub(crate) struct SerializeFrontier {
    /// Level -> filter (`None` at level 0). The deduped union of both ladders.
    pub(crate) levels: Vec<Option<Ignore>>,
    /// Per-node surviving-child count per level.
    pub(crate) surv: Vec<Vec<u32>>,
    /// `(parent, child)` -> survival bitmask over levels.
    pub(crate) edge_mask: HashMap<(usize, usize), u64>,
    /// Node -> order-index; order-index -> node. Assigned once at build.
    pub(crate) index: Vec<usize>,
    pub(crate) by_index: Vec<usize>,
    /// Per-level min-heap of order-indices currently believed leaf.
    pub(crate) ready: Vec<BinaryHeap<Reverse<usize>>>,
}

/// Fall back to plain `leaf_nodes()`/`is_leaf()` scans when
/// `PORTAGE_SERIALIZE_FRONTIER_DISABLE` is set (debugging escape hatch).
/// Portage: _serialize_frontier.py
pub(crate) fn frontier_enabled() -> bool {
    std::env::var_os("PORTAGE_SERIALIZE_FRONTIER_DISABLE").is_none()
}
