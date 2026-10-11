# crates/portage-repo/src/merge_order/digraph.rs
The merge-order digraph and the serialize frontier.

## struct Digraph
- A `portage.util.digraph` restricted to what `_serialize_tasks` reads: per-node child/parent adjacency with a priority list per edge, plus `order` (the sequence nodes were added in, which Portage's own `leaf_nodes()` iterates and `_merge_order_bias` re-sorts in place).
- Node `i` is `entries[i]`; `children[i]` = `(child, priorities)`, in edge-creation order.
- `installed`: Portage's `node.installed` -- a "nomerge" node, present only to constrain and inform merge order, never emitted.

## impl Digraph::child_nodes
- Ports Portage's `digraph.child_nodes(node, ignore_priority)`: a child counts when at least one of the edge's priorities survives the filter.

## impl Digraph::leaf_nodes
- Ports Portage's `digraph.leaf_nodes(ignore_priority)` -- every alive leaf, in `order` sequence.

## struct SerializeFrontier
- Ports Portage's `_SerializeFrontier`: per-node, per-level surviving-child counts plus per-level ready heaps. A node is a leaf under level L iff `surv[node][L] == 0`.
- Level -> filter (`None` at level 0: a leaf iff it has no children at all). The deduped union of both `PriorityRange` ladders, so every filter the loop can ask about has a level.
- Node -> order-index; order-index -> node. Assigned once at build from `order`; never changes afterwards.
- The filter union both ladders query: `None` first, then every rung of `NORMAL` and `SATISFIED` in ladder order, deduplicated (Portage deduplicates by object identity too).
- Seed each level's ready heap with its initial leaves, in order-index order -- ascending pushes need no heapify.

## impl SerializeFrontier::compute_mask
- Bitmask of the levels whose filter an edge with `prios` survives. Level 0 (`None`) is always set; for level L > 0 the edge survives iff some priority passes the filter -- exactly `Digraph::is_leaf`'s per-edge test, as in Portage's `_compute_mask`.

## impl SerializeFrontier::level_of
- Level index for a filter, or `None` if untracked (every filter the loop uses is tracked; the fallback is defensive, mirroring Portage's `level_of` returning `None`).

## impl SerializeFrontier::ready_nodes
- The alive leaves under `level`, in `order` sequence -- the frontier equivalent of `Digraph::leaf_nodes`. Drains the level's heap, discarding stale entries (dead nodes and nodes that stopped being leaves), and rebuilds it from the survivors; entries pop in ascending order-index order, so the result is in `order` and the rebuilt heap needs no heapify. `alive` is the graph's liveness vector (nodes are never deleted here, only flagged, unlike Portage's dict-keyed graph).

## impl SerializeFrontier::remove
- Account for `node` leaving the graph. `children`/`parents` are its adjacency as it stands (only the adjacency and the masks matter here, so call before or after flipping `alive`). Each removed edge decrements the parent's per-level counts wherever the edge had survived; a count hitting zero pushes the parent's index (lazily validated on pop). A removed node's own counts are poisoned so `is_leaf` stays false for it, mirroring Portage's `surv.pop`.

## impl SerializeFrontier::add_edge
- Account for `digraph.add(node, parent, priority)`: an edge from `parent` to `node` whose priority list just grew to `priorities`. Adding a priority only makes an edge survive more levels, so the parent's counts only increase; a parent leaving a level's ready set is handled lazily on pop. Not exercised by the selection loop (which never adds edges); pinned by unit test for parity with Portage's mutation surface.

## fn frontier_enabled
- Ports Portage's `PORTAGE_SERIALIZE_FRONTIER_DISABLE`: fall back to the plain `leaf_nodes()`/`is_leaf()` scans (frontier never built). An escape hatch for debugging and for A/B perf comparison, same as upstream.

## fn leaves_via / is_leaf_via
- `Digraph::leaf_nodes` / `Digraph::is_leaf` through the frontier when built, plain scan otherwise. The two agree by construction (same order, same filter); the equivalence unit tests pin it.
