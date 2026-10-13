# Functions over 200 code lines (backlog #339 input)

> **Final state (2026-10-13):** this table is the input record of #339 and is not edited.
> After slices R1-R12, D1-D6 and M1-M8 the only functions over 200 code lines are the six
> test functions in `LLM/plans/2026-10-11-339/baseline/out-of-scope.txt` and
> `graph_result_from_order` (open residue of #34 Task S6). Final list over 100 lines:
> `LLM/plans/2026-10-11-339/lengths-final.tsv`; `python3 LLM/tools/fn_lengths.py | tail -1` reads
> `over200 7 over150 62 over100 222 fns 5941`.


Method: parsed with `syn` (throwaway cargo project in scratch dir, `span-locations`); length counts body lines strictly between the function's braces that are neither blank nor comment-only (`//`, `///`, `//!`, nested `/* */` aware; strings/chars/raw-strings lexed so markers inside them don't count). Cross-checked with an independent Python brace-matching counter: identical counts for all 40. Scope: `crates/*/src/**/*.rs` plus `crates/*/tests/**/*.rs` (450 files, 5610 fns, 0 parse failures).

| lines | crate | file:line | fn |
| --- | --- | --- | --- |
| 1743 | portuale | crates/portuale/src/pretend/args.rs:119 | `parse_emerge_args` |
| 838 | portage-repo | crates/portage-repo/src/resolve/pass/mod.rs:33 | `run_pass` |
| 538 | portage-repo | crates/portage-repo/src/resolve/pretend.rs:370 | `resolve_pretend` |
| 459 | portuale | crates/portuale/src/pretend/display/entry_line.rs:54 | `print_entry_line` |
| 419 | portage-repo | crates/portage-repo/src/resolve/assemble.rs:5 | `assemble_result` |
| 413 | portuale | crates/portuale/src/pretend/mod.rs:148 | `run` |
| 406 | portage-repo | crates/portage-repo/src/resolve/pass/enqueue_deps.rs:25 | `enqueue_entry_deps` |
| 403 | portage-repo | crates/portage-repo/src/resolve/pass/unresolved_dep.rs:26 | `record_unresolved_dep` |
| 380 | portuale | crates/portuale/src/pretend/info.rs:317 | `run_info` |
| 375 | portage-repo | crates/portage-repo/src/resolve/slot_expand.rs:227 | `expand_resolved_slot_with_flipped_use` |
| 348 | portage-repo | crates/portage-repo/src/tests/unsat_probe.rs:8 | `slot_operator_unsatisfied_probe_seeds_a_replacement_parent` |
| 342 | portuale | crates/portuale/src/pretend/display/tree.rs:43 | `print_tree` |
| 336 | portage-repo | crates/portage-repo/src/solver_bridge.rs:337 | `graph_result_from_order` |
| 329 | portage-repo | crates/portage-repo/src/merge_order/select_nodes.rs:6 | `select_nodes` |
| 329 | portage-repo | crates/portage-repo/src/resolve/enqueue.rs:61 | `enqueue_dependencies` |
| 315 | portage-repo | crates/portage-repo/src/resolve/blockers.rs:515 | `resolve_blockers_with_seeds` |
| 315 | portage-repo | crates/portage-repo/src/resolve/direct_solve.rs:174 | `direct_solve_slot_conflicts` |
| 312 | portuale | crates/portuale/src/pretend/problem_reports.rs:379 | `show_slot_conflicts` |
| 308 | portage-repo | crates/portage-repo/src/resolve/rev_dep.rs:282 | `reverse_dependency_constraints` |
| 298 | portuale | crates/portuale/src/ebuild.rs:72 | `run` |
| 294 | portage-repo | crates/portage-repo/src/tests/blockers.rs:538 | `resolve_blockers_classifies_final_graph_matches_by_parent_operation` |
| 292 | portage-repo | crates/portage-repo/src/resolver_trace.rs:362 | `dump_resolution_walk` |
| 291 | portage-repo | crates/portage-repo/src/slot_operator/rebuild_scan.rs:35 | `slot_operator_rebuild_scan` |
| 281 | portuale | crates/portuale/src/ebuild_merge/from_binpkg.rs:31 | `merge_binpkg` |
| 279 | portage-repo | crates/portage-repo/src/resolve/pass/existing_slot.rs:24 | `step_existing_slot` |
| 265 | portuale | crates/portuale/src/pretend/removal/unmerge.rs:82 | `run_unmerge_pretend` |
| 246 | portage-repo | crates/portage-repo/src/tests/abi_probe.rs:6 | `slot_conflict_abi_probe_rebuilds_the_built_parent` |
| 241 | portage-repo | crates/portage-repo/src/tests/rebuild_scan.rs:253 | `slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider` |
| 233 | portuale | crates/portuale/src/pretend/standalone_actions.rs:18 | `dispatch_standalone_actions` |
| 231 | portage-repo | crates/portage-repo/src/resolve/slot_conflict.rs:515 | `build_residual_slot_conflicts` |
| 227 | portuale | crates/portuale/src/remote/binpkg_flow.rs:200 | `run_binpkg_flow` |
| 217 | portuale | crates/portuale/src/pretend/resolve_phases.rs:22 | `run_resolve_phases` |
| 216 | portage-repo | crates/portage-repo/src/tests/eliminate.rs:266 | `slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order` |
| 212 | portuale | crates/portuale/src/ebuild_merge/after_install.rs:5 | `merge_after_install` |
| 208 | portage-repo | crates/portage-repo/src/tests/tests_195d.rs:58 | `violated_parent_flags_matches_portage_across_the_oracle_matrix` |
| 208 | portuale | crates/portuale/src/ebuild_package.rs:1078 | `quickpkg_from_vdb` |
| 208 | portuale | crates/portuale/src/pretend/resume.rs:60 | `run_resume` |
| 204 | portuale | crates/portuale/src/pretend/merge_list.rs:129 | `render_merge_list` |
| 203 | portage-repo | crates/portage-repo/src/repo_config.rs:193 | `find_repos_impl` |
| 201 | portage-repo | crates/portage-repo/src/resolve/driver.rs:5 | `backtracking_resolve` |

Totals: 40 functions over 200 code lines, 19 over 300, 3 over 500.

## In test files

6 of the 40 are test fns (all under `crates/portage-repo/src/tests/`):

- 348 lines — crates/portage-repo/src/tests/unsat_probe.rs:8 `slot_operator_unsatisfied_probe_seeds_a_replacement_parent`
- 294 lines — crates/portage-repo/src/tests/blockers.rs:538 `resolve_blockers_classifies_final_graph_matches_by_parent_operation`
- 246 lines — crates/portage-repo/src/tests/abi_probe.rs:6 `slot_conflict_abi_probe_rebuilds_the_built_parent`
- 241 lines — crates/portage-repo/src/tests/rebuild_scan.rs:253 `slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider`
- 216 lines — crates/portage-repo/src/tests/eliminate.rs:266 `slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order`
- 208 lines — crates/portage-repo/src/tests/tests_195d.rs:58 `violated_parent_flags_matches_portage_across_the_oracle_matrix`
