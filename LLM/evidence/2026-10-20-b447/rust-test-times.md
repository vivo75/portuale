# Backlog #447: per-test durations for `cargo test --release` (workspace)

Measurement only. No product code or tests were changed. Commit: `bdbbcb22` (branch `tests-speed`).

## Method

libtest `--report-time` works on this toolchain (`RUSTC_BOOTSTRAP=1` on stable), so no fallback was needed. Exact command, run from `/home/vivo/repo/PORTUALE/portuale`:

```
RUSTC_BOOTSTRAP=1 cargo test --release --workspace -- -Z unstable-options --report-time
```

Raw output: `/tmp/claude-1000/-home-vivo-repo-PORTUALE-portuale/31e72e06-5910-492b-8d0e-7bb237e53e32/scratchpad/rust-report-time.txt`. Parsed with a short script over the `test <name> ... ok <N.NNNs>` lines, keyed by the `Running ... (target/release/deps/<name>-<hash>)` header. Default test threads (parallel). Exit status 0, no failures.

Caveats: per-test times are wall times while the test runs in parallel, so the sum of per-test times (518 s) is larger than the wall time. The shares below are of that summed time, not of wall time. Some `ok` lines have no `<time>` because the test's own stdout was printed mid-line; 28 passing portuale tests are in that set and are excluded from the sums, as are 4 ignored tests.

## Wall time and inodes

- Total wall time of the cargo command: **270 s** (start 1792522540, end 1792522810 epoch). This includes **74 s** of build (`Finished release ... in 1m 14s`, because `RUSTC_BOOTSTRAP` forced a rebuild). Test-execution wall time is about 196 s.
- `portuale` binary alone: `finished in 192.26s`.
- Inodes before the run: `/var/tmp` 995 inodes used of 10737418240 (1%); `/tmp` 23415 used of 1048576 (3%).
- HEAD: `bdbbcb22`. Tree clean at start (`git status` empty; pmtest `fixtures/` clean).

## Per test binary

| binary (target/release/deps) | tests | timed | sum of test times (s) | results |
|---|---:|---:|---:|---|
| atom_harness | 0 | 0 | 0.00 | {} |
| harness_common | 0 | 0 | 0.00 | {} |
| mrg_director | 17 | 17 | 0.26 | {'ok': 17} |
| portage_dep | 66 | 66 | 0.01 | {'ok': 66} |
| portage_fetch | 48 | 48 | 0.01 | {'ok': 48} |
| portage_profile | 123 | 123 | 0.06 | {'ok': 123} |
| portage_repo | 1156 | 1156 | 10.75 | {'ok': 1156} |
| portage_required_use | 20 | 20 | 0.00 | {'ok': 20} |
| portage_use_reduce | 26 | 26 | 0.00 | {'ok': 26} |
| portage_util | 8 | 8 | 0.01 | {'ok': 8} |
| shuffle_seed_freeze | 1 | 1 | 0.00 | {'ok': 1} |
| portage_vdb | 96 | 96 | 3.10 | {'ok': 96} |
| conformance | 139 | 139 | 4.32 | {'ok': 139} |
| corpus_round_trip | 6 | 6 | 0.15 | {'ok': 6} |
| no_hand_built_vdb_paths | 1 | 1 | 0.03 | {'ok': 1} |
| portage_versions | 5 | 5 | 0.00 | {'ok': 5} |
| portuale | 1130 | 1099 | 499.34 | {'ok': 1127, 'ignored': 3} |
| required_use_harness | 0 | 0 | 0.00 | {} |
| use_reduce_harness | 0 | 0 | 0.00 | {} |
| versions_harness | 2 | 1 | 0.00 | {'ok': 1, 'ignored': 1} |

TOTAL_SUM 518.04 TIMED 2812 UNTIMED 32 (summed over all timed tests, parallel overlap included).

## 40 slowest tests

| # | seconds | test | source file:line |
|---:|---:|---|---|
| 1 | 183.011 | `portuale::fetch::tests::fetch_src_uri_symlinks_a_verified_portage_ro_distdirs_file` | crates/portuale/src/fetch.rs:1160 |
| 2 | 16.148 | `portuale::ebuild_merge::tests::db_merge::a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files` | crates/portuale/src/ebuild_merge/tests/db_merge.rs:987 |
| 3 | 16.145 | `portuale::ebuild_merge::tests::options::from_env_folds_the_calling_env_over_the_default_on_unmerge_orphans` | crates/portuale/src/ebuild_merge/tests/options.rs:105 |
| 4 | 16.145 | `portuale::ebuild_merge::tests::options::from_env_reads_uninstall_ignore_from_the_calling_env` | crates/portuale/src/ebuild_merge/tests/options.rs:163 |
| 5 | 10.266 | `portuale::ebuild_phases::tests::phase_runs::install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer` | crates/portuale/src/ebuild_phases/tests/phase_runs.rs:598 |
| 6 | 10.167 | `portuale::ebuild_phases::tests::commands::features_xattr_routes_phase_installs_through_the_xattr_wrapper` | crates/portuale/src/ebuild_phases/tests/commands.rs:471 |
| 7 | 10.146 | `portuale::ebuild_phases::tests::commands::phases_see_the_per_package_fake_distdir` | crates/portuale/src/ebuild_phases/tests/commands.rs:943 |
| 8 | 8.999 | `portuale::ebuild_phases::tests::commands::debug_flag_exports_portage_debug` | crates/portuale/src/ebuild_phases/tests/commands.rs:861 |
| 9 | 8.993 | `portuale::ebuild_phases::tests::commands::phases_export_portage_actual_distdir_so_doins_keeps_absolute_symlinks` | crates/portuale/src/ebuild_phases/tests/commands.rs:902 |
| 10 | 8.212 | `portuale::ebuild_phases::tests::commands::brush_phase_env_is_filtered_of_bash_special_variables` | crates/portuale/src/ebuild_phases/tests/commands.rs:647 |
| 11 | 8.054 | `portuale::ebuild_phases::tests::commands::install_inherits_a_real_eclass_that_only_exists_in_the_overlays_own_master_repo` | crates/portuale/src/ebuild_phases/tests/commands.rs:787 |
| 12 | 7.970 | `portuale::ebuild_phases::tests::commands::install_writes_the_real_build_time_file` | crates/portuale/src/ebuild_phases/tests/commands.rs:138 |
| 13 | 7.938 | `portuale::ebuild_merge::tests::db_merge::a_merge_upgrade_unmerge_sequence_is_equivalent_on_redb_and_files` | crates/portuale/src/ebuild_merge/tests/db_merge.rs:994 |
| 14 | 7.042 | `portuale::ebuild_phases::tests::commands::install_runs_the_portage_post_install_qa_check_and_strips_a_genuinely_empty_dir` | crates/portuale/src/ebuild_phases/tests/commands.rs:322 |
| 15 | 6.707 | `portuale::ebuild_phases::tests::commands::install_really_inherits_a_real_eclass_and_calls_its_own_function` | crates/portuale/src/ebuild_phases/tests/commands.rs:541 |
| 16 | 6.700 | `portuale::ebuild_phases::tests::commands::install_lands_a_real_file_under_a_real_d` | crates/portuale/src/ebuild_phases/tests/commands.rs:8 |
| 17 | 6.649 | `portuale::emerge_build::tests::source_merge::run_source_merge_upgrade_replaces_the_installed_version` | crates/portuale/src/emerge_build/tests/source_merge.rs:983 |
| 18 | 6.252 | `portuale::emerge_build::tests::source_merge::source_merge_with_resolved_config_threads_use_and_slot_into_the_phase` | crates/portuale/src/emerge_build/tests/source_merge.rs:8 |
| 19 | 5.980 | `portuale::ebuild_phases::tests::commands::expanded_name_export_assigns_in_both_backends` | crates/portuale/src/ebuild_phases/tests/commands.rs:702 |
| 20 | 4.517 | `portuale::ebuild_merge::tests::db_merge::a_standalone_unmerge_prunes_preserved_libs_in_its_retire_commit` | crates/portuale/src/ebuild_merge/tests/db_merge.rs:642 |
| 21 | 3.310 | `portuale::ebuild_merge::tests::plib_registry::plib_registry_two_consecutive_soname_bumps_replace_the_record_under_the_merging_cpv` | crates/portuale/src/ebuild_merge/tests/plib_registry.rs:326 |
| 22 | 3.029 | `portuale::emerge_build::tests::source_merge::run_build_scheduler_builds_two_leaves_in_parallel_then_the_parent` | crates/portuale/src/emerge_build/tests/source_merge.rs:496 |
| 23 | 3.011 | `portuale::pretend::tests::unmerge::unmerge_backup_gate_stays_on_for_an_env_only_token` | crates/portuale/src/pretend/tests/unmerge.rs:246 |
| 24 | 3.010 | `portuale::pretend::tests::unmerge::unmerge_backup_gate_honours_an_env_negation_over_make_conf` | crates/portuale/src/pretend/tests/unmerge.rs:223 |
| 25 | 3.010 | `portuale::pretend::tests::unmerge::unmerge_backup_gate_reads_a_make_conf_only_token` | crates/portuale/src/pretend/tests/unmerge.rs:188 |
| 26 | 2.964 | `portuale::remote::tests::config_place::remote_client_without_repos_conf_resolves_on_the_production_path` | crates/portuale/src/remote/tests/config_place.rs:443 |
| 27 | 2.962 | `portuale::remote::tests::config_place::remote_client_make_profile_resolves_against_same_named_server_repo` | crates/portuale/src/remote/tests/config_place.rs:119 |
| 28 | 2.960 | `portuale::remote::tests::config_place::remote_client_make_profile_prefers_main_repo_and_relative_targets` | crates/portuale/src/remote/tests/config_place.rs:329 |
| 29 | 2.958 | `portuale::remote::tests::config_place::remote_client_make_profile_unresolvable_fails_loudly` | crates/portuale/src/remote/tests/config_place.rs:218 |
| 30 | 2.955 | `portuale::remote::tests::config_place::remote_client_make_profile_resolves_without_client_repos_conf` | crates/portuale/src/remote/tests/config_place.rs:177 |
| 31 | 2.953 | `portuale::remote::tests::config_place::remote_client_make_profile_dir_with_cross_repo_parent_resolves` | crates/portuale/src/remote/tests/config_place.rs:265 |
| 32 | 2.951 | `portuale::remote::tests::config_place::placed_config_root_covers_both_placements` | crates/portuale/src/remote/tests/config_place.rs:7 |
| 33 | 2.951 | `portuale::remote::tests::config_place::remote_binpkg_path_resolves_the_placed_client_config` | crates/portuale/src/remote/tests/config_place.rs:383 |
| 34 | 2.937 | `portuale::ebuild_merge::tests::db_merge::a_same_version_reinstall_removes_dropped_files_on_every_backend` | crates/portuale/src/ebuild_merge/tests/db_merge.rs:1429 |
| 35 | 2.825 | `portuale::emerge_build::tests::source_merge::reinstall_leaves_a_byte_identical_file_a_preinst_helper_would_remove` | crates/portuale/src/emerge_build/tests/source_merge.rs:300 |
| 36 | 2.781 | `portuale::ebuild_merge::tests::db_merge::collision_protect_names_the_owner_from_the_index_on_every_backend` | crates/portuale/src/ebuild_merge/tests/db_merge.rs:1079 |
| 37 | 2.688 | `portuale::remote::tests::stages::remote_client_config_stacks_server_make_globals` | crates/portuale/src/remote/tests/stages.rs:521 |
| 38 | 2.611 | `portuale::emerge_getbinpkg::tests::run_merge_plan_merges_a_binary_and_a_source_entry_in_one_run` | crates/portuale/src/emerge_getbinpkg.rs:3858 |
| 39 | 2.524 | `portuale::emerge_build::tests::source_merge::run_build_scheduler_with_a_high_load_average_never_throttles` | crates/portuale/src/emerge_build/tests/source_merge.rs:560 |
| 40 | 2.493 | `portuale::ebuild_merge::tests::run_merge::a_contents_path_with_spaces_is_removed_and_the_prefix_decoy_is_kept` | crates/portuale/src/ebuild_merge/tests/run_merge.rs:381 |

## Cumulative share of summed test time

- top 10 55.6%
- top 20 68.7%
- top 40 79.9%

## Top 15: why they are slow (only where evident from the body)

| # | test | guess |
|---:|---|---|
| 1 | `fetch_src_uri_symlinks_a_verified_portage_ro_distdirs_file` | Body fetches https://192.0.2.1 (unroutable TEST-NET-1) in the 'no other candidate' branch; likely a network connect/timeout wait (not traced). |
| 2 | `a_merge_upgrade_unmerge_sequence_is_equivalent_on_sqlite_and_files` | Runs a full merge/upgrade/unmerge sequence on the sqlite backend (body calls sequence_equivalence). |
| 3 | `from_env_folds_the_calling_env_over_the_default_on_unmerge_orphans` | unknown |
| 4 | `from_env_reads_uninstall_ignore_from_the_calling_env` | unknown |
| 5 | `install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer` | Runs a real install phase over a large eclass in a thread with a 120 s deadline. |
| 6 | `features_xattr_routes_phase_installs_through_the_xattr_wrapper` | Runs setfattr and a real install phase through the xattr wrapper. |
| 7 | `phases_see_the_per_package_fake_distdir` | Runs a real install phase (run_commands). |
| 8 | `debug_flag_exports_portage_debug` | Runs a real install phase twice (debug true and false). |
| 9 | `phases_export_portage_actual_distdir_so_doins_keeps_absolute_symlinks` | Runs a real install phase (run_commands). |
| 10 | `brush_phase_env_is_filtered_of_bash_special_variables` | Runs a real install phase via run_commands_logged. |
| 11 | `install_inherits_a_real_eclass_that_only_exists_in_the_overlays_own_master_repo` | Writes two repos on disk and runs a real install phase that inherits an eclass. |
| 12 | `install_writes_the_real_build_time_file` | Runs a real install phase (run_commands). |
| 13 | `a_merge_upgrade_unmerge_sequence_is_equivalent_on_redb_and_files` | Runs a full merge/upgrade/unmerge sequence on the redb backend (body calls sequence_equivalence). |
| 14 | `install_runs_the_portage_post_install_qa_check_and_strips_a_genuinely_empty_dir` | Runs a real install phase with the post-install QA check. |
| 15 | `install_really_inherits_a_real_eclass_and_calls_its_own_function` | Runs a real install phase that inherits an eclass. |
