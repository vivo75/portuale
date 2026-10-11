# crates/portuale/src/pretend/tests/world.rs
World and world_sets recording, usersets candidacy, and the installed set.

## fn world_file_additions_reports_only_new_plain_targets
- Backlog #232: the deferral arm's pre-record computation -- new atoms in argument order; already-present atoms, `@set` targets (the `world_sets` half), and args with no resolved entry contribute nothing.
- Nothing new -> empty (the `--ask` prompt stays silent too).

## fn world_sets_additions_reports_only_new_sets
- Backlog #232: the `@set` half of the deferral arm's pre-record computation -- `@`-prefixed lines for names not already recorded, in argument order.

## fn usersets_world_candidate_honors_only_an_explicit_usersets_override
- Backlog #232 review (Important-2): Portage's `lib/portage/_sets/__init__.py:133` + `:258-262` -- file sets are candidates unless `sets.conf` says otherwise.
- No `sets.conf` at all: the shipped default (candidate). An explicit `[usersets] world-candidate = false` flips it; option names are case-insensitive, section names are not.
- Every `getboolean` false spelling counts; `true` spellings flip back (later lines win, like Portage's merged sections). An unparsable value is ignored, never a suppression. The `sets.conf/` directory form: sorted walk, later wins. Unrelated sections never matter.

## fn update_world_file_records_a_new_target_and_skips_deps_and_oneshot
- `adep` isn't a requested atom -> not recorded; `wanted` is. A second run is idempotent, and `--oneshot` writes nothing. An unslotted `@system` member is never recorded.

## fn update_world_file_slot_qualifies_a_slotted_target
- dev-libs/dualslotpkg has SLOT=1 and SLOT=2 in the repo (neither `"0"`) -> "slotted"; dev-libs/packagepkg is SLOT=`"0"` only.
- Slotted cp + `:1` arg -> `dev-libs/dualslotpkg:1`. Bare arg for the same slotted cp -> plain `cat/pkg`. Unslotted cp (SLOT=`"0"` only) + `:0` arg -> still plain `cat/pkg`.

## fn update_world_sets_file_records_a_new_set_and_skips_oneshot
- A new set is appended; the comment line is dropped on rewrite, `@existing` is kept, output sorted. Idempotent, and `--oneshot` / `--onlydeps` write nothing.

## fn installed_set_atoms_are_slot_qualified_sorted_and_deduped
- A lone-slot package is still slot-qualified (bug #338959); a sub-slot is dropped (only the main slot); output sorted.
