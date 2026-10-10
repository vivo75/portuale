# crates/portage-repo/src/tests/skipped.rs
Skipped-update collapse/group pins mirroring Portage's missed-update rendering.

## fn collapse_skipped_updates_keeps_the_highest_version_per_slot
- Backlog #129 (review minor 3): the two sources are backtrack-mask rows and direct-solve rows. Portage `_get_missed_updates` (lib/_emerge/depgraph.py:1553-1562); a consecutive `dedup()` could not do this. A slot present in both sources renders once, at the higher version, whichever source holds it.

## fn collapse_skipped_updates_keeps_a_slots_first_position
- Backlog #292 (Portage `_get_missed_updates` dict key order, depgraph.py:1544-1560): slot X's lower row, block Y, then X's higher row render X, Y -- not Y, X.

## fn group_skipped_updates_merges_parents_of_the_same_missed_pkg
- Backlog #129 (review round 2): Portage `_show_missed_update_slot_conflicts` (lib/_emerge/depgraph.py:1650+) over the single per-`(root, slot_atom)` entry (:1553-1562). The `blk0b blk0c blk0a --backtrack=0` oracle shape (bed `l0-fx-20260927T125711Z`: one `dev-libs/blk0x:0` block for `-3` with both parents). Groups keep first-seen order, parents keep row order; a different missed version stays its own block.
