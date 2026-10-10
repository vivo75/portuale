# crates/portage-repo/src/tests/tests_162.rs
Direct unit tests for the eight pure selection predicates (mutation cluster 2).

## module
- Backlog #162 (`lib.rs` mutation cluster 2): P7b's `cargo mutants` run left ~102 missed (`TEST/findings/mutants.md` "## `portage-repo/src/lib.rs`"). Each test observes the predicate's own return value -- no end-to-end contract reproduction -- so every in-body operator flip changes an asserted outcome. Kept self-contained (own config, scratch repo/vdb/entry helpers) so the other Track U branches' blocks rebase mechanically.

## section S1 (`bare_cp`)
- Portage `3rdparty/portage/lib/portage/update.py` `parse_updates`: `atom.blocker or atom != atom.cp` rejects everything but a bare `cat/pkg`; the `slotmove` slot side is `valid_slot_name`'s job, not this predicate's.

## section S2 (`clean_selection`)
- Portage `emerge --prune --nodeps` selection, `3rdparty/portage` `unmerge.py` `_unmerge_display`: every cp with more than one matched version keeps the highest and selects the rest.

## section S5 (`disjunction_preference`)
- Portage `dep_check.py` `dep_zapdeps`' whole choice-bin classification, soft 449-523 and 599-724: `Unsatisfiable` < `Other` < `OtherInstalledAnySlot` < `OtherInstalledSome` < `OtherInstalled` < `UnsatUseNonInstalled` < `UnsatUseInstalled` < `UnsatUseInGraph` < `Available` < `Installed`. `self_cp` is a non-tree consumer everywhere except the circular legs.

## section S5b (`circular_atom` demotion)
- Backlog #221: Portage `dep_zapdeps` (dep_check.py:673-691, bugs 703440, 705986): on the restart after a stranded walk, an alternative with an atom matching a recorded in-cycle child of the resolving parent ranks `Other`, so the re-walk takes the cycle-breaking branch.

## section S6 (`promote_tied_alternative`)
- Portage `dep_zapdeps`'s in-bin upgrade-preference pass, `dep_check.py` soft 738-802: `all_installed_slots` first, then a strict upgrade (no downgrade anywhere on a shared cp), then the in-graph alternative.

## section S7 (`alternative_downgrade_demoted`)
- Portage `depgraph` bug-531656 `conflict_downgrade`/`installed_downgrade`: an otherwise-available `||` alternative demotes to `other` when it would install below what the graph already holds and no visible candidate makes the downgrade desirable.

## fn bare_cp_accepts_a_plain_cat_pkg
- The whole-body `None`/`Some((...)` mutants fail here.

## fn bare_cp_rejects_every_non_bare_form
- Every `||`-arm flip (`||` -> `&&` returns `Some` for a single-violation input) and both `!=` flips fail here.

## fn qatom_162
- The `#90 (S1)` drain-state legs.

## fn disj_162_circ
- Backlogs #216/#221.

## fn clean_selection_reports_every_multi_version_cp
- Kills the whole-body `vec![]` row, the three `versions.len() < 2` flips and the cross-slot best flip.

## fn clean_selection_version_pinned_atom_selects_only_its_set
- The `==` flips on the atom/cp guard fail here.

## fn clean_selection_impossible_version_matches_nothing
- The `parse && match` flip (`&&` -> `||` admits the cp-matched but version-missed package) and the `!is_empty` deletion (which admits it the other way round) both fail here.

## fn complete_graph_auto_enable_upgrade_and_downgrade_follow_if_new_ver
- The deleted `Reinstall`-arm neighbour legs live below; both whole-body rows (already S0-caught) fail here too.

## fn complete_graph_auto_enable_new_only_for_a_new_slot
- The `e.new_slot` guard flips and the `||` -> `&&` flip fail here.

## fn complete_graph_auto_enable_reinstall_needs_churn
- The deleted `Reinstall` arm, the `||` -> `&&` flip and the `!changed_flags` deletion fail here.

## fn complete_graph_auto_enable_reinstall_slot_change_needs_if_new_ver
- The two remaining `&&`/`||` flips fail here.

## fn complete_graph_auto_enable_settled_entries_stay_off
- Pins the `_ => false` arm the mutants never touch.

## fn check_if_latest_atom_form_renders_gte_when_nothing_is_higher
- The whole-body rows (already S0-caught) plus the outer `!higher` deletion and the `&&` widening inside the probe fail here.

## fn check_if_latest_atom_form_renders_slot_when_only_other_slots_are_higher
- The `||` narrowing, the `Greater` flip and the inner `!higher` deletion fail here.

## fn check_if_latest_atom_form_renders_equals_when_its_slot_is_higher
- The `!same_slot` deletion and the same-slot `Greater` flip fail here.

## fn check_if_latest_atom_form_ignores_a_masked_higher_candidate
- The visibility `&&` widening and the `!check_visibility` deletion (masked side) fail here.

## fn check_if_latest_atom_form_counts_a_masked_candidate_without_visibility
- The `||` narrowing and the `!check_visibility` deletion (unmasked side) fail here.

## fn write_pkg_162
- The md5-cache validation guard fails otherwise.

## fn disjunction_preference_ranks_a_fresh_alternative_available
- The `avail || running_root` narrowing, the `!avail` deletion, the `!all_available` deletion and the whole-body `Default` (which is `Unsatisfiable`) fail here.

## fn disjunction_preference_ranks_an_installed_alternative_installed
- The `installed || in_graph` narrowing fails here.

## fn disjunction_preference_rejects_a_circular_self_alternative
- Backlog #216 narrows the old unconditional refusal; the blocker-`==` flip fails here.

## fn disjunction_preference_keeps_an_installed_self_alternative
- The `!installed` deletion fails here.

## fn disjunction_preference_keeps_an_in_graph_self_alternative
- Backlog #216. The cycle forms, then the `circular_dependency` map re-resolves it. The in-graph `&&` widening fails here.

## fn disjunction_preference_cross_slot_atom_is_not_in_graph
- g221c (L0 `dev-lang/rust`): the retry takes `-bin`. The slot-blind candidate used to rank it `Installed`, beating `-bin` and re-closing the cycle through the older slot's own self edge. The `Available` (not `Unsatisfiable`) also pins that the circular-self gate let it through (see the next test).

## fn disjunction_preference_cross_slot_self_atom_still_proceeds
- Ranks `Available` (the bins use the slot-precise rule), never `Unsatisfiable`: slot precision here would wrongly refuse it as circular.

## fn disjunction_preference_demotes_a_recorded_circular_self_alternative
- Backlog #216: Portage `dep_zapdeps`' `circular_atom` (dep_check.py:688-691). The re-resolve takes the bootstrap branch. The demotion `return` fails here.

## fn disjunction_preference_circular_demotion_spares_an_installed_atom
- Backlog #216. Still `Installed`; the exemption `continue` fails here.

## fn disjunction_preference_keeps_an_uninstalled_other_alternative
- The `cp == self` flip and both circular-`&&` widenings fail here.

## fn disjunction_preference_counts_a_queued_atom_as_in_graph
- The `#90 (S1)` drain-state shortcut.

## fn disjunction_preference_ranks_a_use_unsatisfiable_alternative
- The use-block shortcut flip, the `if use_ok` inversion and the masked-probe inversion (unmasked side) fail here.

## fn disjunction_preference_demotes_a_masked_use_violation_to_other
- The `!all_use_unmasked` deletion and the masked-probe inversion (masked side) fail here.

## fn disjunction_preference_matches_an_installed_alternatives_vdb_use_direct
- Both installed-USE `||` narrowings fail here.

## fn disjunction_preference_needs_the_target_slot_installed
- The slot-check `&&` widening fails here.

## fn disjunction_preference_ranks_an_unavailable_group_other
- The blocker-filter flip and the cp-level `!is_empty` deletion fail here.

## fn disjunction_preference_ranks_a_fully_installed_group_other_installed
- The cp-level `is_empty` flip (installed side) fails here.

## fn disjunction_preference_demotes_a_recorded_circular_child_to_other
- Otherwise `Installed` via the in-graph branch; the empty-map leg fails here.

## fn disjunction_preference_ignores_a_non_matching_circular_record
- The lookup-key and the `atom.match(child)` flips fail here.

## fn disjunction_preference_exempts_an_installed_satisfying_circular_atom
- The exemption deletion fails here.

## fn promote_tied_alternative_prefers_the_strict_upgrade
- The whole-body `-> 0`, two `< 2` early-return flips, the break-on-self flip, the position flip and the upgrade-arm `||` narrowing fail here.

## fn promote_tied_alternative_keeps_disjoint_order
- The whole-body `-> 1` (already S0-caught) fails here too.

## fn promote_tied_alternative_promotes_through_a_longer_list
- The `< 2` -> `>` flip (which returns 0 for any longer list) fails here.

## fn promote_tied_alternative_rejects_a_mixed_upgrade_and_downgrade
- Both mixed-shape flips fail here.

## fn promote_tied_alternative_prefers_the_in_graph_alternative
- The two `!...` deletions and the inner-`&&` widening in the in-graph arm fail here.

## fn promote_tied_alternative_keeps_order_when_both_are_in_graph
- The two outer-`&&` widenings in the in-graph arm (already S0-caught) fail here too.

## fn promote_tied_alternative_rejects_an_in_graph_downgrade
- The `!has_upgrade` deletion in the downgrade guard fails here.

## fn promote_tied_alternative_prefers_the_installed_slots_alternative
- The `!all_installed_slots` deletion fails here.

## fn promote_tied_alternative_rejects_an_uninstalled_alternative
- The `all_installed_slots &&` widening (already S0-caught) fails here too.

## fn promote_tied_alternative_needs_the_candidate_slot_installed
- The slot-check `&&` widening fails here.

## fn promote_tied_alternative_ignores_blockers_in_cp_maps
- The blocker-skip flip fails here.

## fn promote_tied_alternative_returns_zero_for_a_single_alternative
- Pins the early return the `< 2` mutants never touch.

## fn alternative_downgrade_demoted_flags_an_older_alternative
- The whole-body `-> false`, the three graph-filter comparison flips, the `!= Less` flip and the `!downgrade_probe` deletion fail here.

## fn alternative_downgrade_demoted_keeps_the_graphed_version
- The whole-body `-> true` (already S0-caught) plus the graph-filter `&&` widenings fail here (a widened filter would admit the decoy other-slot entry and demote).

## fn alternative_downgrade_demoted_skips_the_resolving_package
- Both skip-guard flips (`||` -> `&&`, `==` -> `!=`) fail here.

## fn alternative_downgrade_demoted_skips_blockers
- The blocker-guard flip (`!=` -> `==`, which would evaluate the blocker against the graph and demote) fails here.

## fn alternative_downgrade_demoted_uses_the_queued_hypothetical
- The `#90 (S1)` hypothetical: the queued-filter comparison flips and the hypothetical-match `!is_empty` deletion fail here.

## fn alternative_downgrade_demoted_ignores_other_cp_queue
- The queued-filter `||` -> `&&` flip (which would fold the other cp's installed versions in and demote) fails here.

## fn alternative_downgrade_demoted_uses_the_tree_hypothetical_under_update
- Pins the update arm the queued-filter mutants never touch.
