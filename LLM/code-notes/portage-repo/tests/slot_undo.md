# crates/portage-repo/src/tests/slot_undo.rs
Single-pass (`run_161`) walk pins: leaf/NVC/reinstall, update, newuse, use-deps, slots.

## fn run_pass_use_mismatched_installed_instance_leaves_the_build_edge_hard
- Backlog #250: the `u250json -> u250make` edge, pmtest fixture; Portage's playground in `LLM/evidence/2026-09-30-250/`.

## fn run_pass_resolves_a_leaf_to_a_new_entry
- Backlog #161 S6.

## fn run_pass_reports_a_missing_atom_as_nvc
- Backlog #161 S6.

## fn run_pass_reinstalls_the_requested_installed_version
- Backlog #161 S6.

## fn run_pass_keeps_a_satisfied_dependency_installed
- Backlog #161 S6.

## fn run_pass_upgrades_an_older_installed_version
- Backlog #161 S6.

## fn run_pass_downgrades_a_newer_installed_version
- Backlog #161 S6.

## fn run_pass_walks_dependencies_and_records_parent_atoms
- Backlog #161 S6.

## fn run_pass_newuse_reinstalls_a_flag_change
- Backlog #161 S6: the vdb records no `wantdep`; the profile enables it.

## fn run_pass_revisiting_a_reinstall_reuses_the_entry
- Backlog #161 S6r: the `resolved_slots` revisit reads the entry's own version back out. Same shape as the use-unsat dedup pair, but installed, so the first outcome is a reinstall.

## fn run_pass_without_newuse_keeps_a_use_mismatch_quiet
- Backlog #161 S6.

## fn run_pass_usepkgonly_without_binaries_reports_nvc
- Backlog #161 S6.

## fn run_pass_nodeps_resolves_only_the_requested_atom
- Backlog #161 S6: no dep-driven rows appear.

## fn run_pass_selective_keeps_the_requested_version
- Backlog #161 S6.

## fn run_pass_resolves_multiple_atoms_in_order
- Backlog #161 S6: both merge.

## fn run_pass_satisfies_a_disabled_use_dep
- Backlog #161 S6.

## fn run_pass_reports_an_unenabled_use_dep_unsatisfied
- Backlog #161 S6.

## fn run_pass_dedups_use_unsat_reports
- Backlog #161 S6.

## fn run_pass_marks_a_new_slot_row
- Backlog #161 S6.

## fn run_pass_records_oldbest_for_a_moved_reinstall
- Backlog #161 S6: installed sub-slot 9 disagrees with the tree's 1.
