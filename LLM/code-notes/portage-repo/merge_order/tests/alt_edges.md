# crates/portage-repo/src/merge_order/tests/alt_edges.rs
Alternative (`||`) branch suppression and target-ranking pins.

## fn resolved_dep_targets_ranks_by_vercmp_not_by_string_order
- Backlog #82: `print_tree` carried a private copy of this narrowing whose tie-break was `jv.cmp(bv)`, a string compare under which `1.9` outranks `1.10`. Portage resolves a slot-unqualified atom through `_select_pkg_highest_available`, i.e. the vercmp-highest of the matching graph nodes. Container oracle: `TEST/findings/l0.md` "#82" (`dev-libs/treeslotuser`).

## fn unqualified_atom_collapses_onto_the_slot_qualified_sibling_target
- Backlog #85: Portage's `_minimize_children` (`depgraph.py:4751-4856`). `user` pulls slot `:0` (selects 1.9) and the bare cp (selects 1.10, the vercmp-highest). Portage eliminates the redundant 1.10 selection -- the `:0` atom matches only 1.9 while the bare atom matches both -- so the bare atom resolves to 1.9 and the scheduler graph holds no `user → 1.10` edge (Portage's own `--debug` digraph dump for `dev-libs/treeslotuser` shows only the 1.9 and parent edges). Without the elimination the extra edge doubles 1.10's parent count and `_merge_order_bias` flips the pair to `1.10, 1.9`.

## fn resolved_dep_targets_reports_an_installed_target_as_no_tree_edge
- Backlog #82: the single `merge_bound_only` difference between the two callers. `build_digraph` edges to an installed (nomerge) node the way Portage's digraph does; `print_tree` must not, because portuale renders no line for one and the edge would open a hole in the tree.

## fn kept_alt_branches_agrees_with_build_digraph_on_a_multi_branch_group
- Backlog #76 B1: Portage `dep_zapdeps`' all-in-graph bin outranks all-installed, so the kept branch is the merge-bound one; the public helper must report exactly that (B0 q2), and `build_digraph` -- which shares `suppressed_alt_edges` -- must record an edge only to it.
- The mogo half pins the backlog #53 self-branch exclusion the helper shares (see `build.rs`'s `self_referential_disjunctive_bdepend_skips_the_self_branch`).

## fn select_dep_target_eliminates_with_suppressed_sibling_sets
- With the suppressed sibling's set {x1} counted, the shared-by-all test keeps x1 and drops x2, flipping the pick away from the vercmp-highest merge-bound target.

## fn suppressed_alt_edges_requires_every_atom_of_a_branch_to_match
- Cluster-run survivor (backlog #144 follow-up): only a branch whose every atom matches installed wins the all-installed bin.
