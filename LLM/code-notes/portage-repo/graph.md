# crates/portage-repo/src/graph.rs
Shared dependency-graph helpers.

## fn strongly_connected_components
- Tarjan's algorithm, iterative (an explicit `work` stack of `(node, next successor)`), so a 200 000 node chain does not overflow the thread stack (test `strongly_connected_components_groups_cycles_and_survives_long_chains`). Portage: `digraph.strongly_connected_components()`, the call behind `cycle_members` in `_calc_depclean`.

## fn strongly_connected_components visibility (#512)
- Promoted from `pub(crate)` to `pub` (re-exported through `pub use graph::*`, moved here from `depclean.rs`) so the `-j` scheduler can find dependency cycles (`merge_wait::cyclic_nodes`): Portage `Scheduler._find_cyclic_nodes` runs the same `digraph.strongly_connected_components()`. No behaviour change; the depclean callers are untouched.
