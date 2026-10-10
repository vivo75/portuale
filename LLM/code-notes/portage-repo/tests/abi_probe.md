# crates/portage-repo/src/tests/abi_probe.rs
ABI conflict probe tests: rebuild-vs-report gates and display continuity.

## fn slot_conflict_abi_probe_rebuilds_the_built_parent
- Backlog #214 (v2 `#24e`): the conflict-fired probe schedules the installed `:=` parent for rebuild instead of reporting, with one guard arm per coded gate.
- A merging parent rebinds through its own walk, so no probe fires; an in-flight rebuild is not scheduled twice.
- Excluded parent: Portage's `excluded_pkgs` skip; never force-rebuilt.
- Non-built atom: a plain version pin is a conflict, not an ABI rebuild (Portage's `slot_operator_built` gate).
- `(Argument)` parent: not a package, Portage skips it.
- Same sub-slot on both instances: no ABI shift to chase. Downgrade direction (older other-instance): never the update target.
- No same-version ebuild for the parent: the forced reinstall would dead-end. Without a visible replacement the probe does nothing.
- Veto: another installed parent rejects the fresh candidate (the R2 refusal, same gate as the scan).
- #254: the replacement's `:=` under a USE conditional that is off in the tree ebuild. Portage's `_select_atoms_probe` (`_emerge/depgraph.py:2667-2713`) evaluates it against the replacement's USE, so the atom is not selected and the probe refuses; a flat token match would accept it.

## fn slot_conflict_abi_display_pairs_follow_the_rebuild
- Backlog #214 (v2 `#24e`): settle-time display continuity.
- #254: the rebuilt consumer's own walk pulls the provider; Portage pairs only a child of the replacement.
- #254: the rebuilt consumer does not pull the provider when its new build dropped the `:=` (e.g. a USE conditional now off): Portage's `_compute_abi_rebuild_info` finds no child, so no pair.
- Settle-time display continuity: a `slot_operator_rebuild` reinstall re-derives its `(provider-cpv, consumer-cpv)` pair from the recorded built atom and the merged provider, so the `causing rebuilds` block renders even though the firing pass restarted.

## fn argument_probe_parents_veto_a_candidate_the_argument_does_not_match
- Backlog #288: an argument atom is a probe parent of its own cp (Portage `_slot_operator_check_reverse_dependencies` over `_parent_atoms`); a blocker argument is not a parent.
