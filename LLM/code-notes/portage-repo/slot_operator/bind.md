# crates/portage-repo/src/slot_operator/bind.rs
Binding `:=` dependencies to the selected slot.

## fn bind_slot_operator_deps
- Backlog #24 S4: Portage's graph-aware `:=`/`:S=` binder (`portage/dep/_slot_operator.py::_eval_deps`, run per atom from `evaluate_slot_operator_equal_deps`).
- Portage binds every slot-operator atom against `_graph_trees[root]["vartree"]`, whose dbapi is the `PackageTrackerDbapiWrapper` (`depgraph.py:744-775`) -- "the state that the vdb will have after new packages have been installed". So a merge-bound package wins over the installed instance: in portuale's one-instance-per-cp model that is a merge-bound (New/Upgrade/Downgrade/Reinstall) `GraphEntry` first, else `<root>/var/db/pkg` (`installed_candidates`), highest matching version (`vardb.match(x)[-1]`).
- An atom no candidate satisfies is left unchanged, exactly like Portage's own "just leave it as-is for now ... keeping the information in vdb".

## fn built_equals_binding
- Backlog #90 (S1): does a bare `cat/pkg:=` atom's built binding point at a different sub-slot than `existing_sub`? Portage binds `:=` to the sub-slot of the *installed* instance (the ABI it was built against), never to whatever version this walk happened to select first: under reversed walk order the raw `:=` would match the just-selected 3.0.4:0/0 and swallow the genuine sub-slot shift the installed 3.1.1:0/13 reports (the need_rebuild-trailer shape: ark's built `:0/0=` alongside the ebuild `:=`).
- The binding's sub-slot is the tree metadata's for the highest installed version (the vdb `SLOT` file of a minimal root need not carry it); no installed instance -- or no tree sub-slot for it -- falls back to "not shifted" (the raw match above stands).

## fn built_equals_shift_unrebuildable
- Backlog #90 (S1), second half: a `:=` shift is only a *conflict* when the bound consumer cannot be rebuilt -- Portage rebuilds a rebuildable consumer in place (`R`, no conflict, no trailer) and only the `--exclude` / `--useoldpkg-atoms` consumer is stuck (the two need_rebuild-trailer reasons this pins; the masked-or-unavailable ebuild reason stays the filed follow-up).
- Without this gate the shift rule above would record a phantom conflict on every ordinary sub-slotted update (mlocaml-4.02.1: the update IS the bound version's move, mllablgl rebuilds), which backtracking then masks into invisibility.

## fn bind_slot_operator_token
- The same reconstruction `ebuild_phases.rs::bind_slot_operator` uses. The substring is distinctive enough to appear exactly once in a well-formed atom: a version can't contain `:`, a `::repo` carries no `=`, a `[usedep]` carries no `:`.

## fn atom_matches_str
- The shape every atom-vs-instance check below uses. A literal `[flag]` parent atom the walk accepted therefore reads as matching, the same direction `reverse_dep_constraint_atom` already documents for its own use-dep cut.

## fn strip_libc_atoms
- Applied to the flattened set; Portage strips only each key's outer level, the flat comparison has no levels left (documented narrowing).
- Portage `strip_libc_deps` (`portage/dep/libc.py`): remove every atom whose `cat/pkg` is an installed libc provider from a rule-8 side. Applied to the flattened set (Portage strips only each key's outer level; the flat comparison has no levels left -- documented narrowing).

## fn tree_metadata_for
- The source rule 8's *new* side is compared from (Portage `pkgsettings.setcpv(pkg)` -> `configdict["pkg"]`).

## fn apply_prune_rebuilds
- Backlog #213 (v2 `#24d`): Portage `resolver/backtracking.py::_feedback_config` `:247-257`. Portage pops that reason off the runtime masks of the recorded packages and clears its own `slot_operator_mask_built` set; portuale keeps no parallel set -- the `MaskReason::SlotOperatorBuilt` negatives in `runtime_pkg_mask` ARE the record, so removing them plus their `mask_order` slots is the whole equivalent.
- The caller fires this once (the latch guards the re-entry); the re-walk re-masks any still-stale binary through the probe above, exactly like Portage's re-probe.
- The #236 A forcing half is Portage's replace-set probe upgrade (backlog #295): it goes with the set.

## gate threading (#571)
- `tree_metadata_for`, `built_equals_binding`, `built_equals_shifted_slot` and `BuiltEqualsShiftUnrebuildableArgs` carry the gate.
