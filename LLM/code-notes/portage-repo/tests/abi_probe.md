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

## without_or_groups reference removed (#618)
- The #601 test docstring no longer cites the removed `without_or_groups`
  strip; the scan side now reads the selected arm.

## fn slot_conflict_abi_probe_accepts_an_or_group_arm_for_the_new_child
- Backlog numbers removed from the comment (original text): #601: a live `|| ( =prov-2 =prov-1 ) prov:=` consumer accepts the 5.44 child through the `:=` arm. Portage's `_slot_operator_update_probe` needs only one `||` arm to match; `replacement_accepts` reads the selected arm (backlog #618), but `slot_conflict_abi_probe` flattens every member and requires all of them, so the parent is never filed and the provider misses the replace set (the cell's exit-1 residual conflict). Portage: depgraph.py, _slot_operator_update_probe

## fn collect_probe_parents_chooses_the_selected_or_arm
- Backlog numbers removed from the comment (original text): #601: a recorded `|| ( … )` group contributes the arm `dep_zapdeps` selects -- the in-graph member first, then an installed match -- and only that arm's atom. The walk records the atom it resolved (`_add_parent_atom`, `depgraph.py:3787`, from `selected_atoms`, `dep_check.py:928-937`), so the unselected arms are not parent atoms and cannot veto a candidate.

## fn slot_conflict_abi_probe_keeps_an_or_arm_bound_to_the_installed_instance
- Backlog numbers removed from the comment (original text): Backlog #641 (residue of #623): a `||` menu arm the merge does not take stays bound to its kept installed instance instead of vetoing the probe. The installed consumer's live ebuild keeps `|| ( =abiprov-1 ... )` arms that exclude the new child 2 alongside the `:=` that pulls it; the old veto dropped the whole parent because no arm matched 2, so the provider missed the replace set (plain `U`). Portage records only `:=`/soname atoms in `_slot_operator_deps`, so menu pins never join the probe loop; an arm matching the kept installed instance keeps the group satisfied.

## fn slot_conflict_abi_probe_subsumed_child_is_display_only
- Backlog numbers removed from the comment (original text): Backlog #642: a filed child a merging upgrade subsumes is display-only. The probe files the installed child alongside the parent (Portage `_slot_operator_update_backtrack`), but when this pass merges a strictly newer version of the same `(cp, slot)` the fresh merge already carries the new ABI: seeding the child would ghost a same-version reinstall node beside the upgrade that Portage never builds (its restart re-walks with one provider node), costing a spurious mask restart. The child still lands in the display mirror, so the upgrade row keeps its `r` tag.

## fn slot_conflict_abi_probe_kept_child_files_the_seed_set
- Backlog numbers removed from the comment (original text): Backlog #642 control: without a merging upgrade the filed child stays in the seed set (the keep-both outcome of #641).

## fn slot_conflict_abi_probe_ignores_an_unselected_or_arm_for_the_new_child
- Backlog numbers removed from the comment (original text): Backlog #642: only a *selected* `||` arm can accept the candidate child through the replacement's live atoms. Portage intersects the candidate sets of the replacement parent's selected atoms (`atom_not_selected`, `all_candidate_pkgs`, `depgraph.py:2747-2790`); an unselected arm that tree-matches the candidate does not make the probe accept it. The live `|| ( =abiprov-1 =abiprov-2 )` group (no plain `:=` leg) is bound by the first graph-matching arm `=abiprov-1`, which rejects 2 while the unselected `=abiprov-2` accepts it -- so without `--update` (no #641 installed-arm fallback) the probe files nothing. With `--update` the installed arm still keeps the group satisfied and the parent is filed.

## fn slot_conflict_abi_probe_files_when_the_selected_or_arm_accepts
- Backlog numbers removed from the comment (original text): Backlog #642 control: when the *selected* `||` arm accepts the candidate child, the probe files the parent with or without `--update`. The live `|| ( =abiprov-2 =abiprov-1 )` group selects its first graph-matching arm `=abiprov-2`, which tree-matches 2.
