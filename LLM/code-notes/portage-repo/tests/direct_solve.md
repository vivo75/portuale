# crates/portage-repo/src/tests/direct_solve.rs
Direct-solve unit tests: tuple forcing, gates, sink fallbacks.

## fn direct_solve_removes_the_unforced_merge_and_reports_the_skip
- The 2-atom old/new shape (S0 matrix). Expected value from Portage: the `remove: 2.0 / keep: 1.0` verdict and `WARNING` text (staged oracle).

## fn direct_solve_reports_an_argument_parent_with_the_cli_text
- Backlog #230 fix round 1 (review Important 3), expected value from Portage: Portage's `PackageArg`/`AtomArg` arm (`depgraph.py:1681-1686`) prints `str(parent)` with no atom and no `^` marker. The direct-solve producer is the one path that can supply it (top-level argument puller recorded with empty parent fields and the argument text as its atom; see `slot_pullers`' depth-0 ownerless push), so the skip row carries the CLI text in `atom` with an empty `consumer_cpv`, and the renderer printing `s.atom` bare reproduces Portage's arm exactly.
- The reverse-pin producer always builds a non-empty consumer cpv; the backtrack-mask rows skip empty-category pullers outright. No contract or bed cell grounds an Argument parent end to end (blk0 parents are all Packages), so this producer-level pin is the only coverage; the renderer arm beyond it stays unpinned.

## fn direct_solve_keeps_an_unsolvable_conflict_untouched
- Both instances singly forced (newpin's `>=2.0`, oldpin's `<2.0`): rc 1, like Portage.

## fn direct_solve_skips_slot_operator_rebuild_conflicts
- Keyed in `slot_operator_replace_installed`: Portage never feeds those to the direct solve (`:1784-1788`); #24 territory, not #90.

## fn direct_solve_arg_parents_protect_the_triangle
- The #57 triangle: installed 1.0 excluded from every argv parent's match list (`is_arg_parent`: top-level pull with an update wanted), so `othermod`'s `<2.0` matches nothing and protects the whole conflict. Expected value from Portage (staged oracle, both argv orders), rc 1.

## fn direct_solve_orders_merge_instances_before_installed
- Backlog #161 S5: the `or_tuple` first-pulled rule. Walk order names 1.0 first, but the merge instance must still outrank the installed one; `arg_mode` stays off with no naming arg.

## fn direct_solve_tuple_order_follows_walk_order
- Backlog #161 S5: with 1.0 walked first, an ambiguous bare/bare conflict drops 2.0.

## fn direct_solve_ignores_parents_outside_the_conflict
- Backlog #161 S5; the S0 verdict stands.

## fn direct_solve_solves_past_a_non_built_parent
- Backlog #161 S5: a slot-only atom is not a built op.

## fn direct_solve_solves_past_an_uninstalled_parent
- Backlog #161 S5: the fixture vdb holds `paired-1.0`, so `paired-9.9` is not installed.

## fn direct_solve_exclude_alone_arms_the_rebuild_gate
- Backlog #161 S5: the `useoldpkg` alternative need not match too. Portage's rebuild-path residue, not remove-path material.

## fn direct_solve_replace_set_holds_the_conflict
- Backlog #161 S5: even when no rebuild parent exists.

## fn direct_solve_lone_bare_parent_solves_through_the_tuple
- Backlog #161 S5: drops the later-walked instance.

## fn direct_solve_ambiguous_conflict_prefers_the_walked_instance
- Backlog #161 S5: over three `mgfc` versions with only 2.0 walked, the two unforced merges drop in rank order behind the installed keeper; the 1-vs-2 residual survives for the mask trial.

## fn direct_solve_lone_outside_parent_solves_through_the_tuple
- Backlog #161 S5: misattributing it to an instance must change the verdict, not pass silently.

## fn direct_solve_argless_forcing_drops_the_unforced_merge
- Backlog #161 S5: the verdict must not depend on which path (tuple or direct edge) each parent takes.

## fn direct_solve_unresolvable_parent_falls_back_to_the_sink
- Backlog #161 S5: a parent triple naming a version that does not exist, instead of hijacking an instance edge.

## fn direct_solve_mismatched_parent_installedness_falls_back_to_the_sink
- Backlog #161 S5: a merge instance pulled "as installed".

## fn direct_solve_unreached_parent_edge_deletes_quietly
- Backlog #161 S5: the degenerate shape the take-over documents (no edge reached anything, no keeper owed).
