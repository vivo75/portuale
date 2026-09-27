# Report — Track R, backlog #129 S1: real's skipped-updates notice on the mg2top shape

Status: **DONE_WITH_CONCERNS** (code, pins and gates green; 1 corpus-bless-gated
assert pending; container beds pending coordinator).

## What was built

Portuale now emits real's two backtrack notices by feeding the existing
`skipped_updates` mechanism (#92) from the settled backtrack trial state --
no second resolver pass (#107 stays PARKED), no NEEDS_CONTEXT.

- `backtrack_missed_updates` (`rust/portage-repo/src/lib.rs`): a port of real
  `_get_missed_updates` (`3rdparty/portage/lib/_emerge/depgraph.py:1529-1565`)
  over the settled `BacktrackParams::runtime_pkg_mask` (masks accumulated
  across backtracking runs, real `:706`). `_conflict_missed_update`
  (`:2090-2106`) already rides `skipped_updates` via the direct solve (#90
  S2). `"slot conflict"` masks become `SkippedUpdate` rows (one per rejecting
  parent, the established provisional shape); `"missing dependency"` masks
  become the new `GraphResult::skipped_missing_deps` slots, but only when the
  atom still matches a backtrack-masked version at settle time -- real's
  `check_backtrack` probe (`:6471-6484`, abbreviated tail `:1638-1649`).
- First-seen mask order: new `BacktrackParams::mask_order` (real's dict
  insertion order, which the render follows); excluded from `params_equal`
  like `depth`/`mask_steps`. Replaces keep first-seen position like real's
  dict assignment.
- Wired in `assemble_result`, gated on a clean `Complete` settle with no
  surviving slot conflicts, no unsolvable blocker rows and no orphans (real
  `display_problems` shows missed updates only with no unresolved conflicts,
  `:11127-11132`). `--backtrack=0` derives nothing.
- Renderer (`rust/portuale/src/pretend.rs`): the abbreviated `!!! .../!!!
  triggered by backtracking:` tail after the `WARNING` block, byte-exact
  blank-line structure.
- `solver_bridge.rs`: bridge engines run no backtrack loop, both lists empty.

## Oracle fidelity

`--pretend --tree dev-libs/mg2top` now matches
`l0-fx-20260927T080515Z` real output byte-for-byte modulo portuale's
established cuts (no `to '<ROOT>'` suffixes; bare `USE=""` where real shows
profile `ELIBC="glibc"`/`ABI_X86="(64)"` -- same cut as the #90 skip pin).
The settled masks mirror real's final mask dict exactly
(`mgxc-2` slot / `mgfc-3.0`+`mgfc-2` slot / `mgxb-2`,`mgfb-2` missing);
`--pretend` flat matches the same way. Deliberate narrowings are
documented on the function: single root, installed masked versions never
reported, metadata-less versions skipped, missing-dep full form (probe
doesn't raise) skipped not rendered, no cross-source collapse against
direct-solve rows, parents merge-scheduled.

## Tests and gates

- Rust: `backtrack_missed_updates_reports_the_mg2top_shape`,
  `backtrack_missed_updates_stays_silent_without_a_clean_settle`,
  `mask_order` exclusion in the `params_equal` test. `cargo fmt --check`
  clean, `cargo clippy --release --all-targets` 0 warnings,
  `cargo test --release` workspace 1688 passed / 0 failed.
- pmtest: extended `test_tree_mg2top_...` (its docstring deferred the
  notice to #129), extended the orbt and btparent pins (their new
  `WARNING`/tail output follows the same real rule -- the 42 script's
  row-only filter never captured notices, but its verified mergelist
  proves the masks this reads), doc notes on the mg2/mg3 flat tests.
  Fixture already existed; no new fixture.
- Full suite (`PMTEST_PROFILE=release`, `--basetemp=/var/tmp/pmtest-r129d`,
  log `/tmp/opencode/r129/pmtest2.log`): **1960 passed / 5 failed /
  37 skipped / 5 xfailed**. 4 known (3 gpkg `test_emerge_buildpkgonly_*`
  per Z0 + `pid-sandbox` host). 5th: orbt `_assert_harvested` only -- its
  stdout assertions pass; the corpus entry predates the notice.
- Corpus drift (reported, NOT blessed): 4 contract keys (orbtblocked#0 x2,
  mg3#0, mgf#0, mg2#0) + 48 expanded keys over the six backtracked shapes
  (`btgp`, `btparent`, `mg2top`, `mgfa`, `mgxa`, `orbtblocked`) x flag
  variants. All are the notices real prints there. **Coordinator: bless.**

## Review fixes

Fix brief `r129-fix-brief.md` (review `r129-review.md`: Important 1,
Minor 2-4; Minor 5 stays with the coordinator). Commit
`resolve: gate the skipped-updates notices on --quiet like real (#129 review)`
(portuale only -- no pin touched, so no pmtest commit).

1. **Quiet gate (Important 1 + Minor 2).** Both `pretend.rs` blocks
   (`skipped_updates` WARNING, `skipped_missing_deps` tail) are now
   gated on `!(quiet && !debug)` -- exactly real
   `_show_missed_update` (`lib/_emerge/depgraph.py:1576-1581`, drops
   the `"slot conflict"` and `"missing dependency"` types; the gate
   is type-agnostic so it covers the #90 direct-solve rows too).
   The `:12536` comment (and the twin claims on `SkippedUpdate` /
   `GraphResult::skipped_updates` in `lib.rs`) now describe the code:
   suppressed under `--quiet` unless `--debug`; `--json` returns
   before the block; no `--columns` gate (real shows notices
   regardless of columns). The 6 named
   `expanded: --pretend --quiet dev-libs/{btgp,btparent,mg2top,mgfa,
   mgxa,orbtblocked}` drifts are gone.
2. **Cross-source collapse (Minor 3).** The call-site
   `append` + consecutive `dedup()` is replaced by
   `collapse_skipped_updates`: both sources collapse per
   `(category, package, slot)` (single root, so root is constant)
   keeping the highest `skipped_version`, like real `:1553-1562`;
   equal versions all stay (one row per parent), exact duplicates
   collapse to one, mask rows lead. The "no cross-source collapse"
   narrowing is struck from `backtrack_missed_updates`' docs. New
   unit test `collapse_skipped_updates_keeps_the_highest_version_per_slot`
   covers a slot present in both sources (each side higher once),
   equal-version parents, and exact dupes.
3. **Bless claim (Minor 4).** Corrected to what is verified, no
   bless (coordinator decides): mg2top by the `l0-fx-20260927T080515Z`
   bed oracle; mgfa/btparent/orbtblocked live-verified by the
   reviewer against host real 3.0.82.2; blk0b/c/a live-verified
   below; every other drifted key is "same mechanism", not
   directly oracled.

Gates: `cargo fmt --check` clean, `cargo clippy --release
--all-targets` 0 warnings (one `type_complexity` on the first draft,
fixed via a `SkippedUpdateKey` alias), `cargo test --release`
workspace **1689 passed / 0 failed** (log
`/tmp/opencode/r129-fix/cargo-test.log`). pmtest suite
(`PMTEST_PROFILE=release`, `--basetemp=/var/tmp/pmtest-r129fix`,
log `/tmp/opencode/r129-fix/pmtest.log`): **1956 passed / 9 failed
/ 37 skipped / 5 xfailed**. 4 known (3 gpkg `test_emerge_buildpkgonly_*`
per Z0 + `pid-sandbox` host). 1 orbt corpus-bless assert (output
assertions pass; entry predates the notice). 3 package-moves
failures are environmental: the shared pmtest fixture vdb was
already dirty at session start (`oldmovepkg-1.0` deleted,
`newmovepkg-1.0` untracked, `slotmovepkg-1.0/SLOT` modified, all
timestamped 09:27, i.e. during S1's suite window) and those tests
pin exact output against committed vdb state. 1 upstream-blocker
failure is this slice's real-correct change -- see below. Not
touched (common-rule 7): pins, oracles, corpus (no bless).

**Remaining drift, by key (52 lines, all reported not blessed).**
Contract (8): orbt `test_or_group_...#0` x2 (intended notice);
`test_oracle_slot_conflict_masks_highest_version_first#0`,
`test_oracle_two_simultaneous_conflicts_defer_second_to_later_pass#0`,
`test_oracle_missed_update_siblings_masked_together#0` (intended
notices, same mechanism); `test_profiles_updates_...[args5]#0`,
`[args7]#0`, `test_package_moves_n_disables_profiles_updates#1`
(environmental dirt, above). Expanded (44): the six backtracked
shapes x {emptytree, tree, update-deep-newuse, update-deep,
update, verbose, flat} (intended notices) plus `--pretend --quiet
dev-libs/libgit2-glib` and `--pretend --quiet dev-vcs/gitg`: their
stored baselines contain the WARNING under quiet (harvest-era
behaviour) and the new gate removes it -- real-correct per
`:1576-1581`, so these two are re-baseline candidates, not
regressions.

**Failing pin owned by #142, not this slice.**
`test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`
(order `blk0b blk0c blk0a`): the collapse drops the provisional
`blk0x-2` block, keeping `blk0x-3`. Live-verified against host real
3.0.82.2 on the fixture tree just now: real prints exactly one
`dev-libs/blk0x:0` block with the highest skipped version (`-3`)
and both parents (`<blk0x-2`/blk0b, `<blk0x-3`/blk0c) inside it --
the pin's three-block expectation (with an `-2` block) is stale on
its face, and the pin's own docstring already routes this outcome
("provisional for the other four orders ... any divergence from
real there reopens #142 instead of re-pinning here"). Per rule 7
the pin is left untouched for the #142 close-out bed.

## Review fixes — bed notes (coordinator only)

- Small-case oracle + L0 by name: expect the six `--quiet` notice
  lines gone from the findings; add two `--quiet` cells
  (`libgit2-glib`, `gitg`) whose real output should show merge rows
  only (gate check); add a `blk0b blk0c blk0a --backtrack=0` cell
  whose real output is the single-`-3`-block form quoted above.
- No merge-path change (resolver display only): L1 gate not
  triggered.

- Small-case oracle + L0 by name (brief): expect the mg2top notice lines
  gone from the findings; orbt/btparent notice lines are new real-correct
  output. Suggest extending `42-or-backtrack.sh`'s `filt` to include the
  `WARNING` lines so the notice stays covered.
- No merge-path change (resolver display only): L1 gate not triggered.

## Suggested follow-ups (not filed -- coordinator owns backlog numbers)

1. `-q`/`--columns` print both notice blocks (pre-existing #90 S2 gap: the
   comment claims silent, the code doesn't gate); this slice matches the
   adjacent block. One follow-up for both. **[Review fix: the `-q`
   half is done -- both blocks are now gated on `quiet && !debug`
   like real; `--columns` stays un-gated, which matches real. No
   follow-up left on `-q`.]**
2. Missing-dep full form (probe doesn't raise) needs a
   `_show_unsatisfied_dep` port -- own slice.
3. Cross-source mask-vs-direct collapse needs an oracle (un-oracled overlap).
   **[Review fix: done -- `collapse_skipped_updates` + unit test, and a
   live host-real oracle for `blk0b blk0c blk0a` (single highest-version
   block); the stale provisional pin stays for the #142 close-out bed.]**
4. No docs touched (`what-this-proves.md`, `backlog-tasks.md` #129 entry):
   left for the coordinator's batch bookkeeping.
