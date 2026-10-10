# crates/portage-repo/src/tests/visibility.rs
Visibility license/keyword/mask and memo-key tests.

## fn license_eula_style_negation_masks_a_matching_license
- Portage-profile's `resolve_config` would have produced the already-@group-expanded form from Portage `"* -@EULA"` with license_groups EULA="SomeEula". Portage-repo never expands groups, only consumes the already-expanded tokens (see `accept_license`'s doc, portage-profile).

## fn metadata_key_accepted_memo_does_not_leak_between_differing_accept_lists
- Backlog #110: the memo key must distinguish two configs that differ only in their accept lists -- the fields `use_context_fingerprint` does not cover. Two configs with the same USE context, candidate and value but opposite `accept_properties` must not collide.

## fn metadata_key_accepted_memo_does_not_leak_between_differing_candidate_iuse
- Backlog #110 (review fix): `use_flags_if_conditional` derives the USE set from IUSE/KEYWORDS whenever the value is conditional; a binary candidate from a `Packages` index and the ebuild for the same CPV can share `candidate_str`. Same reason `EUF_CACHE` hashes `iuse`/`keywords` into its key.

## fn metadata_key_accepted_memo_hits_on_a_repeat_and_separates_metadata_keys
- Backlog #110: the uncached body runs once on a repeat; the explicit `MetadataKey` discriminant (rather than inferring the pair from the slices) is the memo's identity for the two accept-list pairs.

## fn is_visible_memo_hits_on_repeat_and_separates_its_inputs
- Backlog #114.

## fn package_accept_keywords_negation_revokes_a_globally_accepted_keyword
- Portage's `KeywordsManager._getEgroups` folds `-token` removals over the combined global+package list, not just unions everything a matching entry ever mentions.

## fn package_accept_keywords_entries_fold_in_source_order
- Portage `stack_lists(incremental=True)` over the per-source lists. See `specificity_ordered_flags`'s doc for the deliberate "plain source order, not a global specificity sort" choice.

## fn is_visible_fingerprint_debug_asserts_on_post_freeze_mutation
- Backlog #121 (audit O6): same post-freeze guard on the visibility path; `IV_CACHE` is the poisoned memo.
