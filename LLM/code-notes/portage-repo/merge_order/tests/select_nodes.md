# crates/portage-repo/src/merge_order/tests/select_nodes.rs
`select_nodes` scheduling-loop pins.

## section fix pass
- Backlog #144 fix pass: the reviewer's counterexamples.

## fn select_nodes_prefers_a_leaf_whose_parent_is_an_asap_node
- A mutated gate/predicate picks 2 or 3 instead of the asap-parented leaf 4.
- The asap block cannot select (every asap node has a child that survives all SATISFIED rungs), and the cycle search's asap-only candidate set is empty, so `prefer_asap` flips and the rung loop reaches the asap-parent preference: leaf 4 (parent 1 is asap) wins over 2/3.

## fn select_nodes_escalates_to_the_satisfied_range
- A ring of satisfied buildtime edges: NORMAL's cycle search has no mergeable leaf, so the `drop_satisfied` escalation to `DepPrioritySatisfiedRange` is what breaks the ring.

## fn select_nodes_promotes_only_an_unsatisfied_pdep_child
- `||` in the predicate promotes the satisfied optional child and reorders the run.

## fn select_nodes_escalates_to_the_satisfied_range_before_the_roots
- The reviewer's counterexample for the `drop_satisfied && !ptr::eq` -> `||` mutant.

## fn select_nodes_pins_the_normal_range_cycle_pass
- The reviewer's counterexample for deleting the `!` on `!ptr::eq(range, &NORMAL)`: the mutant drops the NORMAL cycle pass instead of adding it.
