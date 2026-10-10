# crates/portage-repo/src/merge_order/tests/select_nodes.rs
`select_nodes` scheduling-loop pins.

## section fix pass
- Backlog #144 fix pass: the reviewer's counterexamples.

## fn select_nodes_prefers_a_leaf_whose_parent_is_an_asap_node
- A mutated gate/predicate picks 2 or 3 instead of the asap-parented leaf 4.

## fn select_nodes_promotes_only_an_unsatisfied_pdep_child
- `||` in the predicate promotes the satisfied optional child and reorders the run.

## fn select_nodes_escalates_to_the_satisfied_range_before_the_roots
- The reviewer's counterexample for the `drop_satisfied && !ptr::eq` -> `||` mutant.

## fn select_nodes_pins_the_normal_range_cycle_pass
- The reviewer's counterexample for deleting the `!` on `!ptr::eq(range, &NORMAL)`: the mutant drops the NORMAL cycle pass instead of adding it.
