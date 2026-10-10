# crates/portage-repo/src/merge_order/tests/tree_display.rs
Tree-mode stuck-branch pins.

## fn tree_simulation_solves_a_stalled_replacement_row
- Backlog #81 S1: the C0 p2b shape in miniature. Owner O merges (1.0 -> 1.1) carrying a satisfied Replacement row for the installed 1.0 that R=blocked-2.0 replaces in-slot. Both nodes are roots, so tree mode's one-at-a-time selection stalls on round one with the blocker pending, schedules the uninstall, and solves the row when the uninstall is selected -- while the flat greedy pop would drain both without stalling. (Portage's verdict for the full shape is in `TEST/findings/l0.md` "#75 C0"; the flat-hidden half is `replacement_wait_index`'s call.)
- With two roots left, tree mode's one-at-a-time scan skips both and the stuck branch fires while the blocker is pending.

## fn tree_schedule_stuck_pins_guards_and_node_bookkeeping
- Pins the backlog #131 edge direction (the replacement's parent, Portage's edge reversal).
