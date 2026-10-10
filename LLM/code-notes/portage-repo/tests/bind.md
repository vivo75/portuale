# crates/portage-repo/src/tests/bind.rs
Slot-operator binder unit tests: prune-rebuilds latch and token binding order.

## fn slot_operator_prune_rebuilds_clears_replace_and_built_masks
- Backlog #213 (v2 `#24d`): latches the flag, clears the whole replace set, drops only the `slot_operator_mask_built` steers plus their `mask_order` slots; other masks, the undone latch and the rest of the search state survive.
- Portage: `resolver/backtracking.py::_feedback_config:247-257`.
- The undone latch is not Portage state; the prune does not touch it.

## fn bind_slot_operator_token_prefers_graph_entries_then_installed
- Backlog #161 S1: one token of the S4 binder. A same-category entry for another package never binds; `New` and `Reinstall` entries bind before the installed fallback; of several same-cp entries the greatest version wins.

## fn bind_slot_operator_deps_binds_against_entries_then_installed
- Merge-bound entry wins over the installed instance because Portage's `_graph_trees` models the post-merge vdb: the bare `:=` binds to the tree's 0/2.
- An explicit stale `:0/1=` doesn't match the 0/2 entry because Portage's `vardb.match(x)` enforces the built atom's own sub-slot, so it falls back to the installed 0/1, unchanged.
- A version constraint no entry satisfies falls back to nothing and is left as-is ("keeping the information in vdb").
