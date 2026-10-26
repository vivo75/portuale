# Task 8 — pins that cover the four heuristics (written 2026-10-26)

Pins that must stay green through Tasks 9–10. **Keeper** = one of the four
shapes whose regression made the `dropped_pin_mask_restart` gates necessary
in the first place (#54 K3, #222, #223, #35 R4; portuale `2a2f7a09`,
"Fix round 1"). A keeper going red after one fix round stops the phase; a
non-keeper going red after one fix round keeps its heuristic (plan Task 10).

| Pin | Where | Heuristic it holds | Class |
|---|---|---|---|
| `test_keeper_reachable_pin_breaks_an_explicit_upgrade_and_reports_it` | pmtest contract | G1 (bare explicit-arg want, no heal) | **keeper** (#54 K3) |
| `test_keeper_reachable_pin_breaks_a_hard_dependency_requirement_and_reports_it` | pmtest contract | G1 | **keeper** (#54 K3) |
| `test_keeper_reachable_needer_triangle_reports_the_installed_instance_with_both_parents` | pmtest contract | G1 | **keeper** (#54 K3 family) |
| `test_complete_graph_use_break_fails_with_world_consumer` | pmtest contract | G1 (cgp) | **keeper** (#222) |
| `test_complete_graph_ver_break_fails_with_world_consumer` | pmtest contract | G1 (cgp) | **keeper** (#222) |
| `test_ignore_world_merges_only_the_arg_on_an_eapi_8_tree` | pmtest contract | G1 (igw) | **keeper** (#223) |
| `test_ignore_world_contrast_against_a_world_bound_consumer` | pmtest contract | G1 (igw) | **keeper** (#223) |
| `test_or_choice_avoids_downgrade_into_the_graphed_update` | pmtest contract | G2 (`||` choice, bug 531656) | **keeper** (#35 R4) |
| `p563_pull_heals_with_skip_block_and_backtrack_notice` | `portage-repo/src/tests/tests_p563.rs:22` | the heal itself (all gates open) | non-keeper (#563) |
| `p563_pinned_newver_aborts_with_slot_conflict` | `tests_p563.rs:137` | G1 (explicit newver arg aborts) | non-keeper (#563) |
| `p563_pinned_oldver_still_resolves_clean` | `tests_p563.rs:201` | heal does not fire on a clean resolve | non-keeper (#563) |
| `g0_benign_disclosure_blocks_the_dropped_pin_heal` | `tests_p563.rs:274` | G0 | non-keeper (#570) |
| `skip_block_pins_skip_non_selected_or_branches` | `tests_p563.rs:344` | `or_branch_vdb_deselected_set` (scan site) | non-keeper (#569) |
| `test_skip_block_keeps_the_built_atom_and_drops_non_selected_or_branches` (+ CASES row "#568/#569: the perl-heal skip block …") | pmtest contract | `or_branch_vdb_deselected_set` | non-keeper (#568/#569) |
| `p491or_seed_ignores_a_deselected_or_branch_slot_operator_atom` | `portage-repo/src/tests/tests_p491.rs:198` | `or_branch_vdb_deselected_set` (probe site; fixture pmtest `6bdb765`) | non-keeper (#575 Task 16) |

Bed: the L0 run on the phase-0 tree is `l0-phase0.txt` (this directory).
The 9 bed perl heals named in `reverse_dep_pins.md` (G2: "the 9 bed heals
keep firing") are L0 cells, and their names are in that file.
