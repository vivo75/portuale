# crates/portage-repo/src/tests/queue.rs
Queue-walk pins: --exclude, slot-keyed dedup, surviving slot conflicts, deep levels.

## fn exclude_leaves_an_already_installed_package_alone_even_with_update
- Without `--exclude`, `--update` offers the 1.0 -> 2.0 upgrade for installed `dev-libs/upgradepkg` (see the `upgrade` test); `--exclude` matching it overrides `--update` entirely.
- Portage: `_want_update_pkg` / `_replace_installed_atom` (excluded-checked-first precedent).

## fn exclude_matches_via_a_wildcard_atom_too
- Portage's WildcardPackageSet accepts wildcard atoms, not just plain ones; ported as the same two-tier `matches_config_entry` matching package.mask/.unmask already uses.

## fn other_outcomes_dedups_installed_instances_by_slot_not_cp
- Portage `pkg.slot_atom` node identity, not the cp.
- Fixture shape: `fixtures/var/db/pkg` has `dev-libs/slotdedup-1.0` (slot 1, no deps) and `slotdedup-2.0` (slot 2, RDEPEND `dev-libs/slotdedupmarker`) installed; `fixtures/repo/dev-libs/slotdedupconsumer-1.0` RDEPENDs `dev-libs/slotdedup:1 dev-libs/slotdedup:2` (slot 1 first).
- With the old cp-keyed dedup the slot-2 visit was dropped, so its deps were never walked and `slotdedupmarker` never entered the graph -- the #74 class B1 shape (`llvm-core/clang:22` shadowing `:23`, losing myst-parser).

## fn surviving_slot_conflict_graphs_both_same_slot_instances
- Backlog #148: Portage's `l0-fx-20260922T192350Z` cell `-D dev-libs/slotusegroup` shows `[ebuild N] slotusetarget-1.0` and `-2.0` (rc 1).
- `dev-libs/slotusegroup` RDEPENDs `slotuseplain slotusex slotusey`; slotusey/slotusex pull `>=slotusetarget-1.0[y]` / `[x]` (2.0 lacks `y` in IUSE and has `x` default-off) while slotuseplain pulls `>=slotusetarget-2.0`.
- Before the fix the walk's `resolved_slots` conflict arm recorded the `SlotConflict` and continued past `GraphEntry` creation, so the graph held one `slotusetarget` entry (1.0) and the merge list one row.
