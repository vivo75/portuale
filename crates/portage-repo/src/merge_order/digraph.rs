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
