# B450 analysis: contract-suite quality (read-only)

Scope: `/home/vivo/repo/PORTUALE/pmtest/pytests-contract-suite` at pmtest branch state on 2026-10-20 (read-only). Method: Python `ast` over every `test_*.py` (`python3 -I`), resolving literal argv through local and module-level assignments and `+` concatenation. Scripts and raw output live in this directory: `analyze_core.py`, `analyze_main.py`, `analyze_bc.py`, `analyze_d.py`, `analyze_d2.py`, `write_md.py`, `raw*.json`. Nothing was edited, committed or blessed.

Caveats that apply to every section: (1) an "argv" match ignores the environment (`fixture_env` or a per-test env), so two calls with the same argv can hit different fixtures; (2) `ast` ignores formatting and comments, so "identical" means identical code with literals, not identical text; (3) counts are per function, not per parametrized case, unless stated.

## Counts at a glance

| file | test functions |
|---|---|
| `test_emerge_pretend_contract.py` | 912 |
| `test_portuale.py` | 262 |
| `test_registry.py` | 19 |
| `test_output_invariants.py` | 18 |
| `test_cached_harness.py` | 10 |
| `test_atom_contract.py` | 8 |
| `test_translator_floor.py` | 8 |
| `test_versions_contract.py` | 5 |
| `test_required_use_contract.py` | 4 |
| `test_use_reduce_contract.py` | 4 |
| `test_metamorphic.py` | 1 |
| `test_primitive_regressions.py` | 1 |
| `test_fixture_caches.py` | 1 |
| `test_musl_smoke.py` | 1 |
| `test_benchmark_gate.py` | 1 |
| `test_harvested_corpus.py` | 1 |

## A. Exact duplicates

Result: **1 identical-except-docstring pair** (pretend), **1 identical pair that is dead code** (portuale), **1 same-name pair that is dead code with a different body** (portuale), and **1 identical helper pair** (portuale). No duplicate test names exist in `test_emerge_pretend_contract.py`.

| # | item | location | keep | why |
|---|---|---|---|---|
| A1 | `test_autounmask_use_parent_flip_suggestion_is_suppressed_by_autounmask_use_n` (`test_emerge_pretend_contract.py:10612`) vs `test_use_dep_equal_parent_mismatches_when_parent_flag_is_disabled` (`:15520`) | same body (argv `--pretend --autounmask-use=n dev-libs/useeqparentoffpkg`, rc 1, `_assert_abort_preamble`, identical stderr list); docstring differs only | `:15520` | its docstring explains the `opt=` truth table and why the abort is the expected outcome; `:10612` tests the same abort under the autounmask-use gate story. Delete `:10612` (plain delete; its corpus keys become orphans, see F). |
| A2 | `test_direct_ebuild_merge_prints_the_portage_listing` at `test_portuale.py:1668` and `:6867` | identical code AND docstring | `:6867` | Python keeps the LAST definition, so `:1668` is never collected. Deleting it changes no test outcome. |
| A3 | `test_direct_ebuild_qmerge_replacement_prints_safely_unmerging` at `test_portuale.py:1721` and `:6920` | same name; `:1721` has 4 extra asserts (`payload-2.0.txt` is_file, `payload-1.0.txt` absent, `var/db/pkg/.../binpkgrmpkg-2.0/CONTENTS` is_file, `var/db/pkg/.../binpkgrmpkg-1.0` absent) | `:6920` plus the 4 asserts | `:1721` is DEAD: the 4 filesystem assertions have never run. This is not a pure deletion. Copy the 4 asserts into `:6920`, run the test, then delete `:1721`. If it fails, that is a real finding to triage, not a reason to just delete. |
| A4 | `_remote_rmpkg_args` at `test_portuale.py:1536` and `:1593` (module-level helper) | identical code | `:1593` (the live def) | Module-level redefinition; identical, so deleting one is behaviour-neutral. |

Other name collisions checked: `run`, `install`, `rebuilds`, `resolve`, `merge`, `_run`, `_fresh_root`, `_root`, `_env`, `log_message` are each defined more than once, but all of those extra defs are nested inside tests (not module level) except the ones listed above. `one` in `test_output_invariants.py` is nested (5 times). No cross-file duplicate names among the test modules.

Identical-code groups found by the AST pass (all test modules): 2 groups, 4 tests (A1 and A2). The other test modules have none.

## B. Same argv, same expectation literal

Method: the key is (argv, set of string literals longer than 3 chars and list literals that appear in the asserts). 14 groups, 30 tests. Argv-only groups (same argv, any expectation): 48.

Read this carefully: most groups share generic pins such as `ROOT`, `[ebuild  N     ] ...` prefixes or the same `No news items` line. They share argv and some pinned lines, not the full expectation. Treat each group as a *review* list, not a deletion list. The AST pass does not compare fixtures, so a shared argv can mean different fixture setups.

| group | tests (file:line) |
|---|---|
| B1 | `test_emerge_pretend_contract:3568 test_root_deps_pinned_output`; `test_emerge_pretend_contract:3600 test_bdepend_routes_to_the_running_root_for_a_cross_root_build_without_root_deps` |
| B2 | `test_emerge_pretend_contract:6633 test_s3_bid_accept_keywords_bid2_accepts_only_build_2`; `test_emerge_pretend_contract:6717 test_s3_bid_range_atom_ignores_the_build_id` |
| B3 | `test_emerge_pretend_contract:7487 test_s5_formats_repo_deps_and_build_id_accepted`; `test_emerge_pretend_contract:7542 test_s5_nokey_layout_key_ignored_without_default_eapi_format` |
| B4 | `test_emerge_pretend_contract:7888 test_s1_user_package_use_mask_masks_the_flag_like_portage`; `test_emerge_pretend_contract:7929 test_s1_user_use_mask_masks_the_flag_like_portage`; `test_emerge_pretend_contract:8041 test_s1_user_use_stable_mask_masks_the_flag_like_portage` |
| B5 | `test_emerge_pretend_contract:7952 test_s1_user_profile_accept_keywords_unmasks_like_portage`; `test_emerge_pretend_contract:13163 test_package_accept_keywords_bare_atom_implicitly_grants_tilde_arch` |
| B6 | `test_emerge_pretend_contract:8334 test_s2_user_profile_package_keywords_adds_a_stable_keyword_like_portage`; `test_emerge_pretend_contract:8363 test_s2_chain_package_keywords_adds_a_stable_keyword_like_portage` |
| B7 | `test_emerge_pretend_contract:10612 test_autounmask_use_parent_flip_suggestion_is_suppressed_by_autounmask_use_n`; `test_emerge_pretend_contract:15520 test_use_dep_equal_parent_mismatches_when_parent_flag_is_disabled` |
| B8 | `test_emerge_pretend_contract:12071 test_getbinpkg_multi_instance_walks_the_selected_builds_own_deps`; `test_emerge_pretend_contract:12097 test_getbinpkg_multi_instance_local_build_does_not_shadow_a_newer_remote_build` |
| B9 | `test_emerge_pretend_contract:19634 test_unreachable_installed_consumer_does_not_block_an_upgrade`; `test_emerge_pretend_contract:19664 test_reachable_installed_consumer_holdable_pin_does_not_block_an_upgrade` |
| B10 | `test_emerge_pretend_contract:19907 test_unreachable_installed_pin_does_not_block_an_explicit_upgrade`; `test_emerge_pretend_contract:20024 test_keeper_reachable_pin_breaks_an_explicit_upgrade_and_reports_it` |
| B11 | `test_emerge_pretend_contract:19923 test_unreachable_installed_pin_does_not_block_a_hard_dependency_requirement`; `test_emerge_pretend_contract:20055 test_keeper_reachable_pin_breaks_a_hard_dependency_requirement_and_reports_it` |
| B12 | `test_emerge_pretend_contract:22284 test_check_news_reports_none_when_all_items_are_read`; `test_emerge_pretend_contract:22305 test_check_news_skip_file_excludes_items_like_the_read_file`; `test_emerge_pretend_contract:22357 test_check_news_matches_a_versioned_display_if_installed_atom` |
| B13 | `test_emerge_pretend_contract:24199 test_oracle_289_world_slot_atom_holds_the_old_instance`; `test_emerge_pretend_contract:24232 test_oracle_296_installed_holder_in_world_holds_the_old_instance` |
| B14 | `test_emerge_pretend_contract:26544 test_oracle_b0b_satisfied_replacement_p2c_inline_row`; `test_emerge_pretend_contract:26622 test_oracle_b0b_satisfied_replacement_p2e_runtime_inline_row` |

## C. Redundant by subset

Result: **319 CASES rows** (of 872) have a `returncode` assertion of the same value in a hand-written test with the same argv. The weak claim is "exit code == row value" (`test_pretend_case_exit_code`), and the stronger claim is "exit code == N" in the hand-written test. The implication is exact for the exit code only. `CASES` rows also feed output invariants, metamorphic and corpus, so this is *not* a licence to delete them (see E).

No hand-written test is fully implied by another hand-written test (the two partial matches at `test_emerge_pretend_contract.py:3339` and `:3399` also carry `substr` claims that are not implied). Two rc-only tests have a same-argv partner that pins output but does not check the exit code, so the rc claim is NOT redundant:

- `test_missing_repos_conf_pinned_output` (line 3554) vs `test_pv_decorates_the_cpv_with_slot_and_repo` (line 12639), argv `--pretend dev-libs/newpkg`.
- `test_prune_nodeps_pretend_nothing_outdated` (line 18071) vs `test_prune_nodeps_ignores_the_unresolvable_dep` (line 17757), argv `--pretend --prune --nodeps`.

Examples of the CASES implication (first 8 of the list, argv-equal only):

| CASES row (index) | hand-written test implying its rc | argv |
|---|---|---|
| CASES[0] 'new install' | `test_autounmask_continue_and_backtrack_are_inert_without_autounmask_changes` (line 10166) | `--pretend dev-libs/newpkg` |
| CASES[58] "an installed parent's conditional BDEPEND is evalu" | `test_root_deps_evaluates_conditional_use_deps_before_the_satisfied_check` (line 3727) | `--pretend -D --root-deps dev-libs/deeprootdepconsumer` |
| CASES[281] 'recursion: unbreakable build-time cycle is a fatal' | `test_unbreakable_build_time_cycle_prints_the_circular_deps_error` (line 3966) | `--pretend dev-libs/hardcyclea` |
| CASES[325] 'circular: upstream test_circular_choices pg4 reque' | `test_ccd4b_tree_follows_the_cycle_breaking_branch` (line 4027) | `--pretend --tree dev-libs/ccd4b` |
| CASES[289] 'circular dep: softened build-time ring reports the' | `test_softened_build_time_cycle_reports_the_persisting_ring` (line 4054) | `--pretend dev-libs/sbrA` |
| CASES[282] 'circular dep: mixed-priority unserializable cycle ' | `test_mixed_priority_cycle_prints_each_edge_portage_label` (line 4103) | `--pretend dev-libs/slopcyca` |
| CASES[283] 'circular dep: USE-flag suggestion (_find_suggestio' | `test_circular_dep_use_flag_suggestion` (line 4154) | `--pretend dev-libs/usecyclea` |
| CASES[286] 'circular dep: grandparent cycle in --tree still ex' | `test_tree_nomerge_ancestor_row_carries_the_package_use_column` (line 4254) | `--pretend --tree dev-libs/gpcyclec` |

Environment is NOT part of this key. A CASES row runs with `fixture_env`; the hand-written partner may use its own env. Confirm the env matches before treating a row as implied.

## D. Weak / possibly vacuous tests

Ranked by how many tests share the pattern. "Weak" here means every assert is one of: `returncode` (`== 0`, `!= 0` or a pinned value), `"lit" in stdout/stderr`, `== ""`/`== []`, `is not None`, a bare truthy expression, or a corpus-only pin (`_assert_harvested`, whose drift is only a warning). None of these is wrong by itself; the question is whether the test also pins the output.

| rank | pattern | count | files | one-line weakness |
|---|---|---|---|---|
| 1 | `empty + rc0` | 23 | test_emerge_pretend_contract.py 23 | negative "nothing happens" check; a silent no-op also passes |
| 2 | `rcN + substr` | 18 | test_portuale.py 11, test_emerge_pretend_contract.py 7 | exit code plus two or three substrings; full output not pinned |
| 3 | `rc0 + substr` | 16 | test_portuale.py 12, test_emerge_pretend_contract.py 4 | exit code plus substrings; full output not pinned |
| 4 | `rc0 + truthy` | 15 | test_portuale.py 8, test_emerge_pretend_contract.py 7 | exit code plus a bare truthy check (often a file existence) |
| 5 | `truthy` | 14 | test_output_invariants.py 9, test_registry.py 2, test_portuale.py 1, test_fixture_caches.py 1, test_harvested_corpus.py 1 | only a truthy check; relies on the helper or fixture |
| 6 | `corpus` | 13 | test_emerge_pretend_contract.py 13 | `_assert_harvested` only: pins Rust-vs-harvest agreement, drift is only a warning (no Portage pin) |
| 7 | `substr` | 7 | test_portuale.py 5, test_emerge_pretend_contract.py 2 | substrings only; no exit code and no full output pin |
| 8 | `rcN + substr + truthy` | 5 | test_portuale.py 4, test_emerge_pretend_contract.py 1 | mixed weak asserts |
| 9 | `rcN` | 4 | test_emerge_pretend_contract.py 4 | exit code only (pinned non-zero) |
| 10 | `rc0` | 3 | test_portuale.py 1, test_musl_smoke.py 1, test_benchmark_gate.py 1 | exit code only |
| 11 | `empty + rcN` | 2 | test_emerge_pretend_contract.py 2 | mixed weak asserts |
| 12 | `rc0 + rcN + substr` | 2 | test_emerge_pretend_contract.py 1, test_portuale.py 1 | mixed weak asserts |
| 13 | `rc0 + substr + truthy` | 2 | test_portuale.py 2 | mixed weak asserts |
| 14 | `empty + rc0 + truthy` | 2 | test_portuale.py 2 | mixed weak asserts |
| 15 | `corpus + substr` | 1 | test_emerge_pretend_contract.py 1 | mixed weak asserts |
| 16 | `corpus + rc0 + substr` | 1 | test_emerge_pretend_contract.py 1 | mixed weak asserts |
| 17 | `empty + rcN + substr` | 1 | test_emerge_pretend_contract.py 1 | mixed weak asserts |
| 18 | `empty + rc0 + rcN + substr + truthy` | 1 | test_emerge_pretend_contract.py 1 | mixed weak asserts |
| 19 | `empty + rc0 + rcN + truthy` | 1 | test_emerge_pretend_contract.py 1 | mixed weak asserts |

Total weak-only tests: 131. No test in the suite has zero assertions (0 found; every test calls an asserting helper or contains an `assert`).

### D1. Two-run comparisons without a returncode check

AST pass: 52 tests compare two run outputs with `==`; 30 of them never assert `returncode`. Of those, the run variable that is not pinned by any literal assert is listed below. Most of the other flagged tests are `rust == python` parity tests whose helper uses `subprocess.run(..., check=True)` (`test_atom_contract.py:97`, `test_versions_contract.py:70`, `test_use_reduce_contract.py:54`, `test_required_use_contract.py:109`), so a non-zero exit already raises. Those are NOT vacuous, and they are excluded from the list below.

| test | unpinned run | comparison |
|---|---|---|
| `test_emerge_pretend_contract.py:11038 test_useoldpkg_atoms_prefers_the_old_binary_over_a_newer_ebuild` | inert | `inert.stdout == default.stdout` |
| `test_emerge_pretend_contract.py:20397 test_complete_graph_does_not_merge_a_missing_deep_dep_of_an_installed_pkg` | off | `cg.stdout == plain.stdout` |
| `test_emerge_pretend_contract.py:20427 test_complete_graph_if_new_ver_auto_enables_on_an_upgrade` | off | `off.stdout == auto.stdout` |

Reading the table: `test_emerge_pretend_contract.py:11038` is not vacuous, because `inert` is compared with `default`, whose stdout is pinned exactly (`default.stdout.splitlines() == [...]`, line 11050). `:20397` is the real gap: `cg` is pinned only by a negative substring check and by equality with `plain`, `off` is never pinned, and no run asserts rc. `:20427` is pinned through `auto`, but `off` has no rc check. `test_cached_harness.py:86` is not vacuous: it asserts `first[0] == 0` through tuple indexing, which the AST pass does not recognise as a returncode check. `test_cached_harness.py:153` is an intentional replay-equality test of a non-zero exit.

Manual additions (closure-based comparisons the AST pass cannot see):

- `test_output_invariants.py:114` `test_repeated_runs_are_byte_identical`: runs each case three times and compares `(returncode, stdout, stderr)` with each other. No returncode check, so three identical *failures* pass. Weak by design (determinism), add `rc == 0` or pin the rc set.
- `test_output_invariants.py:131` `test_output_is_identical_under_shuffled_directory_order`: same shape, seeds 1/17/4242 compared against each other; no returncode check.
- `test_output_invariants.py:85` `test_contract_cases_satisfy_output_invariants` and `:99` `test_fixture_packages_satisfy_output_invariants`: the invariant checker returns `None` for "not a resolver command" and that is silently skipped; only `_report` asserts that at least one case produced JSON (`:80-81`).
- `test_metamorphic.py:368` `test_transform_does_not_change_output`: NOT weak. It pins the pristine rc to `case[2]` (`:376`) and compares normalised rc+stdout+stderr. This is the post-#449 shape.

### D2. The weakest single-assert tests (listed in full)

- `test_emerge_pretend_contract.py:3554 test_missing_repos_conf_pinned_output` (rcN)
- `test_emerge_pretend_contract.py:4465 test_abort_debug_digraph_dump_order_matches` (rcN)
- `test_emerge_pretend_contract.py:13360 test_unrelated_masked_by_keywords_package_is_still_hidden` (rcN)
- `test_emerge_pretend_contract.py:17405 test_depclean_package_provided_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:17516 test_depclean_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:17652 test_depclean_lib_check_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:17770 test_depclean_unresolved_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:17893 test_depclean_removal_order_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:18092 test_prune_nodeps_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:18132 test_prune_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:18152 test_prune_pinned_output_on_the_shared_vdb` (corpus)
- `test_emerge_pretend_contract.py:18240 test_unmerge_pretend_vdb_path_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:18655 test_deselect_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:21308 test_search_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:21579 test_clean_and_rage_clean_pretend_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:21733 test_profiles_updates_package_moves_pinned_output` (corpus)
- `test_emerge_pretend_contract.py:23057 test_backtracking_good_version_first_aborts_below_the_oracle_minimum` (rcN)
- `test_portuale.py:2107 test_ebuild_debug_flag_enables_real_set_x_tracing` (truthy)
- `test_portuale.py:10514 test_s4_ebuild_fetch_uses_make_conf_distdir` (rc0)
- `test_output_invariants.py:114 test_repeated_runs_are_byte_identical` (truthy)
- `test_output_invariants.py:131 test_output_is_identical_under_shuffled_directory_order` (truthy)
- `test_output_invariants.py:211 test_checker_flags_a_wiped_required_by` (truthy)
- `test_output_invariants.py:220 test_checker_flags_an_owner_that_is_neither_entry_nor_installed` (truthy)
- `test_output_invariants.py:226 test_checker_flags_a_duplicate_slot_without_a_conflict` (truthy)
- `test_output_invariants.py:236 test_checker_flags_a_dependency_merged_after_its_owner` (truthy)
- `test_output_invariants.py:285 test_checker_tolerates_a_cycle` (truthy)
- `test_output_invariants.py:305 test_checker_flags_a_flush_left_never_reached_tree_row` (truthy)
- `test_output_invariants.py:318 test_checker_flags_a_merge_row_printed_twice_in_tree_mode` (truthy)
- `test_registry.py:141 test_yaml_target_path_unchanged_by_default` (truthy)
- `test_registry.py:184 test_absent_applet_still_falls_back_to_the_build` (truthy)
- `test_fixture_caches.py:74 test_committed_fixture_md5_cache_entries_match_their_ebuilds` (truthy)
- `test_musl_smoke.py:46 test_musl_static_binaries_run_in_scratch_container` (rc0)
- `test_benchmark_gate.py:23 test_rust_versions_harness_is_faster_than_python` (rc0)
- `test_harvested_corpus.py:21 test_expanded_corpus_replays` (truthy)

## E. CASES table (`test_emerge_pretend_contract.py:53-3104`)

872 rows, all well-formed 3-tuples. Consumers: `test_pretend_case_exit_code` (parametrize), `test_output_invariants.py:29,91,108` (all rows), `test_metamorphic.py:42,108,124` (SUBSET picks by atom with a cap of 3 per atom, plus `SUBSET_DESCRIPTIONS` which must all exist or the module raises at import, `:146`), and the corpus (the parametrize id embeds the row description, the index and the rc).

- Identical descriptions: **0**. Exact duplicate rows (desc+args+rc): **0**.
- Identical args (any description): **14 groups, 32 rows**. Removable in principle: 18 rows. I checked the harvested corpus: for all 14 groups the stored stdout, stderr and rc are byte-identical across the group, so the rows run the same thing twice under different descriptions. The descriptions are the only difference.

| group | rows (index, file line, rc) | description examples |
|---|---|---|
| 13, 578 | 13 (L122, rc 0), 578 (L2134, rc 0) | "already installed" / "package.mask: -atom removal leaves candidate unaffected" |
| 14, 567, 696 | 14 (L123, rc 0), 567 (L2091, rc 0), 696 (L2336, rc 0) | "a New package shows its full USE="..." list at plain -p" / "profile config: real USE flag gates a dependency" |
| 17, 279 | 17 (L138, rc 0), 279 (L678, rc 0) | "the merge list is in dependency-first order (diamond: l" / "recursion: diamond dependency dedup" |
| 19, 562 | 19 (L148, rc 0), 562 (L2066, rc 0) | "merge order threads through --tree too (roots re-derive" / "--tree: indents a diamond dependency, shown once" |
| 20, 195 | 20 (L153, rc 0), 195 (L515, rc 0) | "--json entries carry an explicit merge_order index" / "--json: diamond dependency, required_by lists both owne" |
| 116, 695, 697 | 116 (L436, rc 0), 695 (L2335, rc 0), 697 (L2337, rc 0) | "-pv: cpv decorated with ::repo" / "-v short alias is now implemented, not rejected" |
| 119, 808, 819 | 119 (L439, rc 0), 808 (L2830, rc 0), 819 (L2885, rc 0) | "-pv: new-slot other-version list, decorated" / "new-slot install: -v keeps the S column ahead of the -v" |
| 249, 250 | 249 (L603, rc 1), 250 (L608, rc 1) | "#341 S5: profile EAPI/formats validation earns Portage'" / "#341 S6: user-config atom validation earns Portage's rc" |
| 262, 263 | 262 (L637, rc 1), 263 (L644, rc 1) | "#341 S9: build-id match cell earns Portage's rc 1" / "#346 S3: build-id user-config cell earns Portage's rc 1" |
| 402, 556 | 402 (L1281, rc 0), 556 (L2036, rc 0) | "recursion: dedup across DEPEND and RDEPEND" / "--buildpkgonly: off by default, a New->New dependency c" |
| 455, 689 | 455 (L1515, rc 1), 689 (L2329, rc 1) | "--autounmask: no keyword suggestion by default" / "--misspell-suggestions: a masked (existing) cp gets no " |
| 465, 576 | 465 (L1565, rc 1), 576 (L2132, rc 1) | "--autounmask-keep-masks: a package.mask'd top-level tar" / "package.mask: hidden, no unmask" |
| 568, 571, 818 | 568 (L2092, rc 0), 571 (L2107, rc 0), 818 (L2880, rc 0) | "USE_EXPAND: VIDEO_CARDS=nvidia expands to video_cards_n" / "-pv groups IUSE by USE_EXPAND variable (VIDEO_CARDS=".." |
| 629, 820 | 629 (L2233, rc 0), 820 (L2890, rc 0) | "blocker: -v widens the [blocks b ] bracket by the mask " / "Total: counters line (-v) with a blocker Conflict: line" |

- Flag-order or `-p` vs `--pretend` spelling groups: **13**. Most are *operand* reorderings (`--pretend A B` vs `--pretend B A`), which test merge-order determinism and must be kept. The genuine alias pairs (`--update`/`-u`, `--deep`/`-D`, `--emptytree`/`-e`, `--verbose`/`-v`, `--pretend`/`-p`) test option spelling; keep them, since each alias is its own parser path.

| group | rows (index, line, rc) | args |
|---|---|---|
| 7, 8 | 7 (L85, rc 1), 8 (L90, rc 1) | `--pretend dev-libs/needer dev-libs/othermod || --pretend dev-libs/othermod dev-libs/needer` |
| 9, 10 | 9 (L95, rc 0), 10 (L100, rc 0) | `--pretend dev-libs/plainuser dev-libs/needer || --pretend dev-libs/needer dev-libs/plainuser` |
| 48, 49 | 48 (L209, rc 0), 49 (L210, rc 0) | `--pretend --update dev-libs/upgradepkg || --pretend -u dev-libs/upgradepkg` |
| 51, 52 | 51 (L212, rc 0), 52 (L213, rc 0) | `--pretend --deep dev-libs/deeppkg || --pretend -D dev-libs/deeppkg` |
| 77, 78 | 77 (L258, rc 0), 78 (L259, rc 0) | `--pretend --emptytree dev-libs/deeppkg || --pretend -e dev-libs/deeppkg` |
| 116, 694, 695, 697 | 116 (L436, rc 0), 694 (L2334, rc 0), 695 (L2335, rc 0), 697 (L2337, rc 0) | `--pretend -v dev-libs/newpkg || --pretend --verbose dev-libs/newpkg || --pretend -v dev-libs/newpkg` |
| 301, 302, 303, 304, 305, 306 | 301 (L780, rc 1), 302 (L785, rc 1), 303 (L790, rc 1), 304 (L795, rc 1), 305 (L800, rc 1), 306 (L805, rc 1) | `--pretend --autounmask-backtrack=y dev-libs/aub0c dev-libs/aub0b dev-libs/aub0a || --pretend --autounmask-backtrack=y dev-libs/aub0c dev-libs/aub0a dev-libs/aub0b || --pretend --autounmask-backtrack=y dev-libs/aub0b dev-libs/aub0c dev-libs/aub0a` |
| 308, 309 | 308 (L815, rc 0), 309 (L820, rc 0) | `--pretend --update --deep --selective dev-libs/bkt1a dev-libs/bkt1b || --pretend --update --deep --selective dev-libs/bkt1b dev-libs/bkt1a` |
| 310, 311 | 310 (L825, rc 0), 311 (L830, rc 0) | `--pretend --backtrack=1 dev-libs/bkt3c dev-libs/bkt3d || --pretend --backtrack=1 dev-libs/bkt3d dev-libs/bkt3c` |
| 312, 313 | 312 (L835, rc 0), 313 (L840, rc 0) | `--pretend dev-libs/bkt4b dev-libs/bkt4a || --pretend dev-libs/bkt4a dev-libs/bkt4b` |
| 314, 315 | 314 (L845, rc 0), 315 (L850, rc 0) | `--pretend =dev-libs/bkt5a-1 dev-libs/bkt5b || --pretend dev-libs/bkt5b =dev-libs/bkt5a-1` |
| 616, 617, 618, 619, 620, 621 | 616 (L2172, rc 0), 617 (L2177, rc 0), 618 (L2182, rc 0), 619 (L2187, rc 0), 620 (L2192, rc 0), 621 (L2197, rc 0) | `--pretend --backtrack=0 dev-libs/blk0a dev-libs/blk0b dev-libs/blk0c || --pretend --backtrack=0 dev-libs/blk0a dev-libs/blk0c dev-libs/blk0b || --pretend --backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c` |
| 685, 686 | 685 (L2325, rc 1), 686 (L2326, rc 1) | `--pretend dev-libs/does-not-exist dev-libs/newpkg || --pretend dev-libs/newpkg dev-libs/does-not-exist` |

Removability: a row can go only if (a) no other consumer needs it: invariants and metamorphic use every row or the SUBSET picks, and (b) its corpus entry is handled. The 319 rows in section C are also pinned by a hand-written rc assertion, so they are the safest from the test-count view, but their removal is still blocked by the invariant and corpus consumers. The 14 same-args groups are the only candidates with identical recorded outputs.

## F. Corpus keys and the bless/harvest procedure

Files: `corpus/contract.json.xz` (1316 entries, `version 1`, blobs de-duplicated by sha), `corpus/expanded.json.xz` (4900 entries).

**Keys.** Contract: `<pytest nodeid>#<ordinal>`, e.g. `pytests-contract-suite/test_emerge_pretend_contract.py::test_pretend_case_exit_code[<description>-args<index>-<rc>]#0`. The suite-directory prefix is ignored when matching (`corpus.py:157-162`); 276 entries carry the legacy `tests/` prefix. The ordinal counts calls with the same args+env in the same test (`corpus.py:223-265`). The parametrize id embeds the CASES **index** (pytest appends `args<i>` for a list parameter) and the expected **rc**, so editing a description, an expected rc, or inserting or deleting a row changes the key.

Expanded: keyed by the args string, e.g. `--pretend --emptytree @world` (no test name). Replayed by `test_harvested_corpus.py`. Renames do not affect it.

**Current state (checked against the CASES table).** 860 `test_pretend_case_exit_code` keys. 4 keys are already orphaned because their row was rewritten after harvest (row 72: stored desc "without a value is a parse error", rc 2, current row is "followed by a non-number stands alone", rc 0; row 192: rc 2 stored, rc 1 now; row 265: rc 2 stored, rc 1 now; row 705: desc "-pf" stored, "-pA" now). 16 CASES rows have no entry at all (rows added after the last harvest, indices 72/192/265/705 aside, and 860-871). Nothing else is orphaned: every stored function name still exists.

**What the code does with a call (`corpus.py`):**

- `check_contract_call` (`:233-284`): no candidate with the same args+env means `(None, None)`. That is **silent**: no drift, no warning, no failure, unless the test calls `_assert_harvested`.
- `compare` / `flag` (`:194-221`): a matched key whose rc or stdout/stderr changed produces a `CorpusDrift` warning (strict only with `PORTUALE_CORPUS_STRICT=1`).
- `_assert_harvested` (`test_emerge_pretend_contract.py:3149-3157`): asserts that a key was matched and, unless BLESS is set, that there is no drift. Drift is a warning, so this pins only "Rust still equals the harvest snapshot".
- `write_blessed` (`:322-332`, run from `conftest.py:352` at session end): replaces only keys that were matched and drifted (`blessed`). It does not add new calls and does not drop orphans.
- `write_harvest` (`:293-319`, `PORTUALE_CORPUS_HARVEST=1`): (1) drops every stored key no call matched in this run; (2) records an unmatched call **only if the base name of its test function (`file::func`, without the parameter id) equals the base name of a function that had an orphan**. Everything it records is a Rust snapshot, not a Python-agreement.

**Consequences for this cleanup:**

- **Pure function rename**: step (1) drops the old keys; step (2) does not record the new name, because the new base name is not in the stale set. The pin is lost silently. This is a code gap, not a procedure: fix `write_harvest` (for example, record unmatched calls of any function that had orphans *or* is absent from the stored index) before renaming anything.
- **Deleting a test**: its keys stay until a HARVEST run drops them. Plain runs ignore them silently. No re-record is needed.
- **Inserting or deleting a CASES row**: every later index changes. Deleting row 13 changes the id of 858 rows; deleting row 249 changes 622. Those keys go orphan and only a HARVEST run repairs them, which turns their stored Python-agreement provenance into a plain Rust snapshot for every shifted row. Fix the ids first (for example, `ids=` built from the description only, which works because descriptions are unique today: 0 duplicates), then do the CASES edit in one batch with one re-record.
- **Metamorphic module**: deleting one of the three `SUBSET_DESCRIPTIONS` rows (`test_metamorphic.py:98-102`) makes `_subset_extra` raise (`:127`) at import, which breaks the whole module.

**Bless procedure.**

1. Value change on an unchanged key (drift only): `PORTUALE_CORPUS_BLESS=1 python3 -m pytest pytests-contract-suite -q` over the whole suite. Review the printed drift list first. Diff the decoded JSON (`lzma` plus `json`), not the `.xz` bytes. Commit in pmtest only.
2. Key change (renamed row, changed desc or rc, inserted or deleted rows, deleted test): `PORTUALE_CORPUS_HARVEST=1 python3 -m pytest pytests-contract-suite -q` over the whole suite. Check the printed `dropped N orphan(s), recorded M call(s)`. Do not run a subset: `matched_keys` only covers the tests that ran, so a subset drops everything else. Do not combine it with `BLESS=1`: `write_harvest` reloads the cached, pre-bless corpus and overwrites the blessed entries.
3. Renamed functions: step 2 cannot re-record them until the stale-set gap above is fixed. Until then, re-add the pins by hand after a HARVEST run.

## Recommended cleanup order

Counts are tests or rows affected. Steps 1 to 3 are zero-behaviour-change and can run without a corpus rewrite. Steps 4 onward need a corpus re-record and must be batched.

| step | action | affected | risk |
|---|---|---|---|
| 0 | Fix the corpus gaps first: the 4 orphaned keys and the 16 CASES rows with no entry. Decide per row whether to re-harvest or delete. | 20 rows | low, but needs a HARVEST run |
| 1 | Delete the dead duplicate `test_direct_ebuild_merge_prints_the_portage_listing` at `test_portuale.py:1668` (keep `:6867`) and the duplicate helper `_remote_rmpkg_args` at `:1536` (keep `:1593`). | 1 test + 1 helper | none; no test outcome changes |
| 2 | Port the 4 extra asserts from dead `test_direct_ebuild_qmerge_replacement_prints_safely_unmerging` (`test_portuale.py:1721`) into the live one (`:6920`), run it, then delete `:1721`. | 1 test | may fail: that is a real finding |
| 3 | Delete one of `test_autounmask_use_parent_flip_suggestion_is_suppressed_by_autounmask_use_n` (`test_emerge_pretend_contract.py:10612`) or `test_use_dep_equal_parent_mismatches_when_parent_flag_is_disabled` (`:15520`). Keep `:15520`. | 1 test | low; its corpus keys become orphans (plain delete) |
| 4 | Fix `write_harvest` for renames and new tests, and make the CASES ids index-free. Do this before touching CASES. | 0 tests; 872 ids | medium: forces one full re-record |
| 5 | Collapse the 14 identical-args CASES groups: keep one row per group and move the other descriptions into a comment. Re-record once, in the same batch as step 4. | 20 rows (34 rows in 14 groups) | medium: CASES consumers |
| 6 | Strengthen the weakest non-deletions: add `rc == 0` to `test_repeated_runs_are_byte_identical` and to the shuffled-order test (`test_output_invariants.py:114,131`); add an rc check and a pin for the unpinned `off` runs at `test_emerge_pretend_contract.py:20397` and `:20427`; replace the 13 corpus-only pins with Portage-sourced expectations where a capture exists. | 4 tests strengthened; 13 reviewed | low |
| 7 | Review (do not delete) the 131 weak-only tests and the 23 `empty + rc0` negative tests. Strengthen the 14 `truthy`-only checker tests where a full expected output is available. | 131 reviewed | low |
| 8 | Section C (the 319 CASES rows) and section B (the 14 groups): review only. Their argv is shared, but env and fixtures are not, so each needs a human check. | 319 + 30 tests | none unless reviewed |

Not in scope but noticed: `test_atom_contract.py`, `test_versions_contract.py`, `test_use_reduce_contract.py` and `test_required_use_contract.py` still run a `rust == python` parity against `python-harness/cached_harness.py`. Their rc is enforced by `check=True`, so they are not vacuous, but the Python side is the thing the owner has been removing elsewhere. Worth a separate decision.
