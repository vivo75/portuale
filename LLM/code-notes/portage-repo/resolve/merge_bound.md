# crates/portage-repo/src/resolve/merge_bound.rs
Lookups over the entries a run will merge.

## fn merge_bound_version
- Exactly the four outcomes `PassState::resolved_slots` ever indexes; a #72 B3 removal is deliberately not one of them.

## fn merge_bound_cpv
- `None` for `AlreadyInstalled`/`NoVisibleCandidate` (which never participate in a build-time cycle).

## fn merge_bound_index
- Lowest-index merge-bound entry per `(category, package)` -- the merge list is already in dependency order, so the first is the instance an edge belongs to. Shared by `find_hard_cycles` (cycle nodes) and `cycle_edge_labels` (owner lookup), so the two cannot disagree on which instance an edge belongs to. (It no longer picks the rendered start node: since backlog #278 that follows Portage's `shortest_cycle[0]`, i.e. `discovery` order.)

## fn merge_bound_root_index
- Track X Slice C (#242): merge-bound entry indices keyed by `RootCp` -- first index per (root, cp). `merge_bound_index` stays the cp-level view (render order starts at the first scheduled instance); this is the edge-resolution view (Portage's package-keyed digraph never merges two roots' instances).

## fn root_cp_lookup
- Track X Slice C (#242): resolve one root-qualified map endpoint to its entry index -- the exact-root instance first, falling back to the cp-level first index (the pre-slice behaviour) when this graph holds no such instance in that root (same-root graphs, synthetic entries, the `--root-deps` sidecar shapes the walk never root-splits).

## fn dep_child_lookup
- Track X Slice C (#242): resolve a `deps` edge to its child instance -- the resolving-root instance (`dep_child_root` from the owner's own root and the edge's dep key) when one is merge-bound, else the legacy cp-level first index. The walk always resolved the atom in the resolving root, so the filter only ever removes a provably-wrong same-cp other-root sibling.
