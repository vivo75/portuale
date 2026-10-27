# R2 (#593): pins edited for the ` for <ROOT>/` header suffix

All in `pmtest/pytests-contract-suite/test_emerge_pretend_contract.py`. Only
the skipped-update header line changed (` for <ROOT>/` appended, via
`_for_root()`); no pin needed a block-order change (none held both a full
entry and the `triggered by backtracking` block). Pins that failed: 13
test items (12 functions, `test_oracle_210_...` has two parameters).

Slot-conflict (`WARNING`) block headers:
- `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it` (orbttool)
- `test_autounmask_use_backtrack_world_bound_falls_back_to_the_older` (abk0a)
- `test_unsolvable_slot_conflict_resolved_by_masking_a_puller_version` (bttarget)
- `test_oracle_backtrack_masks_are_discarded_with_their_reason` (btra)
- `test_tree_mg2top_nests_backtrack_parents_under_the_earliest_puller` (mgxc, mgfc)
- `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1` (blk0x, two helpers)
- `test_oracle_90_reversed_two_targets_withhold_with_a_skip_notice` (slotconflicttarget)
- `test_oracle_210_slot_change_reinstall_withheld_with_a_skip_notice[changed-slot, changed-slot-update-deep]` (reinstslottarget)
- `test_f484_old_build_warns_the_skipped_binary_update_like_portage` (f484-mime)
- `test_skip_block_keeps_the_built_atom_and_drops_non_selected_or_branches` (p569prov)
- `test_oracle_292_pin_withhold_block_follows_the_conflict_block` (header filter, not a literal)

Full unsatisfied-dependency entry header:
- `test_autounmask_keep_keywords_y_falls_back_to_the_older_stable_version` (akk0a)

`triggered by backtracking` headers:
- `test_unsolvable_slot_conflict_resolved_by_masking_a_puller_version` (btconsumer)
- `test_oracle_backtrack_masks_are_discarded_with_their_reason` (btrc)
- `test_tree_mg2top_...` (mgxb, mgfb)
- `test_skip_block_keeps_the_built_atom_...` (p569pull)

(Counts found: 12 skip-block + 4 triggered-by-backtracking pins that assert
header text exactly; the plan's 13/7 counted pins that match on substrings
and did not fail.) New pin: `test_593_skipped_update_blocks_follow_portage_order_and_carry_the_root`.

Unchanged on purpose: the `--backtrack=0` slot-collision block header in the
btparent test (a different renderer), and `All ebuilds that could satisfy`
(already uses the running-root rule, D:6502; the suite pins the running
root to ROOT so it stays bare).

Corpus: contract 7 blobs replaced, expanded 22. All header-only except the
`mg2top` blobs (see the report: backtrack counter 14/20 -> 10/20, present at
HEAD before this change).
