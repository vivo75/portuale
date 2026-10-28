# R3 (#594) implementer report (2026-10-28)

Scope per the R3 amendment (`r3/premise.md`, `r3/s0.md`, `r3/targets.md`):
T1 (a USE-only miss that Portage meets first wins, no restart), T2 (the
`--autounmask-backtrack=n` stop also covers missing-dependency feedback when
the change came before the stop), G0 display (no residual installed-instance
conflicts when the walk stopped). Cross-parent order (`r594qtop
--autounmask=n`, RED 7) stays out for #609. The G0 count (0/20 vs 1/20) stays
R6's. No commit, no push, no bed run, per brief.

## Files changed

portuale (all on `fix/2026-10-26-575-residues`; the `LLM/02.589-...opus.md` /
`LLM/backlog-tasks.md` modifications and `r3/` evidence were already dirty
from the manager/S0 and were not touched):
- `crates/portage-repo/src/resolve/feedback/missing_dep.rs`: post-walk
  retraction of the missing-dep trigger (`trigger_preempted_by_use_miss`,
  `edge_index`). A USE-only miss no autounmask change can fix (no
  `use_suggestion` on its entry), in the same owner's dep string, textually
  ahead of the trigger's atom, means Portage's walk never reached the
  trigger's dep (`depgraph.py:4766-4788`, `:3494-3499` vs `:3517`).
- `crates/portage-repo/src/merge_order/replay.rs`: `replay_key` processes
  target-less edges first (`replay_edge_has_target`, stable sort), so a
  missing dep fails the key before a later atom's USE change is recorded --
  what the function's own docstring already claimed.
- `crates/portage-repo/src/resolve/driver.rs`: the
  `--autounmask-backtrack=n` break also fires on a `BacktrackFeedback::MissingDep`
  when the replay found new needed changes (`r.needed != needed_init`, no
  `failure.is_none()` -- the replay stops at the first failure, so anything
  recorded came before the stop). Same break body as the slot-conflict arm.
- `crates/portage-repo/src/resolve/assemble.rs`: residual
  installed-instance conflicts skipped when `!ctx.complete &&
  pass_has_unsatisfied_dep` (R1's helper).
- `crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs`:
  `pass_has_unsatisfied_dep` widened `pub(super)` -> `pub(crate)`.
- `crates/portage-repo/src/tests/tests_first_unsatisfied_dep.rs` (new,
  registered in `tests/mod.rs`): 7 RED tests, oracles `r3/captures/A-*.txt`.
- `crates/portage-repo/src/tests/tests_p563.rs:299-327`: G0 assertions
  retargeted to no slot-conflict block; the `backtrack_restarts == 0`
  assertion is untouched (R6's count).
- `LLM/code-notes/...` (`resolve/feedback/missing_dep.md`,
  `merge_order/replay.md`, `resolve/assemble.md`, `resolve/driver.md`):
  R3 sections; `code_notes_index.py` clean, `fact_coverage.py` reports
  0 facts lost on the four touched sources.

pmtest (same branch; fixtures + pins only):
- `fixtures/repo/dev-libs/r594{cm,ctop,em,etop,qa,qb,qtop,pm,ptop,km,ktop,gm,gtop,vdep,xdep}/`
  with a real-md5 `metadata/md5-cache/dev-libs/<pf>` entry per ebuild
  (recipe = `r3/mk.sh` families c, e, q plus p/k/g keepers and the
  vdep/xdep leaves); `test_fixture_caches.py` passes.
- `pytests-contract-suite/test_emerge_pretend_contract.py`: 8 CASES rows
  appended at the end of CASES + 3 new pin functions
  (`test_594_use_only_miss_first_in_string_stops_with_no_restart`,
  `test_594_autounmask_change_before_the_stop_ends_the_search`,
  `test_594_stopped_walk_shows_no_residual_slot_conflict`).

## RED -> GREEN evidence

Rust (each failed before the fix, all pass after; keepers `masked_*`,
`use_only_miss_*`, R1 file, full crate suite green throughout):
- `use_only_miss_first_in_string_stops_with_no_restart` (e default):
  restarts 1 -> 0, Aborted.
- `use_only_miss_first_blocks_the_trigger_under_autounmask_n` (c aun):
  restarts 1 -> 0, Aborted.
- `autounmask_change_before_the_stop_ends_the_search[_within_backtrack_1]`
  (q, q bt1): restarts 1 -> 0, Aborted.
- Keepers held: `masked_dep_first_in_string_still_backtracks` (c default
  1/20, 2 rows), `masked_only_shapes_backtrack_once` (p/k/g 1/20),
  `autounmask_backtrack_y_keeps_backtracking` (q aby 1/20, rows; see
  surprise 3), retargeted `g0_benign_disclosure_blocks_the_dropped_pin_heal`.

pmtest pins (`-k test_594`: 8 passed). Pre-fix failure is witnessed by the
S0 `P-*.txt` captures on equivalent scratch fixtures: `P-c-aun.txt` (rc 0,
1/20, 2 rows), `P-e.txt` (rc 0, 1/20), `P-q.txt`/`P-q-bt1.txt` (1/20, 1/1,
4 rows), `P-g0*.txt` (residual conflict block) -- every pin assertion fails
on those. Post-fix CLI spot checks: c-aun / e / q / q-bt1 all rc 1, 0/20
(or 0/1), 0 rows; g0 triple rc 1 with no `Multiple package instances`
block; q prints the USE-change block, "backtracking has terminated early"
and the masked block exactly as `A-q.txt`.

## Gate result (`gate.sh r3`)

- `cargo fmt --check` rc 0; `cargo clippy --release --all-targets` rc 0,
  0 warnings; `cargo build --release` rc 0;
  `cargo test --release` rc 0 (3223 passed, 0 failed).
- pmtest parallel: 4595 passed, 2 skipped, 2 xfailed, 1 failed =
  `test_terminal_title_set_and_reset_on_a_pty`, a named known flake;
  re-run alone passes.

## Corpus drift (reviewed, NOT blessed)

7 drifted `test_pretend_case_exit_code` rows, all stdout-only, all rc-0
passes. Each was re-run with the R3 changes stashed (fixtures kept) and is
byte-identical before/after, i.e. pre-existing stale harvest, unrelated
to R3:
- `[slot conflict: unsolvable, resolved by masking a puller version]`
  (`--pretend dev-libs/btparent`)
- `[slotconflict: upstream test_slot_conflict_mask_update pg0 ... scm0a ...]`
  (`--pretend dev-libs/scm0a`)
- `[backtrack: upstream test_backtracking pg1 A/B selective-update ...]`
  (`--pretend --update --deep --selective dev-libs/bkt1a dev-libs/bkt1b`
  and the reversed order)
- `[backtrack: upstream test_backtracking pg3 D C ...]`
  (`--pretend --backtrack=1 dev-libs/bkt3d dev-libs/bkt3c`)
- `[autounmask: upstream test_autounmask_keep_keywords pg0 keep=y ...]`
  and `[--autounmask-keep-keywords=True spelling ...]`
  (`--pretend --autounmask-keep-keywords=y/True dev-libs/akk0a`)

## Surprises / residues noticed (not fixed; one fix round used)

1. The walk visits a dep string in reverse textual order
   (`enqueue_flat_deps` pushes plains reversed for the stack discipline),
   while Portage yields package-less atoms first, textually
   (`_minimize_children`). So an arm-time trigger gate can never work --
   for `c --autounmask=n` the masked dep is visited before the USE miss
   exists -- and the fix must retract post-walk in Portage order. The same
   reversal is why the first attempt (same-owner arm-time gate) was
   reverted: it would have broken keeper `d --autounmask=n`.
2. `c --autounmask=n` and `e` now print the masked block past Portage's
   stop point (and `e` prints the `Missing IUSE` header where Portage
   prints the masked-with-IUSE form). Both are S0 display residues 6
   (#609's family) and 1; the pins assert the R3-owned blocks and note
   this, they do not pin the extras.
3. `q --autounmask-backtrack=y` resolves Complete with rows but no masked
   block (Portage prints rows + masked + USE blocks, rc 1 either way via
   the pending USE changes). Tries match (1/20); the missing block is a
   pre-existing report-accumulation gap on the backtrack-y path, untouched
   by R3 (verified mechanically: none of the three changes fire there).
4. No other L0-relevant behavior was touched: keepers p/k/l/u/g/v/x, c
   default/bt1, d, f, j, ip/iq/i1 cells were re-checked against the s0
   matrix by code path (no fixture changes for them); the full workspace
   suite and the contract suite show no other delta.
