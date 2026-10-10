# crates/portage-repo/src/merge_order/tree_display.rs
`--tree` display order and stuck-node handling.

## struct TreeStuck
- Backlog #81: tree-mode stuck-branch state for `select_nodes`.
- Portage's tree serializer keeps `scheduled_uninstalls` as live scheduler state: when a round selects nothing and blockers are pending, it schedules an uninstall for the blocked package, reverses the edges so the blocked package merges on top of it, and appends the solved blocker when the blocked package (or the uninstall itself) is later selected (`depgraph.py:9998`, `:10190`, `:10315-10358`). The flat greedy pop drains past the same shape, so the row stays hidden there.

## struct PendingUninstall::strong
- Portage applies overlap guards to strong (`!!`) blockers (`forbid_overlap`, portage-essential); v1 schedules soft (`!`) rows only and leaves strong rows hidden.

## fn tree_schedule_stuck
- Mirrors Portage's choice loop in narrowed form: Portage picks by fewest parent-deps with an early break at one; single-pending shapes -- every oracle here -- decide identically to first-in-entry-order. Portage's edge reversal makes the merge node the uninstall's child (`depgraph.py:10190` -- `mygraph.add(slot_node, uninst_task)`).
- Backlog #131: the direction is load-bearing for the display order -- as the replacement's child (the pre-#131 shape) the replacement rejoins a later batch; as its parent the next round picks the replacement alone, exactly like Portage's round-2 pick. The frontier never learned the pre-#131 child shape either (likewise never synced). Appended to `order` like Portage's `mygraph.add` appends new nodes.

## fn tree_note_selection
- Portage appends a solved blocker when the blocked package merges while its uninstall is scheduled, and discards it when the uninstall node itself is selected (`:10315-10358`). The replacement-merged removal mirrors Portage's `mygraph.remove`, like the flat loop's removal site.

## fn tree_stuck_pending
- The uninstall-solving `Uninstall` arms and the unsolvable rows already have display homes everywhere; strong blockers stay hidden per the v1 narrowing. Each pending row pairs with the merge-bound replacement entry Portage's solved-blocker append follows (`depgraph.py:10351-10358`).

## fn tree_solved_replacements
- Backlog #81 S1. Callers pass resolver-indexed `entries`; verdicts address the same indexing. Shares `schedule_graph` (closure, prune, bias) and every selection helper with the flat path -- the only differences from `serialize_merge_order` are the `not tree_mode` gate and the stuck branch, exactly like Portage. The common no-pending-row case pays nothing.

## fn tree_display_order
- Backlog #131 S1. Portage's `_ordered_tree_display` walks the reversed tree-mode retlist (`depgraph.py:10618-10619`, tree serializer with the greedy leaf pop disabled, `:9764-9777`), not the flat merge order -- the two differ wherever one-at-a-time selection meets leaves in a different sequence than the flat batch pop (p2b's `newpkg` interleaving, slotusegroup's sibling order). Unseen-by-the-scheduler entries rejoin through the same leftover weave-back as the flat path.
