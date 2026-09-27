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

## BED-PENDING (coordinator only)

- Small-case oracle + L0 by name (brief): expect the mg2top notice lines
  gone from the findings; orbt/btparent notice lines are new real-correct
  output. Suggest extending `42-or-backtrack.sh`'s `filt` to include the
  `WARNING` lines so the notice stays covered.
- No merge-path change (resolver display only): L1 gate not triggered.

## Suggested follow-ups (not filed -- coordinator owns backlog numbers)

1. `-q`/`--columns` print both notice blocks (pre-existing #90 S2 gap: the
   comment claims silent, the code doesn't gate); this slice matches the
   adjacent block. One follow-up for both.
2. Missing-dep full form (probe doesn't raise) needs a
   `_show_unsatisfied_dep` port -- own slice.
3. Cross-source mask-vs-direct collapse needs an oracle (un-oracled overlap).
4. No docs touched (`what-this-proves.md`, `backlog-tasks.md` #129 entry):
   left for the coordinator's batch bookkeeping.
