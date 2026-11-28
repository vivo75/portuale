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

## fn build_residual_renders_a_built_pin_only_in_its_built_form
- Backlog numbers removed from the comment (original text): Backlog #607: a pin stripped from a built `:S/SS=` atom renders only the built atom; a plain pin keeps its own text beside its built siblings. Expected from Portage 3.0.83 on the r25 shape (`=r25lib-2.0` over `r25consumer`'s `<dev-libs/r25lib-2.0:0/1=`): the parent is the built atom, never `<dev-libs/r25lib-2.0`.

## fn slot_conflict_abi_step_files_the_installed_provider_and_consumer
- Backlog numbers removed from the comment (original text): #601: the slot-conflict ABI step must file **both** sides Portage's `_slot_operator_update_backtrack` files for a built `:=` conflict: the installed parent (`dep.parent.installed`) and the installed child (`child.installed`, `depgraph.py:2434-2460`). Portuale's `slot_conflict_abi_probe` only returns the parent, so the installed provider never joins `slot_operator_replace_installed`; `drop_masked_pairs` then wipes every display pair whose provider is the masked-installed instance (the cell's `kept=0`). Portage: depgraph.py, _slot_conflict_backtrack_abi / _slot_operator_update_backtrack
- Backlog numbers removed from the comment (original text): The display mirror carries both sides: it becomes the `r` tag (`__auto_slot_operator_replace_installed__`), so a child-only filing would leave the provider untagged (#601). Portage files both into its one set (the two `backtracking due to missed slot abi update` blocks in the -D oracle debug); portuale splits application but not tagging.
