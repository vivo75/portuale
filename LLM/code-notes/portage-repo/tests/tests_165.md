# crates/portage-repo/src/tests/tests_165.rs
Graph-input unit tests (mutation cluster 5): walk/installed/vdb, merge-order, blockers, use.

## module
- Track U #165. Scratch-vdb / scratch-repo legs in the Phase 8 S2 style (own `dir_165` / `install_165` / `write_pkg_165` / `repo_165` / `entry_165` helpers copied from `tests_161`/`tests_162` -- not shared, so the other Track U branches' blocks rebase mechanically). Every test observes the function's own return value (returned vectors, the vdb fingerprint, filed conflicts, queued atoms) -- no end-to-end contract reproduction.
- Portage grounding: `vartree.dbapi` for the installed-set readers, `lib/_emerge/depgraph.py:8891 _validate_blockers` for the blocker arms, `repository/config.py` for `find_repos_impl`, `lib/_emerge/actions.py`'s unmerge ordering (`UnmergeDepPriority`, `ignore_priority_range` ~:1709) for `topological_removal_order`.
- Track U #165: `lib.rs` graph-input unit tests (mutation cluster 5) -- walk/installed/vdb, merge-order, blockers, use.

## section S1 (`installed_reverse_dependents`)
- Portage `vartree.dbapi`'s reverse-dependency scan: every installed package whose flattened runtime deps match the consumer's own `cat/pkg-ver:slot/sub-slot`, excluding the consumer itself.

## section S3 (`find_repos_impl` survivors)
- Portage `repository/config.py::RepoConfigLoader`. S0 already catches everything except the default-priority `==` and the no-`volatile`-key heuristic, which no existing leg observes.
- The no-`volatile`-key heuristic (Portage `config.py:423`): volatile unless under `/var/db/repos` and owned by root/portage.

## section S5 (`topological_removal_order`)
- Portage unmerge ordering: dependers before dependencies, cpv-descending ties, genuine cycles broken by Portage's `ignore_priority_range` scan.

## section S6 (`prune_cleanlist`)
- Portage `emerge --prune`'s selection: every non-highest version its args-set matches, minus whatever the dependency closure reaches.

## section S7 (`resolve_blockers`)
- Portage `_validate_blockers`: pending blocker atoms matched against installed + merge-bound targets, then filed as satisfied-uninstall, replacement, or unsolvable rows. The existing legs pin the `New` merge-bound arm, the use-dep filter, and the buildpkgonly gate; these legs pin the remaining outcome arms, the installed/graph dedup, the same-slot and installed-parent skips, and the nomerge unsolvable arms.

## section S8 (`file_blocker_conflicts`, `collect_unwalked_installed_blockers`)
- Hanging resolved rows on their owner entry, or -- with no owner entry -- on the `Uninstall` removal the row resolves by. Portage `_validate_blockers`' all-installed-packages scan for blockers the walk never reached.

## section S9 (`enqueue_dependencies`)
- The `--deep` AlreadyInstalled recursion: the installed package's own flattened deps go to the queue, blockers to pending. Called directly with `dynamic_deps = false`, so dep strings come from the vdb snapshot and only the version lookup needs the tree.

## fn installed_reverse_dependents_reports_a_plain_rdepend_consumer
- The whole-body `vec![]` / `vec![String::new()]` / `vec!["xyzzy"]` rows fail here.

## fn installed_reverse_dependents_keeps_siblings_and_other_versions
- Both `&&` -> `||` flips (which wrongly skip on a bare category or category+package match) fail here.

## fn installed_reverse_dependents_excludes_a_self_dependent_consumer
- Each `==` -> `!=` flip in the self-skip lets the consumer's own record through and reports it.

## fn installed_reverse_dependents_ignores_blockers_but_keeps_matches
- The blocker `!=` -> `==` flip would skip every plain dep instead; the match `delete !` flip would only file empty matches. Both pins in one leg.

## fn find_repos_impl_defaults_the_main_repo_to_priority_minus_1000
- The `*name == main_repo` -> `!=` flip swaps them and fails here.

## fn find_repos_impl_computes_volatile_from_location_and_owner
- An absolute location under `/var/db/repos` (which need not exist -- only the lexical prefix and the failed ownership stat matter) is still volatile for a non-root owner, pinning both the `||` -> `&&` flip and the `!owned` deletion. A second repo at a root-owned absolute location pins the `!starts_with` deletion; where the suite has no root-owned system dir that leg skips loudly instead of passing silently.
- The `&&` and `!owned` deletion both yield false on the unowned path.

## fn write_pkg_165
- The md5-cache validation guard fails otherwise.

## fn parent_use_state_resolves_a_new_parent_to_its_tree_version
- The whole-body `-> None` row and the `New`-arm deletion both fail here.

## fn parent_use_state_resolves_upgrade_downgrade_and_reinstall_parents
- Each deleted outcome arm returns `None` instead and fails here.

## fn parent_use_state_ignores_same_category_and_same_package_decoys
- The `&&` -> `||` widening (which accepts either decoy) and each `==` -> `!=` flip (which accepts its own decoy) fail here.

## fn parent_use_state_picks_the_parents_own_version_from_the_tree
- Pins the `c.version == version` -> `!=` flip (which would resolve 2.0).

## fn topological_removal_order_unmerges_a_depend_chain_dependers_first
- The whole-body rows, the `n < 2` widenings, the no-edge (`is_some_and`/`contains`) deletions, the `indeg` narrowings, the ready-batch mutants, and -- crucially -- the `!ready` deletion all fail here. That mutant only diverges (spins the `ready.is_empty()` branch forever) on cyclic input; on this DAG it terminates and merely pops in cycle-break order, so this leg kills it quickly with no timeout involved.

## fn topological_removal_order_ignores_a_self_dependency
- Both `||` -> `&&` narrowings (which admit the self-edge, pushing the package through the cycle-break path instead) fail here.

## fn topological_removal_order_follows_a_cross_category_edge
- The `i == j` -> `!=` flip (which drops every edge) and both category/package `!=` -> `==` flips (which drop every cross-cp edge) fail here.

## fn topological_removal_order_breaks_cycles_by_dependency_key_priority
- Each deleted `match` arm demotes its key to -4 and pops the wrong node first here. The IDEPEND-arm deletion pops `zz` at -4 instead; the RDEPEND-arm deletion ties both at -4 and pops cpv-max `zz` instead; the PDEPEND-arm deletion ties both at -4 and pops `zz`.

## fn topological_removal_order_breaks_cycles_by_dependency_key_priority (mutual PDEPEND)
- The PDEPEND `-3` -> `3` flip strands both (no level ever eligible) and returns nothing.

## fn topological_removal_order_keeps_a_sub_slot_less_slot_operator_at_rdepend_priority
- Both `slot_op_built` narrowings (`&&` -> `||`), which would promote it to -1 and pop `aa` instead of `zz`, fail here.

## fn topological_removal_order_scans_ignore_levels_before_popping
- Each `delete -` in the -4/-3/-2 scan levels turns its level positive, so both nodes are eligible early and cpv-max `zz` pops instead. The -1 level itself can never pop a matchable edge (every matchable priority is -4..-2 or 0, and a node eligible at -1 was already eligible at -2), so its own `delete -` is equivalent; classified in the closeout, not pinned here.

## fn topological_removal_order_pops_a_three_cycle_cpv_max_first
- The cycle-break candidate mutants (stale-`done` inclusion or exclusion), the `<=` -> `>` flip, and the early-`break` deletion all return a short or duplicated order and fail the exact assertion here.

## fn prune_cleanlist_prunes_the_older_of_two_versions
- The whole-body `Default` row does not compile (tool-reported unviable). The `is_highest`/`is_candidate`/seed/cleanlist mutants (which empty, swap, or flood the cleanlist), and the `counts`/`multi_version` narrowings all fail here, as does the highest-version guard's `!= Greater` -> `==` flip (which would protect the lowest and prune 2.0 instead).

## fn prune_cleanlist_protects_the_highest_regardless_of_install_order
- Versions whose lexicographic and version orders disagree (9.0 sorts after 10.0 but compares lower); the guard's `-> false` flip (which protects whatever version the directory scan yielded last) protects 9.0 and prunes 10.0 here, under either scan discipline (sorted readdir, which this host's tmpfs yields, or creation order).

## fn prune_cleanlist_honours_versioned_atoms_in_args
- The args `category`/`package` flips and the match `delete !` (which unmatch everything) fail the companion assertion; the outer `&&` -> `||` (which also matches 1.0 by its bare cp, pruning it) fails here.

## fn prune_cleanlist_keeps_a_version_reached_through_the_walk
- The `matches_atom` narrowings (which blind the walk) and the cleanlist `||`/`&&` flips fail here.

## fn prune_cleanlist_reports_the_keeper_of_a_kept_version
- Single-version `bb` is unmatched by the empty-args set even though the walk records an edge into it. The `kept_parents` `delete !` (which reports nothing) fails here, and the `multi_version` `>=` widening (which would additionally keep `bb` with its own parent line) fails on the exact row count.

## fn resolve_blockers_matches_upgrade_downgrade_and_reinstall_targets
- Each deleted outcome arm drops its version from the match and files nothing here.

## fn resolve_blockers_sees_a_merge_bound_target_beside_its_installed_slot
- The `!candidates.any` deletion (which drops the graph version) and the slot `==` -> `!=` flip (which drops it too) both miss the `:1` blocker here.

## fn resolve_blockers_requires_slot_and_version_for_an_installed_match
- The matched instance is not the installed one. The `&&` -> `||` flip (16930:49) self-matches on the version alone and misfiles it as a slot `Replacement` once a second same-slot entry (`target-2.0:1`) satisfies the replaced-in-slot arm -- so this leg kills it.
- `installed_match` needs slot *and* version: installed `target-1.0:0` beside the merge-bound twin `target-1.0:1` leaves the `:1` row an unsolvable uninstall.

## fn resolve_blockers_keeps_a_strong_blocker_against_its_own_slot
- The `!strong` deletion and the slot `&&`/`==` narrowings (which would all skip it) fail here.

## fn resolve_blockers_ignores_an_installed_only_match_for_a_nomerge_owner
- Every `&&` -> `||` widening and `!` deletion in the cell files a row instead and fails here.

## fn resolve_blockers_unblocks_a_nomerge_row_outside_the_required_sets
- Without the closure the same row is solvable. The `owner_set_parent` `||` -> `&&` narrowing (which drops the set term) fails the first leg; the `merge_bound_match &&` deletion pins the solvable side in the second leg.

## fn resolve_blockers_evaluates_use_deps_against_the_matched_versions_entry
- `targetA` is New 1.0 with the flag off, `targetB` is an Upgrade to 2.0 with the flag on. Every `&&` -> `||` widening and `==` -> `!=` flip in the graphed-USE entry lookup files the wrong set (nothing, or the 1.0 row too) and fails here.

## fn resolve_blockers_needs_slot_and_version_for_a_merge_bound_match
- The `installed_match`/`merge_bound_match` `&&` -> `||` widening marks it merge-bound instead and fails here.

## fn resolve_blockers_falls_back_to_a_same_cp_nomerge_owner_only
- The `&&` -> `||` widening flips the owner to nomerge and solves the merge-bound row instead of blocking on it.

## fn file_blocker_conflicts_files_an_owned_row_on_its_entry_and_removal
- The whole-body row (which files nothing anywhere) and the owner `==` flip (which misses the entry) both fail here. The deleted `Uninstall` match arm does not compile (tool-reported unviable).

## fn file_blocker_conflicts_merges_two_owned_rows_into_one_removal
- Each removal-lookup `==` -> `!=` flip spawns a duplicate removal, and the anchor `delete !` duplicates the anchor -- all fail the exact assertions here.

## fn file_blocker_conflicts_keeps_different_removals_separate
- Both removal-lookup `&&` -> `||` widenings (which merge on a bare category or category+package match) fail here.

## fn file_blocker_conflicts_rides_ownerless_rows_on_their_removals
- The no-owner lookup `==` -> `!=` flips (which split the shared removal) and its `&&` -> `||` widenings (which fuse the separate one) fail here.

## fn collect_unwalked_installed_blockers_files_an_unwalked_strong_blocker
- Every whole-body / early-return / token-filter mutant empties the pending list and fails here.

## fn collect_unwalked_installed_blockers_skips_walked_cps_and_nomerge_runs
- Both legs pin the early returns.

## fn collect_unwalked_installed_blockers_dedups_exact_rows_only
- Each `==` -> `!=` flip re-files the identical row; each `&&` -> `||` widening swallows the different-atom row.

## fn enqueue_dependencies_queues_a_plain_dep_on_an_installed_package
- The whole-body row, the `||`-token skip, and every satisfied-self widening (which would drop it) fail here.

## fn enqueue_dependencies_drops_a_dep_satisfied_by_the_instance_itself
- The `version == owner_version` -> `!=` flip re-queues it and fails here.

## fn enqueue_dependencies_files_a_strong_blocker_into_pending
- The blocker `!=` -> `==` flip (which queues it as a dep instead) and the `Strong` -> inverted flip fail here.

## fn enqueue_dependencies_marks_a_depend_blocker_as_buildtime
- Pins the buildtime-flatten `!=` -> `==` flip (which would record it as runtime).

## fn enqueue_dependencies_records_the_unevaluated_form_of_a_conditional_dep
- The `evaluated != raw` -> `==` flip swaps which rows carry `unevaluated` and fails here.

## fn enqueue_dependencies_queues_plains_before_disjunction_choices
- Both partition mutants flip the order here.
