# Dupes groups skipped by #338
Backlog #338's record: dupes method, before numbers, the three folds, and every skipped group with its reason.

Method: `python3 LLM/tools/dupes.py --json dupes.json` runs
`cargo dupes -p crates --sub-function --min-lines 50` from the repo root.
A sub-function member reports its *enclosing* function's span, so the block
itself was located by reading the enclosing function. Production code first;
folds only for a shared block of about 8 lines or more with one meaning and
a clearly-named helper, stopping after the most valuable folds (~300 changed
lines). `crates/portuale/src/pretend/args.rs` untouched: another slice.

Numbers before (2026-10-11): 1 exact function-level group, 0 near
function-level groups, 883 exact + 50 near sub-function groups
(8890 sub-function units: 385 in args.rs, 392 in test files, rest elsewhere).
Numbers after (2026-10-11, same command): 0 exact function-level groups,
0 near function-level groups, 877 exact + 50 near sub-function groups
(3795 + 104 units). The exact pair is gone; the sub-function drop (-6
groups, -22 units) is the three folds' duplicated blocks no longer
reported, plus span shifts from the edits.

Folds done:
- `check_mask_and_force_stack_repo_then_profile_only_no_user_source`
  (`crates/portage-profile/src/tests/resolve.rs`): the exact pair, bodies
  differing only in file-name and field literals; both test names kept.
- `flatten_evaluated_root_deps_atoms`
  (`crates/portage-repo/src/resolve/root_deps.rs`): the `--root-deps`
  join-flatten-evaluate prefix both halves ran; each keeps its own filter
  tail. One meaning (flatten the build depstring with running-root
  availability), no flags.
- `spawn_bash_script` (`crates/portuale/src/ebuild_phases/commands.rs`):
  the sandbox-spawn tail of `run_one_phase_bash` / `run_misc_functions_bash`
  (curated env, log pump guard, spawn, join). One meaning (run a vendored
  bash helper); the spawn-failure wording stays byte-identical via a
  `context` label.

Skipped:

### args.rs (another slice)
- all 385 `crates/portuale/src/pretend/args.rs` members (187 if-then
  branches, match arms 1-3, if-else branches of one ~1950-line function):
  another worker is splitting the file; skipped entirely.

### test scaffolding
- all 392 members in test files (`*/tests/*`, `tests.rs`, `*test*.rs`).
- 65 enclosing spans inside `#[cfg(test)]` modules of production files:
  crates/mrg-director/src/lib.rs 1612-1750, 1755-1823;
  crates/portage-profile/src/phase_environ.rs 557-634, 563-633, 637-728, 672-726;
  crates/portage-repo/src/solver_bridge.rs 1270-1329, 1334-1422;
  crates/portage-util/src/lib.rs 172-231;
  crates/portage-vdb/src/redb_db.rs 1296-1351, 1432-1506, 1714-1876, 1945-2006, 2536-2655, 2759-2841, 2877-2964;
  crates/portage-vdb/src/sqlite.rs 1340-1519, 1633-1697, 1848-1932, 1967-2070, 2136-2195, 2198-2259, 2262-2316, 2349-2436;
  crates/portuale/src/ebuild_package.rs 1569-1640, 1912-2029, 2032-2116, 2242-2303, 2615-2675;
  crates/portuale/src/elog.rs 1306-1356, 1588-1690, 1619-1687;
  crates/portuale/src/emerge_getbinpkg.rs 2159-2249;
  crates/portuale/src/fetch.rs 2042-2097;
  crates/portuale/src/helpers/chmod_lite.rs 276-326, 366-428;
  crates/portuale/src/helpers/doins.rs 2235-2313, 2642-2821, 2886-2974, 3034-3185, 3042-3143, 3203-3304;
  crates/portuale/src/helpers/gpkg.rs 1510-1588, 1651-1720, 1799-1870, 1874-1926, 2014-2105, 2124-2185, 2504-2559, 2636-2713;
  crates/portuale/src/helpers/locale.rs 96-155;
  crates/portuale/src/helpers/xattr.rs 768-825, 923-1049, 1126-1222, 1264-1349, 1266-1337, 1353-1477;
  crates/portuale/src/helpers/xpak.rs 319-378;
  crates/portuale/src/info_files.rs 449-499;
  crates/portuale/src/mrg.rs 2522-2655, 2808-2941;
  crates/portuale/src/portageq.rs 1008-1093;
  crates/portuale/src/vdb_cmd.rs 797-875;
  crates/portuale/src/vdb_view.rs 1021-1119, 1446-1509.
  Includes the sqlite/redb `snapshot_entry_image_and_reverse_dependents`
  pair: backend-seeding lines differ, assertion bodies match, but both are
  tests, so left as scaffolding.

### production code left alone (per enclosing function)
### crates/portage-dep/src/lib.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  256-334 x6
- enclosing  505-575 x10
- enclosing  1039-1141 x3
### crates/portage-fetch/src/layout.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  136-193 x2
### crates/portage-fetch/src/lib.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  169-234 x7
- enclosing  447-511 x12
- enclosing  550-613 x5
### crates/portage-fetch/src/mirror_cache.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  270-373 x9
### crates/portage-profile/src/binrepo.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  52-171 x7
### crates/portage-profile/src/config.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  277-339 x6
### crates/portage-profile/src/fingerprints.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  11-72 x1
### crates/portage-profile/src/make_conf.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  8-69 x6
- enclosing  75-130 x7
- enclosing  167-223 x8
### crates/portage-profile/src/phase_environ.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  323-401 x6
### crates/portage-profile/src/profile_chain.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  114-163 x1
### crates/portage-profile/src/resolve/licenses.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-68 x5
### crates/portage-profile/src/resolve/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  36-168 x1
### crates/portage-profile/src/resolve/package_env.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-58 x3
### crates/portage-profile/src/resolve/package_masks.rs
reason: shared block below the ~8-line bar (inspected): a 2-4-line chain-level read loop; follow-ups differ per file
- enclosing  17-110 x4
### crates/portage-profile/src/resolve/package_sets.rs
reason: shared block below the ~8-line bar (inspected): a 2-4-line chain-level read loop; follow-ups differ per file
- enclosing  6-55 x4
### crates/portage-profile/src/resolve/package_use.rs
reason: shared block below the ~8-line bar (inspected): a 2-4-line chain-level read loop; follow-ups differ per file
- enclosing  17-97 x3
### crates/portage-profile/src/resolve/scalar_layers.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  24-188 x8
### crates/portage-profile/src/resolve/use_expand.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-162 x3
### crates/portage-profile/src/resolve/use_force_mask.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-79 x5
### crates/portage-repo/src/binary_index.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  265-321 x5
- enclosing  328-383 x4
### crates/portage-repo/src/binpkg_use.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  52-152 x4
- enclosing  175-258 x3
### crates/portage-repo/src/candidates.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  117-173 x2
### crates/portage-repo/src/dep_chain.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  28-165 x8
- enclosing  258-362 x3
### crates/portage-repo/src/depclean.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  44-172 x8
- enclosing  231-323 x2
- enclosing  346-531 x32
- enclosing  542-727 x27
- enclosing  796-855 x5
### crates/portage-repo/src/info_pkgs.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  60-136 x5
- enclosing  143-257 x12
- enclosing  314-416 x10
- enclosing  454-533 x2
- enclosing  542-656 x5
### crates/portage-repo/src/installed.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  251-301 x2
### crates/portage-repo/src/license.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  29-129 x14
### crates/portage-repo/src/masked_deps.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  14-100 x8
- enclosing  107-184 x2
- enclosing  211-288 x9
- enclosing  332-410 x3
- enclosing  417-506 x7
- enclosing  567-620 x4
### crates/portage-repo/src/md5_dict.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  140-205 x2
### crates/portage-repo/src/merge_order/alt_edges.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  11-126 x19
- enclosing  28-124 x19
### crates/portage-repo/src/merge_order/build.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  10-228 x4
### crates/portage-repo/src/merge_order/cycles.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  21-79 x4
- enclosing  142-204 x4
### crates/portage-repo/src/merge_order/debug.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  130-189 x3
### crates/portage-repo/src/merge_order/installed_closure.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  121-326 x6
### crates/portage-repo/src/merge_order/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  144-208 x3
### crates/portage-repo/src/merge_order/prelude.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  45-132 x7
### crates/portage-repo/src/merge_order/priority.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  51-112 x6
- enclosing  140-220 x2
### crates/portage-repo/src/merge_order/replay.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  69-169 x1
- enclosing  188-388 x20
### crates/portage-repo/src/merge_order/select_nodes.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-388 x25
### crates/portage-repo/src/metadata_cache.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  159-215 x1
### crates/portage-repo/src/reinstall.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  79-140 x5
- enclosing  151-208 x4
### crates/portage-repo/src/repo_config.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  193-443 x24
### crates/portage-repo/src/resolve/assemble.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  5-555 x29
### crates/portage-repo/src/resolve/autounmask.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  10-109 x8
### crates/portage-repo/src/resolve/backtrack_params.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  44-162 x16
- enclosing  79-156 x12
### crates/portage-repo/src/resolve/backtracker.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  164-237 x3
### crates/portage-repo/src/resolve/blockers.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  253-339 x6
- enclosing  364-497 x2
- enclosing  515-877 x18
### crates/portage-repo/src/resolve/conflict_use_display.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  192-275 x8
### crates/portage-repo/src/resolve/ctx.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  91-199 x5
### crates/portage-repo/src/resolve/cycles.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  71-243 x8
- enclosing  250-370 x3
- enclosing  378-482 x3
- enclosing  567-763 x12
- enclosing  768-830 x4
### crates/portage-repo/src/resolve/direct_solve.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  47-98 x1
- enclosing  174-524 x18
### crates/portage-repo/src/resolve/driver.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  5-246 x12
### crates/portage-repo/src/resolve/enqueue.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  61-432 x24
### crates/portage-repo/src/resolve/entry.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  166-215 x2
### crates/portage-repo/src/resolve/feedback/circular_dead_end.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-106 x4
### crates/portage-repo/src/resolve/feedback/missing_dep.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-59 x1
### crates/portage-repo/src/resolve/feedback/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  107-216 x7
### crates/portage-repo/src/resolve/feedback/no_leaf_restart.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-86 x1
### crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  6-125 x6
### crates/portage-repo/src/resolve/feedback/slot_conflict_abi.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-75 x4
### crates/portage-repo/src/resolve/feedback/slot_conflict_backtrack.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-100 x4
### crates/portage-repo/src/resolve/feedback/solvable_slot_conflicts.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-59 x3
### crates/portage-repo/src/resolve/feedback/unsatisfied_dead_end.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-163 x3
### crates/portage-repo/src/resolve/graph_lookup.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  61-110 x2
- enclosing  157-206 x1
### crates/portage-repo/src/resolve/installed_meta.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  101-174 x6
### crates/portage-repo/src/resolve/or_preference.rs
reason: coincidental shape (inspected): repeated 1-2-line `parse_atom(...).is_some_and/map` guards inside long functions
- enclosing  26-127 x11
- enclosing  38-126 x11
- enclosing  167-371 x26
- enclosing  403-518 x9
### crates/portage-repo/src/resolve/pass/candidate_pool.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  18-135 x8
### crates/portage-repo/src/resolve/pass/enqueue_deps.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-513 x42
### crates/portage-repo/src/resolve/pass/entry_fetch.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-94 x1
### crates/portage-repo/src/resolve/pass/entry_use_display.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  27-186 x11
### crates/portage-repo/src/resolve/pass/entry_use_flags.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-98 x6
### crates/portage-repo/src/resolve/pass/existing_slot.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  24-338 x21
### crates/portage-repo/src/resolve/pass/graph_entry.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  35-222 x3
### crates/portage-repo/src/resolve/pass/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  33-1046 x42
### crates/portage-repo/src/resolve/pass/parent_use_flip.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  23-241 x17
### crates/portage-repo/src/resolve/pass/slot_conflicts.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  7-156 x2
### crates/portage-repo/src/resolve/pass/top_level_unsatisfied.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  22-175 x9
### crates/portage-repo/src/resolve/pass/unresolved_dep.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  26-495 x41
### crates/portage-repo/src/resolve/pass_helpers.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  94-232 x10
- enclosing  268-382 x7
### crates/portage-repo/src/resolve/pretend.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  31-83 x3
- enclosing  90-192 x3
- enclosing  370-1091 x41
### crates/portage-repo/src/resolve/queue.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  211-300 x2
### crates/portage-repo/src/resolve/replay.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  124-223 x7
- enclosing  271-340 x3
- enclosing  347-449 x3
### crates/portage-repo/src/resolve/result.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  111-216 x5
### crates/portage-repo/src/resolve/rev_dep.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  110-192 x2
- enclosing  282-620 x14
### crates/portage-repo/src/resolve/root_deps.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  19-68 x8
- enclosing  87-138 x9
- enclosing  184-315 x3
### crates/portage-repo/src/resolve/skipped.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  81-190 x11
- enclosing  197-247 x3
### crates/portage-repo/src/resolve/slot_conflict.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  103-231 x20
- enclosing  281-389 x9
- enclosing  408-493 x1
- enclosing  515-762 x18
### crates/portage-repo/src/resolve/slot_expand.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  31-203 x7
- enclosing  227-634 x34
### crates/portage-repo/src/resolve/withhold.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  9-169 x7
- enclosing  210-437 x9
### crates/portage-repo/src/resolver_trace.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  172-230 x1
- enclosing  362-678 x24
### crates/portage-repo/src/slot_operator/abi_probe.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  35-100 x4
- enclosing  179-339 x6
- enclosing  347-412 x3
- enclosing  495-569 x5
- enclosing  587-646 x2
### crates/portage-repo/src/slot_operator/bind.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  92-144 x3
### crates/portage-repo/src/slot_operator/eliminate.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-155 x6
- enclosing  176-304 x11
- enclosing  220-300 x10
- enclosing  311-361 x5
- enclosing  388-543 x5
### crates/portage-repo/src/slot_operator/force_scan.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  35-230 x11
- enclosing  253-408 x5
### crates/portage-repo/src/slot_operator/rebuild_scan.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  35-393 x32
- enclosing  180-351 x25
### crates/portage-repo/src/slot_operator/unsat_probe.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  77-295 x6
### crates/portage-repo/src/solver_bridge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  154-207 x2
- enclosing  337-713 x11
- enclosing  754-808 x4
- enclosing  825-930 x2
- enclosing  944-1079 x5
### crates/portage-repo/src/use_display.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  134-214 x8
- enclosing  230-402 x13
- enclosing  261-311 x12
### crates/portage-repo/src/use_flags.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  106-256 x15
### crates/portage-repo/src/use_unsat.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  20-103 x4
- enclosing  129-317 x9
### crates/portage-repo/src/visibility.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  21-77 x2
- enclosing  253-319 x1
### crates/portage-repo/src/visibility_use_flip.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  281-383 x11
- enclosing  401-530 x4
- enclosing  550-631 x4
### crates/portage-required-use/src/lib.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  332-502 x11
### crates/portage-use-reduce/src/lib.rs
reason: would need flag parameters (inspected): the three depstring parsers share the token-validation skeleton but differ in stack element type and `)` handling; folding needs generics/closures
- enclosing  143-223 x11
- enclosing  237-302 x12
- enclosing  318-402 x8
- enclosing  484-590 x16
- enclosing  771-914 x2
### crates/portage-util/src/varexpand.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  12-97 x12
### crates/portage-vdb/src/convert.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  145-202 x13
- enclosing  215-287 x5
### crates/portage-vdb/src/files.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  233-301 x1
- enclosing  584-636 x5
### crates/portage-vdb/src/files_write.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  245-317 x8
- enclosing  724-779 x1
### crates/portage-vdb/src/redb_db.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  1035-1091 x3
### crates/portage-versions/src/lib.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  209-258 x5
### crates/portuale/src/binpkg/extract.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  143-215 x8
- enclosing  314-365 x3
### crates/portuale/src/binpkg/gpg.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  155-215 x7
### crates/portuale/src/binpkg/inner.rs
reason: coincidental shape (inspected): shared ~6-line trailing-slash-strip + lossy-show; validity checks and error messages differ
- enclosing  114-211 x8
- enclosing  221-274 x2
- enclosing  324-394 x4
### crates/portuale/src/binpkg/local_index.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  60-197 x5
- enclosing  260-374 x9
### crates/portuale/src/binpkg/manifest.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  27-217 x17
### crates/portuale/src/binpkg/outer.rs
reason: coincidental shape (inspected): shared ~6-line trailing-slash-strip + lossy-show; validity checks and error messages differ
- enclosing  32-100 x9
### crates/portuale/src/color.rs
reason: nested-unit artifact (inspected): overlapping spans 140-209/143-208 are the same region counted twice
- enclosing  140-209 x9
- enclosing  143-208 x9
### crates/portuale/src/difflib.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  138-190 x1
### crates/portuale/src/ebuild.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  72-423 x28
### crates/portuale/src/ebuild_merge/after_install.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  5-290 x11
- enclosing  319-405 x2
- enclosing  450-546 x3
- enclosing  568-618 x3
### crates/portuale/src/ebuild_merge/collisions.rs
reason: nested-unit artifact (inspected): overlapping spans 98-154/102-152 are the same region counted twice
- enclosing  17-91 x8
- enclosing  24-89 x8
- enclosing  98-154 x9
- enclosing  102-152 x9
- enclosing  182-270 x5
### crates/portuale/src/ebuild_merge/from_binpkg.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  31-413 x23
### crates/portuale/src/ebuild_merge/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  142-223 x3
### crates/portuale/src/ebuild_merge/plib_merge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  48-105 x2
### crates/portuale/src/ebuild_merge/plib_unmerge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  59-136 x6
- enclosing  192-267 x5
- enclosing  317-400 x5
### crates/portuale/src/ebuild_merge/protect.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  285-348 x8
### crates/portuale/src/ebuild_merge/tree.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  87-293 x19
- enclosing  304-353 x4
### crates/portuale/src/ebuild_merge/vdb_entry.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  129-184 x1
### crates/portuale/src/ebuild_package.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  473-523 x7
- enclosing  627-825 x15
- enclosing  936-1045 x5
- enclosing  1078-1321 x25
### crates/portuale/src/ebuild_phases/commands.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  27-143 x3
- enclosing  280-351 x6
### crates/portuale/src/ebuild_phases/depend_phase.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  110-164 x2
### crates/portuale/src/ebuild_phases/environment.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  21-97 x5
### crates/portuale/src/ebuild_phases/fetch_sources.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  182-276 x1
### crates/portuale/src/ebuild_phases/misc_functions.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  163-225 x2
### crates/portuale/src/ebuild_phases/package_env.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  84-141 x1
### crates/portuale/src/ebuild_phases/phase_env.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  72-291 x6
### crates/portuale/src/ebuild_phases/post_install_meta.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  58-155 x7
- enclosing  161-240 x10
### crates/portuale/src/ebuild_phases/run_phase.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  234-283 x2
### crates/portuale/src/ebuild_phases/standalone_config.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  118-192 x5
- enclosing  128-183 x4
### crates/portuale/src/ebuild_unmerge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  74-186 x5
- enclosing  228-285 x5
- enclosing  389-535 x9
- enclosing  576-643 x2
### crates/portuale/src/elog.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  620-698 x1
- enclosing  741-844 x2
- enclosing  984-1044 x4
- enclosing  1067-1124 x1
### crates/portuale/src/emerge_build/buildpkgonly.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  23-176 x8
- enclosing  224-279 x1
### crates/portuale/src/emerge_build/logs.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  70-124 x1
### crates/portuale/src/emerge_build/resume.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  91-153 x8
### crates/portuale/src/emerge_build/scheduler.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  26-159 x6
- enclosing  177-245 x4
- enclosing  306-497 x17
- enclosing  356-471 x13
### crates/portuale/src/emerge_build/source_merge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  35-153 x7
- enclosing  103-152 x7
- enclosing  161-242 x6
- enclosing  259-363 x8
### crates/portuale/src/emerge_getbinpkg.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  145-302 x8
- enclosing  362-472 x3
- enclosing  513-606 x5
- enclosing  555-605 x4
- enclosing  623-836 x12
- enclosing  1052-1129 x10
### crates/portuale/src/env_update.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  181-301 x16
### crates/portuale/src/fetch.rs
reason: same-function conditional sides (inspected `assemble_candidates` 162-211): if-then vs if-else of one conditional, not a repeated block
- enclosing  162-211 x5
- enclosing  311-375 x4
- enclosing  460-696 x8
### crates/portuale/src/helpers/chmod_lite.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  89-138 x2
### crates/portuale/src/helpers/doins.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  183-239 x4
- enclosing  548-723 x22
- enclosing  810-893 x7
- enclosing  895-1054 x14
- enclosing  1709-1771 x6
- enclosing  1927-1976 x6
- enclosing  1985-2040 x4
### crates/portuale/src/helpers/filter_env.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  90-154 x2
- enclosing  263-351 x4
### crates/portuale/src/helpers/gpkg.rs
reason: coincidental shape (inspected): repeated 1-line `io_msg(&path.display()...)` mappings and match arms; no >=8-line single-meaning block
- enclosing  720-845 x3
- enclosing  864-913 x4
- enclosing  970-1070 x11
- enclosing  1121-1199 x8
- enclosing  1287-1434 x7
### crates/portuale/src/helpers/python.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  14-76 x12
### crates/portuale/src/helpers/xattr.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  187-281 x8
- enclosing  415-534 x13
- enclosing  688-748 x4
### crates/portuale/src/info_files.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  159-338 x9
### crates/portuale/src/mrg.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  1187-1257 x7
- enclosing  1341-1397 x1
- enclosing  1733-1791 x1
### crates/portuale/src/needed_elf.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  274-407 x4
- enclosing  516-593 x4
- enclosing  665-768 x6
- enclosing  906-998 x2
- enclosing  1022-1127 x3
- enclosing  1157-1225 x5
### crates/portuale/src/portageq.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  67-204 x6
- enclosing  222-271 x2
### crates/portuale/src/preserved_libs.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  36-140 x6
### crates/portuale/src/pretend/autounmask_report.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  20-114 x4
### crates/portuale/src/pretend/config_action.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-162 x8
### crates/portuale/src/pretend/conflicts.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  86-153 x8
- enclosing  159-262 x11
- enclosing  391-447 x6
### crates/portuale/src/pretend/deselect.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  17-168 x14
### crates/portuale/src/pretend/display/attrs.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  118-183 x13
### crates/portuale/src/pretend/display/blockers.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  103-174 x2
### crates/portuale/src/pretend/display/circular.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  55-175 x3
### crates/portuale/src/pretend/display/counters.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-173 x18
### crates/portuale/src/pretend/display/entry_line.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  54-598 x27
- enclosing  222-302 x9
### crates/portuale/src/pretend/display/tree.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  43-444 x14
- enclosing  251-313 x3
### crates/portuale/src/pretend/early_actions.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  5-172 x9
### crates/portuale/src/pretend/env_config.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  30-92 x8
- enclosing  96-150 x4
- enclosing  228-283 x2
### crates/portuale/src/pretend/expand_targets.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-228 x16
### crates/portuale/src/pretend/final_gates.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  20-186 x6
- enclosing  189-282 x2
- enclosing  296-360 x4
### crates/portuale/src/pretend/info.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  89-262 x13
- enclosing  317-752 x18
### crates/portuale/src/pretend/json.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  33-245 x5
- enclosing  363-433 x2
### crates/portuale/src/pretend/merge_list.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  26-99 x1
- enclosing  129-359 x1
- enclosing  373-452 x5
### crates/portuale/src/pretend/merge_run.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  25-101 x1
- enclosing  119-244 x6
### crates/portuale/src/pretend/merge_setup.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  21-85 x1
- enclosing  103-214 x5
### crates/portuale/src/pretend/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  148-662 x9
### crates/portuale/src/pretend/news.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  67-130 x2
### crates/portuale/src/pretend/post_merge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  74-125 x2
### crates/portuale/src/pretend/problem_reports.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  9-79 x2
- enclosing  82-144 x1
- enclosing  147-288 x3
- enclosing  379-743 x13
### crates/portuale/src/pretend/removal/depclean.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  23-90 x1
- enclosing  226-445 x9
### crates/portuale/src/pretend/removal/prune_clean.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  31-114 x3
- enclosing  247-401 x11
### crates/portuale/src/pretend/removal/unmerge.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  82-400 x27
- enclosing  437-560 x4
- enclosing  656-716 x3
### crates/portuale/src/pretend/resolve_phases.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  22-274 x9
- enclosing  91-165 x4
### crates/portuale/src/pretend/resolve_plan.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  44-312 x5
### crates/portuale/src/pretend/resume.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  60-342 x6
### crates/portuale/src/pretend/search.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  82-259 x10
- enclosing  290-355 x5
### crates/portuale/src/pretend/standalone_actions.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  18-287 x9
### crates/portuale/src/pretend/world.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  106-168 x5
- enclosing  237-292 x1
### crates/portuale/src/regen.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  84-241 x2
- enclosing  281-341 x1
### crates/portuale/src/remote/binpkg_flow.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  200-450 x14
- enclosing  452-511 x5
### crates/portuale/src/remote/client_binary.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  205-272 x4
- enclosing  415-576 x8
### crates/portuale/src/remote/config_place.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  93-187 x6
- enclosing  192-272 x5
### crates/portuale/src/remote/merge_script.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  305-388 x3
- enclosing  430-505 x4
### crates/portuale/src/remote/mod.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  292-405 x12
### crates/portuale/src/remote/plan.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  59-131 x9
- enclosing  174-347 x7
- enclosing  373-463 x1
- enclosing  469-526 x7
### crates/portuale/src/remote/preflight.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  90-148 x7
### crates/portuale/src/remote/ssh.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  285-339 x3
### crates/portuale/src/remote/stages.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  44-99 x2
- enclosing  131-184 x3
- enclosing  303-356 x2
- enclosing  445-544 x5
- enclosing  558-612 x5
### crates/portuale/src/remote_bundle.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  207-268 x5
- enclosing  301-479 x13
### crates/portuale/src/vdb_cmd.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  374-425 x5
- enclosing  504-555 x5
- enclosing  557-623 x8
### crates/portuale/src/vdb_fuse.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  644-711 x3
- enclosing  719-820 x3
### crates/portuale/src/vdb_rw.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  477-554 x3
- enclosing  565-628 x3
- enclosing  708-788 x7
- enclosing  1164-1223 x4
- enclosing  1251-1302 x4
- enclosing  1330-1404 x8
### crates/portuale/src/vdb_rw/ops.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  154-215 x6
- enclosing  238-294 x4
- enclosing  385-449 x2
### crates/portuale/src/vdb_view.rs
reason: coincidental small shapes (sampled pattern) - not individually triaged; budget stopped after the most valuable folds
- enclosing  523-589 x3

## #339 R4: duplicate pair, kept apart
`expand_resolved_slot_with_flipped_use` (`resolve/slot_expand.rs`) and `enqueue_dependencies` (`resolve/enqueue.rs`) have the same shape (flatten keys, `||` flatten, root-deps split, running-root, enqueue loop), and the cuts `flatten_deps_for_flip`/`flatten_deps_for_enqueue` and the two `group_running` helpers stay twins on purpose: they read different inputs (delta metadata under the flipped USE vs. the vdb-recorded strings), so folding them would need an adapter bigger than the shared code. Recorded for #338, not folded.
