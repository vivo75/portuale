# crates/portage-repo/src/tests/merge_bound.rs
Cross-root DEPEND, missing-dep backtrack, missed-update notices, and blocker merge-order tests.

## fn cross_root_depend_forms_the_running_root_cycle
- Track X Slice B (backlog #242, ex-#207): with a non-`/` target ROOT, EAPI >= 7 `DEPEND` resolves against the running root (Portage `_add_pkg_deps`'s `depend_root`), so graph nodes carry their root and candidates select against that root's tree/installed db.
- `=dev-libs/cyc0z-1`'s `DEPEND` pulls `cyc0y` into the (here empty) running root, whose unversioned `DEPEND` then selects `cyc0z-3` -- the target-root argument is not reused across the root boundary -- forming the running-root `cyc0z-3 <-> cyc0y-1` cycle live Portage reports under default staging (backtrack `1/20`, blame `cyc0z-3`; `LLM/evidence/2026-09-29-242-inventory/probes/default-cyc0z-1.txt`).
- Hermetic running root: nothing installed there, so every build dep selects from the tree -- the empty-`/` half of the S0 repro (a parent in <ROOT> whose DEPEND resolves against the running root, where the running-root choice forms a cycle).
- Portage starts the printed cycle at `cyc0z-3`, reached from `cyc0y-1` -- the argument version is not a cycle member. (`cat/pkg-version` CPVs: `cyc0z-3` is version 3 of `cyc0z`.)
- The blame node is a running-root merge: it carries its root.
- Portage spends its single backtrack retry on the serialize abort.

## fn cross_root_bdepend_self_branch_backtrack0_reports_the_self_cycle
- Track X Slice C (#242): the same g216 shape with `--backtrack=0` reports Portage's pass-1 self-cycle block (rc 1), like Portage's `default-g216top-b0.txt` (`Total: 4` over the dual rows).
- The pass-1 graph strands on the running-root self loop: four merge rows (dual `g216comp`) behind the self block. The cycle node is the merge-bound cpv (the `circular_deps` convention everywhere -- Portage prints `(dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled for merge)`).

## fn missing_dep_backtrack_masks_the_top_level_selected_parent
- Backlog #198: with keyword suggestions off (Portage `--autounmask-keep-keywords=y`, i.e. no `_autounmask_levels` step unmasks `~amd64`), the newest `akk0a-2`'s `akk0b` dep is unsatisfiable even USE-free, so the missing-dependency backtrack masks the top-level-selected parent (`depgraph.py:3493-3503`, no top-level exemption) and the retry settles `akk0a-1` + `akk0c-1` (`foo` is globally on, so no USE change is recorded).
- The masked `akk0a-2` rides out as a full-form skipped-missing-dep row (Portage `_show_missed_update_unsatisfied_dep`, `depgraph.py:1592`): selected `akk0a-1`, skipped `akk0a-2`, masked-only `akk0b` disclosure chained to the single skipped parent.

## fn backtrack_missed_updates_reports_the_mg2top_shape
- Backlog #129 (S1): Portage's two notices on the mg2top backtrack shape (`l0-fx-20260927T080515Z` oracle) come from the settled backtrack trial state, not from a second pass. The search settles every `-1`, and the accumulated masks derive the slot-conflict `WARNING` rows (`mgxc-2` vs `=mgxc-1`/`mgxa-1`, `mgfc-3.0` vs `=mgfc-1`/`mgfa-1`, in first-mask order) plus the abbreviated missing-dependency slots (`mgxb:0`, `mgfb:0`).

## fn blk_pg0_all_orders_merge_x1_and_uninstall_y1
- Backlog #142 S2 (upstream `test_blocker.py::testBlocker`, oracle `differential-test-bed/logs/fx-b1-blocker.json` cases 0-5 and the `l0-fx-20260922T101821Z` blk0 cells): Portage merges X-1 and uninstall-orders installed Y-1 in all six argv orders (`--backtrack=0`, rc 0).
- Joint assertion over the whole defect: the joint slot solve (F1) must settle the row at the uniquely jointly-satisfying X-1 from any walk order, the stale-owner gate (F2) must drop every non-merged candidate's blocker, and the keeper walk (F3) must file X-1's own `!=Y-1` so the uninstall pairs Y-1.
