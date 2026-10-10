# crates/portage-repo/src/tests/backtrack_params.rs
`slot_conflict_mask_choices` unit tests: missed-update sibling grouping.

## fn slot_conflict_mask_choices_groups_missed_update_siblings
- `mgfc` conflict as the `mgfa` walk records it: 2.0 pulled by `mgfb-2`'s `=mgfc-2`, 1.0 by `mgfa`'s and `mgfb-1`'s `=mgfc-1`; `mgfc-3.0` visible in the repo but pulled by nothing.
- The 2.0 choice groups 3.0 (both `=1.0` parents miss it); the 1.0 choice groups 3.0 too (`=2.0` misses it). Ascending length, so the 1.0 choice (one parent) sorts first.
- Content pins beyond counts: the 2.0 choice is opposed by both `=1.0` pulls and so is its 3.0 sibling; the 1.0 choice and its sibling answer only `=2.0`.

## fn slot_conflict_mask_choices_skips_an_already_masked_sibling
- Backlog #161 S5: over `dev-libs/mgfc` (three same-slot versions) the masked non-party 1 stays out of 3.0's group.
- The `=dev-libs/mgfc-2` puller matches 2 only; without it the masked sibling would share every conflict atom and the subset gate rather than the mask check would exclude it.
