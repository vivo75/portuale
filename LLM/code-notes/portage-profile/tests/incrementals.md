# crates/portage-profile/src/tests/incrementals.rs
Incremental-variable folding tests (USE token replay, wildcard removal, snapshots).

## fn use_tokens_capture_the_ordered_raw_use_values_that_produced_use_flags
- Fixture: base make.defaults `USE="foo"`; leaf (parent -> base) `USE="-foo bar"`; make.conf `USE="baz"`. `use_flags` ends up {bar, baz} either way. `use_tokens` = the profile chain (`defaults` layer), `conf_use_tokens` = make.conf (`conf` layer, split out by the "Config depth" slice). Replaying both via apply_incremental from an empty set reproduces use_flags exactly -- which is what lets portage-repo's `effective_use_flags` replay them on top of a different seed (a package's IUSE defaults) instead of union-ing the pre-flattened use_flags, which could never let "-foo" cancel an IUSE "+foo" default.

## fn apply_use_incremental_removes_a_prefix_wildcard
- The USE_EXPAND fold (Portage config.py:2837-2866) runs before the USE loop. The generic `apply_incremental` keeps exact-removal semantics: it must NOT expand `_*` (the wildcard arm is USE-scoped in Portage).

## fn a_files_incremental_layer_is_its_final_assignment
- L3 finding: Portage config files are sourced. `emerge --info` drops the first line's tokens entirely.
