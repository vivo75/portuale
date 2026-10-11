# crates/portage-repo/src/tests/entry.rs
Merge-order entry unit tests: cycle breaking, discovery stamps, root attribution.

## fn topological_order_breaks_a_cycle_at_its_run_time_edge
- a's DEPEND is an unsatisfied build-time edge, the one kind Portage's `ignore_priority` ladder can never relax; b's RDEPEND is a run-time edge.
- Portage itself gives up on this shape and raises its internal circular-dependency error; portuale still has to emit a list, and `select_nodes`' fallback prefers the node whose every remaining dependency the widest filter would drop.
- All-buildtime cycle: unbreakable, falls back to bias order; `find_hard_cycles` renders the `* Error: circular dependencies:` block.

## fn topological_merge_order_stamps_discovery_before_sorting
- Backlog #155 S1: `GraphEntry::discovery` is the pre-sort BFS position, so the `--tree` walk can enumerate display parents in resolution-insertion order like Portage's digraph does. Input [a(dep b), b] sorts to [b, a]; ranks track input slots, not sorted ones.

## fn implicit_system_deps_n_skips_the_merge_order_bias
- Three independent leaves in discovery order a, b, c with c in @system: the bias promotes the deep system dep first, while `--implicit-system-deps=n` (Portage `depgraph._merge_order_bias`'s own early return) keeps discovery order.

## fn skipped_update_roots_attributes_miss_and_parent_per_edge
- Backlog #242 Slice D. The blk0 shape: target `blk0b-1` rejects `blk0x-3` through its `DEPEND` edge while a running-rooted twin of the graph exists (cross-active); the miss lands running, the parent stays target. Single-rooted (no running entry anywhere) the same edges attribute target (the `fixture_env` shape).
- Unknown parent: both target (today's rendering, documented).
