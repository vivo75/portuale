# crates/portage-profile/src/tests/fingerprints.rs
Context moved out of the code comments.

## fn use_context_base_fingerprint_is_frozen_at_resolution_and_excludes_autounmask_use
- Backlog numbers removed from the comment (original text): #105: `resolve_config` freezes the immutable USE-context digest into `use_context_base`, and `use_context_base_fingerprint` is content-derived while excluding `autounmask_use` -- the one field the `'backtrack` loop mutates, which the memo keys hash separately.

## fn is_visible_base_fingerprint_is_frozen_at_resolution_and_content_derived
- Backlog numbers removed from the comment (original text): #114: `resolve_config` also freezes the visibility base (masks + the license/keyword/property/restrict acceptance lists + `license_groups`) and it is content-derived.
