# crates/portage-repo/src/tests/rebuild_scan.rs
Slot-operator rebuild-scan pins: pin-forced downgrades, new-slot arm, refusal vetoes.

## fn slot_operator_rebuild_scan_treats_a_pin_forced_downgrade_as_a_missed_update
- Backlog #303: the provider is held back by an installed consumer's built `:=` pin. Portage's first pass makes the same downgrade, then `_slot_operator_update_probe` (depgraph.py:3131) finds the installed instance and backtracks to replace the consumer.
- Expected value: host Portage on `-uDN sys-apps/systemd` (libfido2 `rRg`, libcbor kept at the installed 0.14); see `LLM/backlog-tasks-2026-10.md` #303.

## fn slot_operator_rebuild_scan_flags_only_the_stale_bindings
- Backlog #24 S3: a cp already in `already` stays reported even once in the graph as the rebuild this scan asked for last pass -- that is how `abi_rebuilds` survives to the settling pass.
- The `--solver=` bridges keep the pre-S3 synthesiser.
- The S5 probe (backlog #24 S5, the slot-operator-dep probe) is not complete-mode gated, but `bar`'s upgrade carries no unbuilt `:=` dep, so nothing fires in the not-reachable leg.

## fn slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider
- Backlog #211 (v2 `#24b`): the new-child-slot arm (depgraph.py:3121-3126, bug 486580). Guards: a superseded installed child in the bound slot, a higher (or equal) version, `--update` (or a directly-requested provider), and a visible tree candidate for the consumer.
- Backlog #269: the new-slot arm files the child (provider) reinstall only when `new_child_slot is None` (Portage `_slot_operator_update_backtrack`, depgraph.py:2442); Portage's `_forced_rebuilds` stays empty (no provider `r`, no `causing rebuilds` block).
- Backlog #252: with `--update` and the provider directly requested, Portage's greedy request-argument expansion pins the installed slot (`massb:1`); the update probe's reverse-deps gate refuses every slot-2 candidate, so the replace set stays empty. Probed on Portage 3.0.82.2: the `mmprov` shape merges only the provider upgrade. Same call as `via_arg` with `update` on.
- The downgrade arm: with `massb-2.0` on the tree the probe fires through it (Portage ranges over every available package, highest acceptable wins, #269); the graph's downgrade entry does not veto that. Pure downgrades never probe.
- #269: Portage's `_slot_operator_update_backtrack` (`depgraph.py:2442`) files the child (provider) reinstall only when `new_child_slot is None` -- the new-slot arm records the consumer alone, so it contributes no `(provider, consumer)` display pair (Portage's `_forced_rebuilds` stays empty: no provider `r`, no `causing rebuilds` block).

## fn slot_operator_rebuild_scan_pin_gate_cuts_probe
- Backlog #268: the `#252` pin gate at scan level. Scratch `massb`/`massc` shape with a twist: the tree's highest package `massb-2.0` (`2/2`) soft-blocks the bound slot (`RDEPEND="!dev-libs/massb:1"`), so Portage's greedy revision with blocker lookahead discards the `massb:1` pin and the probe fires; the request atoms are explicit (`massb:2`, `=massb-1.0`-style), spanning a single slot and pinning nothing. Every call schedules the consumer alone (no display pair -- the new-slot arm, #269).
- Since #288 an exact-version atom is an argument parent of its cp: Portage's update probe refuses every candidate it does not match (`=massb-1.0` vetoes the new-slot `massb-2.0`; Portage 3.0.82.2 merges nothing for the `mmprov` twin, probe `D-arg-pin`).

## fn slot_operator_update_probe_refusal_vetoes_both_arms
- Backlog #211 R2: Portage `_slot_operator_check_reverse_dependencies` (depgraph.py:2472-2573, gates `:2622`/`:2738`). A parent whose pin the candidate still satisfies (built `:S/SS=`, relaxed to `:=`) never vetoes; neither does the consumer itself (Portage's `replacement_parent` skip), an out-of-scope parent Portage never walks, a parent this run replaces, or an excluded one (Portage's `excluded_pkgs` skip).

## fn slot_operator_new_slot_arm_probes_bound_slot_mismatched_entries
- Backlog #211 item 2: `Upgrade`/`Downgrade`/`Reinstall` entries. Portage's candidate loop (`_iter_similar_available`, depgraph.py:2660-2695) ranges over every available package, not only fresh-slot merges.
- #269: the new-slot arm records the consumer alone (Portage's `_slot_operator_update_backtrack`, `depgraph.py:2442`), so the bound-slot-mismatched `Upgrade` entry schedules `massn` with no `(provider, consumer)` display pair.

## fn slot_operator_new_slot_arm_needs_a_replacement_atom_that_accepts_the_new_slot
- Backlog #304: Portage `_slot_operator_update_probe` (depgraph.py:2667-2760). `pinned`'s current ebuild says `dev-libs/massm:0=`, so a slot-1 `massm` can never be pulled in by rebuilding it (host: `gnome-keyring`'s `gcr:0/1=` vs the installed `gcr:4`, expected value from Portage -- no rebuild); `open`'s says `dev-libs/massm:=` and does rebuild.

## fn slot_operator_rebuild_scan_honours_with_bdeps
- Backlog #65 S1.
- Bdeps on (`--with-bdeps=y` / the default without `--usepkg`) but consumers only reachable, not walked: S2 keeps the build-time keys off. Portage's complete-mode-only reachability never probes an installed parent's optional build-time dep (depgraph.py:3114,3802-3806,4277/4287).
- The slot-change probe (`slot_operator_slot_change_probe`) reads merge-bound parents' tree metadata, not the vdb.
- The reachability gate (S1) beats the walk test (S2): even a walked consumer's build-time keys stay out when bdeps is off. Portage `lib/_emerge/depgraph.py:4194-4247`: DEPEND/BDEPEND read only with `with_bdeps` (Portage empties them otherwise); RDEPEND/PDEPEND/IDEPEND stay unconditional.

## fn slot_operator_rebuild_scan_reinstall_parent_and_skip_clauses
- Backlog #161 S1: the `Upgrade | Downgrade | Reinstall` arm.

## fn slot_operator_rebuild_scan_newslot_arm_fires_without_a_scheduled_provider
- Backlog #269: the `mmprov`/`mmcons` shape from the #253 recipe with only `mmcons` requested -- installed `mmprov-1` (`0/1`) + `mmprov-2` (`1/1`) + `mmcons-1` bound `mmprov:0/1=`, tree `mmprov-3` (`1/2`).
- Portage 3.0.82.2 container probe 2026-10-01 (`emerge --ignore-default-opts --pretend --color=n --backtrack=20 --update --deep app-misc/mmcons dev-libs/slotconflictoldconsumer dev-libs/slotconflictnewconsumer`): merges `mmprov-3` bare plus the `mmcons-1` rebuild (`rR`) with no `causing rebuilds` block. The update probe (`_slot_operator_update_probe` with `new_child_slot=True`, depgraph.py:3121-3126) finds `mmprov-3` among the available packages (`_iter_similar_available`) though no pass scheduled it; the backtrack (`_slot_operator_update_backtrack`, depgraph.py:2442) records the consumer alone. The pre-#269 scan ranged only over the scheduled `New`/`Upgrade` entries, so with no provider entry it scheduled nothing.
- The #252 shape (provider requested too): `want_update` rides on `--update`, and the greedy pin keeps it probing nothing.
