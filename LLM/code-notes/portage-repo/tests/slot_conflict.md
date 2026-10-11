# crates/portage-repo/src/tests/slot_conflict.rs
Residual slot-conflict record pins: merge/installed versions, puller filing, re-file.

## fn record_slot_conflict_merges_same_triple_and_appends_new
- Portage keeps one collision handler per conflicting slot: a second distinct atom hitting the same (slot, existing, current) triple re-renders the record (rebuilt from the live pullers) instead of appending a duplicate notice block.

## fn build_residual_forms_a_record_for_a_broken_pin
- Backlog #161 S3.

## fn build_residual_reads_the_merge_version_from_a_new_entry
- Backlog #161 S3.

## fn build_residual_skips_a_consumer_being_replaced
- Backlog #161 S3.

## fn build_residual_reconciles_a_pin_the_merge_satisfies
- Backlog #161 S3.

## fn build_residual_reads_the_merge_version_from_the_target_entry
- Backlog #161 S3: the neighbour sorts last, so a loosened skip reading it would overwrite the target's version.

## fn build_residual_keeps_the_highest_matching_installed
- Backlog #161 S3: installed 2.0 at the same sub-slot does not match `=...-1.0:0/1` and must not displace installed 1.0.

## fn build_residual_prefers_the_greatest_matching_installed
- Backlog #161 S3: Portage's `matches[-1]`.

## fn build_residual_files_puller_parents_by_version_and_use
- Backlog #161 S3: a USE-mismatching atom files nowhere, a version-missing atom files nowhere, and identical pullers file once per side. The puller plus the dropped pin itself folds in after the pullers.

## fn build_residual_folds_each_pin_into_its_own_record
- Backlog #161 S3.

## fn build_residual_dedups_identical_pins
- Backlog #161 S3.

## fn merge_same_slot_conflicts_refiles_a_built_puller_under_the_installed_sub_slot
- Backlog #291: the bt0 lone-record re-file. The installed foo-1.0 was built at 0/9; the tree's 1.0 says 0/1.
