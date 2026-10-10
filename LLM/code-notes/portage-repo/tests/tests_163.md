# crates/portage-repo/src/tests/tests_163.rs
Result/display-assembly unit tests: abort outcomes, USE rows, pretend decisions, result assembly.

## module
- Backlog #163: `lib.rs` result/display-assembly unit tests. Every test observes the assembled result structures (fields of the returned result, rows, outcomes) directly -- never a contract pin's full text output. Scratch-repo harness in the #161/#162 style (`repo_pkgs_163` / `write_pkg_163` mirror `repo_pkgs_162` / `write_pkg_162`); self-contained so the other Track U branches' blocks rebase mechanically.

## section S1 (`abort_outcome`)
- Slice 3: Portage's abandon sites as data; the renderer switches to `partial` in Slice 4.

## section S2 (`use_unsat_parent_row`)
- Portage `_show_unsatisfied_dep`'s parent-flag flip row (depgraph.py:6756-6846): the requesting parent flips its own USE to satisfy a conditional use-dep.

## section S3 (`refresh_entry_use_display`)
- The post-walk USE-line re-render: Portage `_pkg_use_enabled` consults the flip tier for the display regardless of the walked graph.

## section S4 (`resolve_pretend`)
- The single-atom v1 pretend decision: best visible candidate vs installed; Portage `_wrapped_select_pkg_highest_available_imp` plus the `avoid_update` / fallback / autounmask levels around it.

## fn abort_outcome_masked_dep_aborts_with_the_merge_bound_requirer
- Pins the shape the S0 run already covers (its rows are all caught there).

## fn abort_outcome_ghost_requirer_completes_despite_a_same_category_merge
- The `&&` conjunction (and its second `==`) must not match the decoy. Pins the shape the S0 run already covers.

## fn abort_outcome_installed_only_requirer_aborts_with_the_installed_cpv
- Kills the `AlreadyInstalled` arm deletion (which drops the fallback and completes) and both `==` flips of the installed requirer lookup (which miss the requirer and complete).

## fn abort_outcome_ghost_requirer_completes_despite_a_same_category_installed_entry
- Kills the `&&` -> `||` flip and the second `==` -> `!=` flip of the installed requirer lookup (both match the decoy and abort).

## fn abort_outcome_masked_report_for_another_package_does_not_abort_as_masked
- Kills the `&&` -> `||` flip and the second `==` -> `!=` flip of the masked-report lookup (both match the neighbour report and abort as `MaskedDep`).

## fn abort_outcome_masked_report_for_the_same_package_in_another_category_does_not_abort_as_masked
- Pins the shape the S0 run already covers.

## fn abort_outcome_empty_graph_completes
- The cycle gate's `!` must stay. Pins the shape the S0 run already covers.

## fn abort_outcome_cycle_members_collect_the_matching_partial_in_order
- Pins the shape the S0 run already covers.

## fn abort_outcome_cycle_idx_keeps_both_root_instances_of_one_cpv
- Backlog #245/#287.

## fn use_unsat_parent_row_suggests_enabling_an_off_flag
- Portage `violated_conditionals` sees the parent-lacks-child-has mismatch as violated-conditional (dep/__init__.py:1541-1549); a `?` with the parent off would be satisfied instead (no row). Kills the whole-body `None` row and every whole-body `Some` row with a wrong value, plus the `!flag_is_settable` deletion (which returns `None` here).

## fn use_unsat_parent_row_suggests_disabling_an_on_flag
- Kills the `!parent_use.contains` deletion in the other direction (which suggests `+flag` here).

## fn use_unsat_parent_row_suggests_enabling_an_installed_parents_off_flag
- The r135 S0 shape: parent installed -flip, display atom `[flip=]` (`EqualParent`), vdb `USE` empty and vdb `IUSE` declaring flip, so the row proposes `+flip` on the parent's own cpv. Kills the `AlreadyInstalled`-blindness (no row for installed parents).
- An *installed* requesting parent flips on its own vdb-recorded USE (Portage `_pkg_use_enabled(myparent)` for a built package).

## fn chain_suffix_qualifies_a_flag_gated_merge_bound_edge
- The r135 S0 `(b)` shape: `extract_affecting_use` finds qml gating the edge, qml enabled in the node's effective USE. Kills the always-bare-row behaviour.

## fn chain_suffix_qualifies_a_gated_installed_edge_from_the_vdb
- The live `[installed]` `[qml]`/`[kde]` rows. Kills the installed-blindness (bare rows for installed nodes).

## fn use_unsat_parent_row_returns_none_without_an_owner_or_conditional_flags
- The `owner?` early return and the empty-flags early return.

## fn use_unsat_parent_row_notes_a_newly_violated_required_use
- Kills the `!new_sat` deletion (which drops the note here) and the `!is_empty` deletion of the `REQUIRED_USE` gate (which skips the check for a non-empty constraint and drops the note too).

## fn use_unsat_parent_row_omits_the_note_when_required_use_stays_satisfied
- Kills the `&&` -> `||` flip of the violation gate (which appends the note here).

## fn write_pkg_163
- The md5-cache validation guard fails otherwise.
- One scratch ebuild plus its md5-cache entry, in the #161/#162 shape: `SLOT`, `KEYWORDS`, `IUSE`, `RDEPEND` lines in both.

## fn resolve_163
- The `resolve` helper in the main test module has the same shape.

## fn resolve_pretend_fresh_top_level_atom_resolves_new_at_the_best_version
- Pins the baseline the S0 run already covers (its rows are caught or unviable there).

## fn resolve_pretend_emptytree_upgrades_past_the_installed_version
- Pins the `selective && !empty` conjunction the S0 run already covers.

## fn resolve_pretend_selective_top_level_keeps_the_installed_version
- Pins the shape the S0 run already covers.

## fn resolve_pretend_usepkgonly_pulls_a_binary_only_package
- Kills the `usepkg || usepkgonly` -> `&&` flip (which leaves the binary pool empty and reports `NoVisibleCandidate`).

## fn resolve_pretend_usepkg_dependency_falls_back_to_the_installed_version
- Pins the shape the S0 run already covers.

## fn resolve_pretend_dependency_avoids_updating_a_satisfying_installed_version
- Pins the early return all four `!`s guard.

## fn resolve_pretend_early_return_bypass_flags_each_upgrade_past_installed
- The dep baseline keeps the installed version. Pins the gate the S0 run already covers.

## fn resolve_pretend_plain_matching_candidate_needs_no_fallback
- Pins the probe the S0 run already covers.

## fn resolve_pretend_autounmask_keywords_relaxes_a_masked_candidate
- Pins the level loop the S0 run already covers.

## fn resolve_pretend_extra_constraints_narrow_and_hide_versions
- Pins the filters the S0 run already covers.

## fn resolve_pretend_vacuous_negative_extra_constraint_resolves
- Pins the probe the S0 run already covers.

## fn resolve_pretend_use_dep_atom_resolves_only_through_autounmask_use
- Pins the filter the S0 run already covers (its reachable rows are caught there; the `true` guard is unreachable -- see the report).

## fn resolve_pretend_unfixable_use_dep_atom_stays_unresolved
- Pins the shape the S0 run already covers.

## fn resolve_pretend_useoldpkg_prefers_and_exempts_built_candidates
- One test, one global set/reset pair on a dedicated `dev-libs/opkg` cp: the atoms are process-global, so they must match no other test's cp, and two setters would race each other's reset under parallel runs. Kills the `source == Binary` -> `!=` flip and the `&&` -> `||` flip of the oldpkg filter (both resolve the newer ebuild), and the `!uev` deletion of the exemption (which drops the exempt binary).

## fn resolve_pretend_excluded_installed_version_stays_installed_under_newuse
- Pins the shape the S0 run already covers.

## fn resolve_pretend_same_version_in_another_slot_is_a_new_slot_install
- Pins the shape the S0 run already covers.

## fn resolve_pretend_installed_same_version_is_already_installed
- Pins the predicate the S0 run already covers.

## fn resolve_pretend_early_return_honours_a_hiding_negative_constraint
- Kills both `!` deletions of the negative arm (each keeps the installed version and reports `AlreadyInstalled`).

## fn resolve_pretend_early_return_honours_a_pinning_positive_constraint
- The masked tree alone would be `NoVisibleCandidate`. Kills the positive-arm `!` deletion (which drops the installed version and reports the miss).

## fn resolve_pretend_visible_ebuild_disqualifies_a_version_skewed_binary
- Pins the drop the S0 run already covers (its rows are caught there).

## fn resolve_pretend_top_level_use_dep_atom_upgrades_past_a_satisfying_installed_version
- `--autounmask-use` keeps the flippable candidate in the pool so the gate is reached. Kills the `!is_top_level` deletion (which keeps the installed version; the neighbouring `&&` -> `||` is unviable).

## fn resolve_pretend_satisfiable_use_dep_atom_upgrades_under_update
- Kills the `!atom_currently_satisfiable` deletion (which keeps the installed version; the neighbouring `&&` -> `||` is unviable).

## fn resolve_pretend_unsatisfiable_use_dep_atom_keeps_the_installed_version
- `--autounmask-use` keeps the flippable candidate in the pool so the gate is reached. Kills the `!d.is_empty()` deletion (which skips the keep and upgrades).

## fn resolve_pretend_update_shortcut_bypass_shapes_upgrade_past_installed
- The plain dependency keeps the installed version. Pins the shortcut the S0 run already covers.

## fn resolve_pretend_newuse_with_a_gained_flag_reinstalls
- The gained flag (`b`) plus the dropped effective flag (`a`, on in the vdb but default-off in the new ebuild). Pins the trigger the S0 run already covers.

## fn resolve_pretend_installed_version_missing_the_flag_reinstalls
- Pins the flip check the S0 run already covers.

## fn resolve_pretend_triggers_fire_through_the_best_version_path
- Kills the `&&` -> `||` flip of the `changed_slot` gate (13711) and the `||` -> `&&` flips of the `changed_deps`, `changed_slot`, and `new_repo` disjunction arms (13741/13742/13744).

## fn resolve_pretend_each_reinstall_trigger_fires_on_its_own
- The option-on shapes reinstall with only their flag. Pins the shortcut shapes the S0 run already covers (the best-path arms live in the next test).

## fn resolve_pretend_reinstall_disjunction_fires_on_each_arm_alone
- A plain dependency keeps the installed version. Pins the disjunction the S0 run already covers.

## fn resolve_pretend_top_level_arm_needs_non_selective
- Kills the `&&` -> `||` flip of the top-level arm (which reinstalls bare; the `!selective` deletion is already caught).

## fn resolve_pretend_update_shortcut_holds_past_an_unmatched_exclude
- Pins the shape the S0 run already covers.

## fn resolve_pretend_upgrade_and_downgrade_follow_the_own_slot_version
- Pins the slot/version comparison the S0 run already covers.

## fn resolve_pretend_respect_use_drops_a_use_divergent_binary
- With no ebuild matching the atom the visibility filter stays out, so the gate alone decides: the divergent binary must not win. Kills both `||` -> `&&` flips of the gate (each keeps the binary and resolves it).

## fn resolve_pretend_respect_use_compares_against_the_same_version_ebuild
- Kills the `==` -> `!=` flip of the ebuild lookup (which compares against no ebuild, drops the binary, and reports the installed ebuild instead of the new slot), the `==` -> `!=` flip of the version probe (which blinds the retain the same way), and the `||` -> `&&` flip of the retain predicate (which drops the binary the same way).

## fn resolve_pretend_lonely_binary_with_divergent_use_is_dropped
- Kills the `&&` -> `||` and `==` -> `!=` flips of the ebuild lookup (each compares against the masked ebuild, accepts, and resolves the binary).

## fn resolve_pretend_changed_deps_drops_a_stale_binary
- Kills the `!binary_deps_changed` deletion (which keeps the binary and resolves its slot).

## fn resolve_pretend_masked_ebuild_does_not_match_for_visibility
- Kills the first `&&` -> `||` flip of the match probe (which counts the masked ebuild, runs the visibility filter, and drops the binary).

## fn resolve_pretend_pinned_atom_does_not_match_a_newer_ebuild
- Kills the second `&&` -> `||` flip of the match probe (which counts the newer ebuild, runs the filter, and drops the binary).

## fn resolve_pretend_kept_slot_skewed_binary_resolves_into_its_slot
- Kills the `||` -> `&&` flip of the retain predicate (which drops the binary and reports the installed ebuild instead).

## fn resolve_pretend_rebuilt_binaries_reinstalls_on_newer_build
- The `rebuilt` arm runs top-level selective under `--update` (past the update block with the top-level arm shut) so the rebuild flag alone decides; no shortcut, no bare arms. The `bare` arm fires, the flag stays false. Kills the `usepkg || usepkgonly` -> `&&` flip of the rebuild gate and the `||` -> `&&` flip of its disjunction arm (each reports the bare reinstall), and the `&&` -> `||` flip of the gate (which reports the rebuild reinstall without the flag).

## fn assemble_result_no_buildpkgonly_flag_means_no_buildpkgonly_refusal
- Pins the gate the S0 run already covers.

## fn assemble_result_lone_entries_are_no_buildpkgonly_edge
- The uninstall arm kills the `&&` -> `||` flip of the action filter (which reports an edge here); the merge arm pins the already-caught edge predicate.

## fn assemble_result_merge_requiring_a_merge_is_a_buildpkgonly_edge
- Pins the disjunction the S0 run already covers.

## fn assemble_result_chained_uninstalls_are_no_buildpkgonly_edge
- Pins the filter the S0 run already covers.

## fn assemble_result_recorded_autounmask_change_is_not_duplicated
- Kills the first-`==` -> `!=` flip of the dedup comparison (which duplicates the known atom and drops the new one); pins the already-caught gate and the second flip (whose duplicate the coalescing below repairs).

## fn assemble_result_new_token_for_a_known_atom_is_coalesced
- Kills the `&&` -> `||` flip of the dedup comparison (which drops the new token).

## fn assemble_result_coalescing_keeps_atoms_apart_and_flags_singular
- Kills the `==` -> `!=` flip of the coalescing lookup (which merges different atoms) and the `!any` deletion of the flag dedup (which doubles the flag).

## fn assemble_result_uncovered_nvc_gains_a_plain_miss_disclosure
- Kills the `!matches!(NoVisibleCandidate)` deletion (which files none).

## fn assemble_result_claimed_nvc_gains_no_plain_miss_row
- Kills the three `||` -> `&&` flips (each files a duplicate row) and the first-`==` -> `!=` flip inside each arm predicate (each misses its arm's exact claim and files a row).

## fn assemble_result_neighbour_claim_does_not_cover_the_entry
- Kills the `&&` -> `||` flips and the second-`==` -> `!=` flips inside each arm predicate (each treats the neighbour's claim as covering).

## fn assemble_result_extra_list_covers_only_its_own_cp
- Kills the `&&` -> `||` flip and the second-`==` -> `!=` flip of the extra-list predicate (each treats the first row as covering the second).

## fn assemble_result_extra_list_does_not_cover_across_categories
- Kills the first-`==` -> `!=` flip of the extra-list predicate (which treats the cross-category row as covering).

## fn assemble_result_use_config_change_refreshes_the_package_line
- Kills the `!autounmask_backtrack_enabled` deletion and the `!config.is_empty()` deletion (each skips the refresh and leaves the line empty).

## fn assemble_result_backtrack_on_leaves_the_walked_line_alone
- Kills the `&&` -> `||` flip of the refresh gate (which refreshes anyway).

## fn assemble_result_change_gains_its_required_by_chain
- Pins the derivation the `&&` -> `||` flip would also run on a preset chain.

## fn assemble_result_preset_chain_is_kept
- Pins the keep (the `&&` -> `||` flip is unviable, so no mutant reaches this shape).

## fn assemble_result_change_owner_is_the_matching_entry
- Kills the `&&` -> `||` flip of the owner lookup (which derives the chain from the decoy); pins the already-caught `==` flips.

## fn assemble_result_small_cycle_reports_no_large_count
- Pins the count the S0 run already covers.

## fn assemble_result_triangle_reports_three_cycles_without_large_count
- Kills the `>` -> `>=` flip (which reports a large cycle here); pins the already-caught `==` flip.

## fn assemble_result_slot_operator_scan_reports_the_rebuilt_consumer
- The scan runs exactly when neither the ignore flag nor a disabled rebuild gate suppresses it. Kills the match-guard `true` mutant (which reports no pairs here).

## fn assemble_result_slot_operator_scan_gates_suppress_the_pairs
- Kills the match-guard `false` mutant, the `||` -> `&&` flip, and the `!rebuild_if_new_slot` deletion (each runs the scan and reports the pair here).

## fn refresh_entry_use_display_renders_upgrade_and_downgrade_against_the_installed_base
- An `Upgrade` entry renders the target candidate with the installed instance as the diff base; a `Downgrade` the same in the other direction. Kills the combined `Upgrade|Downgrade`-arm deletion (both stay empty).

## fn refresh_entry_use_display_force_shows_a_dropped_reinstall_trigger_flag
- A `Reinstall` entry whose trigger flag the new ebuild dropped from IUSE still shows the `(-flag%*)` removed row at plain `-p` through the trigger set. Kills the `Reinstall`-arm deletion of the version lookup and the `Reinstall{changed_flags}`-arm deletion of the trigger set (both render an empty line here).

## fn ctx_163
- Literal `ResolveCtx` in the #161 `ctx_161` shape (same field list, so it tracks production), with the knobs each leg needs as parameters.
