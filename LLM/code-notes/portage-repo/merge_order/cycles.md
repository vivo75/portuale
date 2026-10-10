# crates/portage-repo/src/merge_order/cycles.rs
Cycle search and the circular-dependency report.

## struct FindSmallestCycleArgs.ineligible
- #142 S2 F4b: nodes temporarily ineligible for harvesting — owners waiting on their unadmitted removal (Portage's validation edge keeps them out of the leaf set, so Portage's cycle search never sees them either). Empty everywhere else, including all unit tests.

## fn find_smallest_cycle
- Ports Portage's `find_smallest_cycle`: the smallest `gather_deps` closure among the currently-mergeable nodes, searched from the lowest `ignore_priority` rung upward so as few dependencies as possible are relaxed. "In the case of multiple runtime cycles, where some cycles may depend on smaller independent cycles, it's optimal to merge smaller independent cycles before other cycles that depend on them."
- Sorts nodes for deterministic results -- Portage's `sorted(nodes)`, i.e. `Package.__lt__`: `cp` then version.
- "Exit this loop with the lowest possible priority, which minimizes the use of installed packages to break cycles."

## fn harvest_cycle
- Ports Portage's `_serialize_tasks`' own final block for a multi-node cycle: harvest one node at a time from the induced subgraph, always taking a leaf at the lowest `ignore_priority` that produces one, and preferring an installed leaf "in order to avoid merging something too early as in bug 917259".
- Fallback: Portage's `leaves = [cycle_digraph.order[-1]]`.

## fn elementary_cycles
- Ports Portage's `depgraph._serialize_tasks`' own selection loop, restricted to what a `--pretend` merge graph contains: no `Uninstall` tasks, no blocker nodes (portuale renders blockers separately), and therefore none of Portage's `myblocker_uninstalls` scheduling. What remains is the whole of Portage's leaf-selection machinery: the `NONE .. MEDIUM_SOFT` `ignore_priority` ladder, with Portage's "greedily pop all of these nodes since no relationship has been ignored" batch at rung `NONE` and one-node-at-a-time selection (preferring a node that actually has a parent) at every relaxed rung; `asap_nodes` -- Portage's `PDEPEND`-promotion path (bug #180045): every unsatisfied `runtime_post` child freed by a relaxed-rung selection is queued to be merged as soon as it becomes a leaf, ahead of the ordinary bias order, and is scanned under `DepPrioritySatisfiedRange`; `find_smallest_cycle` for a genuine runtime cycle, with the `drop_satisfied` escalation to `DepPrioritySatisfiedRange` that lets an already-installed dependency break one.
- Ports Portage's `portage.util.digraph.get_cycles` (+ `shortest_path` + `bfs`): every node's shortest child→node paths (all ties), each a recorded cycle. Portage feeds this the stuck remainder with the `medium_soft` rung and counts the records (`large_cycle_count = len(cycles) > 3`); rotations of one ring count separately (a square records four), which is exactly what makes the count a "lot of cycles" signal rather than a ring census.
- `ig` is the survival filter (Portage's `ignore_priority`); pass `None` for the unfiltered graph. Nodes with no path back to themselves contribute nothing. Only the count and the member union are consumed downstream, so traversal order is fixed (sorted indices) for determinism -- Portage keeps every tie, which makes its result set order-independent too (same reasoning as its PYTHONHASHSEED comment).

## fn shortest_path (in elementary_cycles)
- BFS shortest path, Portage's `digraph.shortest_path`: first visit wins per node (BFS order), returned on first reaching `end`. `None` when `end` is unreachable (Portage raises `KeyError` for unknown nodes instead -- every node here is known by construction).

## fn reduced_merge_order
- Ports Portage's `circular_dependency_handler._prepare_reduced_merge_list` over an explicit drain set: leaf-drain (no filter, like Portage's plain `leaf_nodes()`), falling back to the lowest-order remaining node when nothing is a leaf. Portage drains its whole stuck remainder -- cycle members plus everything left unscheduled downstream of them; the caller (`cycle_report`) passes exactly that set: members plus transitive requirers. A drained child frees its parents (checked against the shrinking remainder, not the static set), matching Portage's shrinking copy.
- A node is a leaf when it has no still-remaining child in the drain set -- self-edges count, exactly like Portage's plain `leaf_nodes()` on its shrinking copy (an unsatisfied buildtime self-loop is the only self-edge `build_digraph` keeps). Checking a static set instead would pin every ring member forever, since a drained child must free its parents.
- Fallback: Portage's `node = tempgraph.order[0]` -- lowest-order remaining node in insertion order (the fallback only fires inside a ring, where every remaining node has a remaining child). Backlog #245: a cp that lives in both roots has its running-root (BDEPEND build-tool) instance inserted after the target-root one in Portage's graph, so among the remaining nodes the target-root ones go first; within a root the scheduler's own insertion order stands (`g.order`). `late` is `GraphEntry::targets_running_root` per node (empty = all target-rooted, the single-root case, which changes nothing).

## fn satisfied_medium_soft_rung
- The `medium_soft` rung of the satisfied range -- the filter Portage's cycle handler passes to `get_cycles`. (Kept as an accessor because `PriorityRange`/`SATISFIED` stay private to this module.)

## fn cycle_report
- Cycle report over a freshly built scheduling graph: all elementary cycles (entry indices) plus the reduced display order. The drain set is the cycle members plus everything transitively requiring them -- Portage drains its whole stuck remainder (members plus everything left unscheduled downstream); portuale schedules everything, so the downstream cone is re-derived here from `required_by` (cp-level, like everywhere else). Built on demand -- callers only pay for it when a hard cycle was already reported (the only path that consumes either number). `top_level_atoms`/`root` feed `build_digraph` exactly as `serialize_merge_order` passes them.
