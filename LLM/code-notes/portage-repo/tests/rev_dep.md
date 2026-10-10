# crates/portage-repo/src/tests/rev_dep.rs
Reverse-dependency constraint pins: built slot-op atoms, reinstall pins, USE-break drops.

## fn is_built_slot_op_requires_equals_operator_and_both_slots
- Backlog #161 S1: `is_built_slot_op` is Portage's `Atom.slot_operator_built` gate. Each conjunct is load-bearing, and the companion normaliser strips the slot binding only for a built op.

## fn relax_built_slot_operator_atom_keeps_everything_but_the_binding
- Backlog #25 S1b-fix: `relax_built_slot_operator_atom` is Portage's `Atom.with_slot("=")` as `_slot_operator_check_reverse_dependencies` applies it (depgraph.py:2494-2502). Non-built atoms (bare `:=`, `:S=` without sub-slot, plain slots, unversioned) pass through verbatim.

## fn built_slot_operator_rebuild_trigger_only_forgives_rebuilds
- Backlog #25 S1b-fix: fires for the slotop upgrade shape; stays silent for the r25 withhold shape (a version veto).

## fn reverse_dependency_constraints_skips_an_unreachable_installed_consumer
- Backlog #54, the `keeper`/`paired` fixture-oracle shape: `reachable` is the `@world ∪ @selected ∪ @system ∪` argument-cp closure (`ResolveCtx::slot_op_reachable`).

## fn reverse_dependency_constraints_finds_a_slot_change_reinstall_pin
- Backlog #210: Portage `_complete_graph` (depgraph.py:8611-8618). Pre-fix the upgrading map held only `Upgrade`/`Downgrade` and this shape yielded no pin at all.
- Both pin flavours fire: the plain slot-bound pin (`target:0/1`) verbatim, and the built slot-operator binding (`target:0/1=`) verbatim too -- the Upgrade path strips the latter into the rebuild domain (#24 S5), but the reinstall re-seed constrains selection with the recorded atom as-is (depgraph.py:8677-8754).

## fn reverse_dependency_constraints_skips_a_noop_top_level_argument
- Backlog #210 (Q2a): a top-level argument settling `AlreadyInstalled` is a no-op request, not a digraph node in Portage; complete-mode arg-seeding still pulls it into `reachable`. A consumer the required sets reach stays a source, whether or not it was also requested.

## fn reverse_dependency_constraints_drops_a_use_break_from_a_use_change_reinstall
- Backlog #222, on the committed fixtures (upstream `test_complete_graph.py` pg0: `dev-libs/cgp0x-2.8.0` IUSE `+icu`, installed with `USE=""`; installed `dev-libs/cgp0q` records `dev-libs/cgp0x:2[-icu]`).
- A same-version USE-change reinstall moves no version, so the stripped-constraint scan finds nothing. The pin drops for the residual report because the merge's USE is decided; Portage fails rather than reflipping it (complete-mode end-of-walk loop, depgraph.py:8751-8791).
- Complete-mode off is `--complete-graph-if-new-use=n` (Portage rc 0); on is Portage rc 1. The unreachable leg is the shared-world shape (Portage rc 0 with that world).
