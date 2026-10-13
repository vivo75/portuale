# crates/portage-repo/src/resolve/pass/existing_slot.rs
Keeps, replaces or conflicts the already-resolved slot.

## fn step_existing_slot
- History: matching without the resolved sub-slot produced a spurious slot conflict for a package two binary parents both depend on (binary recorded RDEPEND commonly carries built slot-operator atoms).

## fn existing_slot_unsatisfied
- Backlog #290: `pprov` (pulled in only by the slot-operator forced upgrade, not requested) vs the installed `:=` consumer's built atom (`>=pprov-1:0/1=`): a rebuild trigger the scan schedules (`rR pcons`), not a conflict (Portage 3.0.82.2, bt > 0). With none installed (`abiprov`, both versions merge-bound) the record drives the conflict-fired ABI rebuild probe (backlog #214), so it stays.

## fn existing_slot_slot_operator_bind
- Backlog #90 (S1): the raw `:=` reuses the just-selected instance, so the installed-bound instance is never pulled and the two-instance record never fires. Existing stays the merge, the bound version files as the installed side (the #57 installed-instance pattern); rebuildable consumers skip this since their update proceeds with a rebuild, like Portage. See `built_equals_shift_unrebuildable`.

## fn existing_slot_version_mismatch
- USE arm (second instance): see `existing_slot_use_flip_suggestion`; Live-verified: Portage pulls a second instance and the notice carries `("use", flag)` parents.

## fn existing_slot_use_flip_suggestion
- USE arm: `match_from_list` matches version + slot only; if the atom carries a `[flag]` use-dep the already-resolved package still has to satisfy it, and if it doesn't but a `package.use` flip would (Portage `--autounmask-use` for a dep on a package already in the graph), the flip folds into `autounmask_use_config` and the whole walk re-runs so the package re-resolves with the flag and its `flag?`-gated deps appear (Portage `_need_restart` after `_needed_use_config_changes` grows). Live-verified: Portage pulls a second instance and the notice carries `("use", flag)` parents.
- Backlog #244: oracle `aubreaktop`, evidence 2026-09-28-244. Unlike the parent-flip site, the flip lands while the package is still stacked, so Portage needs no re-drive (see `expand_resolved_slot_with_flipped_use`); backlog #218.
- Portage `_backtrack_depgraph` finishes the attempt, then restarts with the grown config -- the driver at the bottom of `'backtrack` does the `continue`, not here. Only with `--autounmask-backtrack=y`; off (the default), the change is recorded and displayed but the graph is not re-driven.
- A post-loop pass refreshes the flipped package's own USE line when backtrack is off.
