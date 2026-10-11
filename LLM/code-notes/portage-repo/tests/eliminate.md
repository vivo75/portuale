# crates/portage-repo/src/tests/eliminate.rs
Slot-operator eliminate/scan tests: bridge arm, lookup, eight rules, reachability.

## fn slot_operator_bridge_entries_run_the_new_slot_arm
- Backlog #211 I1, expected value from Portage: the `--solver=` bridge fixpoint runs the new-slot update arm once `update`/`top_level_cps` are threaded through, so both solvers agree. A slot-moving provider merged fresh schedules the walked consumer bound to the old slot; without `--update` (and no arg-chain) the arm stays off (Portage's `want_update` gate). With `--update` the bridge synthesises the rebuild exactly like the default path's scan-then-seed.

## fn slot_operator_eliminate_rebuilds_lookup_matches_the_replace_cp
- Backlog #161 S1: a decoy installed package of the same category that sorts first never satisfies the lookup (a loosened `||`/`!=` lookup lands there and rule 1 keeps the rebuild; the exact lookup walks past it to `sounneed` and rule 8 demotes), and a pin filed for another cp never filters into this cp's parent match.

## fn slot_operator_rebuild_entries_use_the_consumer_tree_slot_and_repo
- Backlog #161 S1: `souprov-1.0` is 0/1; 2.0's 0/2 must not leak in.

## fn slot_operator_rebuild_entries_mark_repo_drifted_rebuilds_oldbest
- Backlog #274: the old post-pass slot-op synthesiser's own `oldbest` assembly carries #247's third disjunct (Portage `resolver/output.py:721-731`); the entry itself keeps the tree repo (Portage `pkg.repo`).

## fn slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order
- Fixture: `sounneed-1.0` records `soflag? ( souprov:0/1= )`; `souprov` upgrades 1.0(0/1) -> 2.0(0/2) in the graph.
- Rule 3 (S5): the `souneedslot` fixture (vdb 0/1, tree 0/2) is the positive shape; rule 3 is behaviour-neutral for ebuilds (documented), and rule 6 keeps it without the flag too, so both must agree.
- Rule 5: non-selective keeps a user-requested package; selective ignores the argument (Portage's `selective` arm).
- Rule 7: binary halves are v2.

## fn required_set_reachable_cps_follows_the_forward_installed_dep_graph
- Mirrors the container's CASE 2 / CASE 3. `sysmember` in @system pulls `sysdep`.
