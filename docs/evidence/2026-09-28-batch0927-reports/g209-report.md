# g209 report — Track G10, backlog #209: `--backtrack=0` must not skip the reverse-dependency feed loop

Status: DONE (with two disclosed caveats, both below; no NEEDS_CONTEXT — no judgment call was left open by the brief).

Branch: `backlog/209-backtrack0-feed-loop` (both worktrees).

## S0 — diagnosis (grounded)

Portuale's settle (`rust/portage-repo/src/lib.rs`, `backtracking_resolve`):
`collect_feedback`'s reverse-dep scan runs post-pass and returns Config
feedback carrying the enforced pins as `slot_constraints`, but the
`backtrack_max == 0` arm discarded every feedback kind and settled the
root pass — the pin never applied. Reproduced locally (debug build,
copied fixture ROOT + world `dev-libs/r25consumer`):
`--backtrack=0 -uD r25target` merged `r25lib-2.0` (plus a "causing
rebuilds" block), where real withholds it.

Real (3rdparty/portage 3.0.82.2, `lib/_emerge/depgraph.py`): `_resolve_conflicts`
(:9444) calls `_complete_graph()` (:8562) with no `_allow_backtracking`
gate — the flag only gates auto-enabling `complete` mode on slot
conflicts (:9444-9450) and `_slot_operator_trigger_reinstalls` (:2131).
So a satisfiable installed-consumer pin enforces in-pass under
`--backtrack=0`. `--nodeps` is the one exclusion: real pops `recurse`
(`create_depgraph_params.py:181-183`), so its complete graph never runs.

Probe (rule 13, one container run — the brief's explicit question: a pin
that *cannot* be satisfied with backtracking off): staged fixtures +
`FX_WORLD_EXTRA=dev-libs/r25consumer`, one real emerge,
`emerge -p --backtrack=0 -uD =dev-libs/r25lib-2.0`.

CAVEAT 1 (version): the image carried Portage 3.0.81.3, and the
in-container upgrade to the 3.0.82.2 pin failed (fixture-only
`PORTAGE_REPOSITORIES`, no portage ebuild visible). So the behavior
evidence is 3.0.81.3; the mechanism is 3.0.82.2 source (the whole
reporting chain — `_resolve_conflicts` -> `_complete_graph` ->
unsatisfied-dep loop -> `_process_slot_conflicts` ->
`_solve_non_slot_operator_slot_conflicts` incl. the "Record missed
updates" tail at :2089-2110 — carries no `_allow_backtracking` gate on
any link). Slot-conflict reporting is long-stable; risk of a
.81.3-vs-.82.2 difference on this path is minimal.

Answer: real reports the slot-collision block and exits 1 (NOT a
skipped-update warning, NOT silent, NOT an abort) — structurally identical
to what portuale already prints at bt0 on that shape (same `(Argument)`
puller + installed-consumer pin parents, same rc). No product change
needed for the unsatisfiable half; it is pinned by contract instead.

CAVEAT 2 (display nit, residue for the owner): in that residual block,
real renders the consumer's `:=` bound form first, portuale the
normalised `<2.0` form (same two parents, order only; the `(and 1 more
...)` tail agrees on both). Untouched: dropped-pin ordering is
budget-independent and outside this slice. The pin asserts portuale's
order and discloses the nit.

## S1 — port (one rule; `rust/portage-repo/src/lib.rs` only)

1. New `BacktrackFeedback::RevDep` variant (same working copy as
   `Config`, identical inside the search): the reverse-dep scan's
   enforced-pin feedback travels distinctly so the driver can feed it
   back under `--backtrack=0`, where every other feedback kind still
   settles as before.
2. Driver feed loop: at `backtrack_max == 0` (and not `--nodeps`) a
   `RevDep` working copy re-runs the pass in-process instead of
   settling. Not a backtrack retry: `restarts` stays 0 (real
   `backtrack: 0/0`). Bounded by the existing `reverse_dep_masked`
   latch (a pass that adds nothing new falls through to `Settle`).
3. Silence gate in `assemble_result`: at bt0, slot-operator withholds
   synthesize no `SkippedUpdate` rows (real's minimizer/probe path
   writes no `_runtime_pkg_mask` / `_conflict_missed_update`, so
   `_show_missed_update` (:1566) renders nothing — the r25 shape is
   bed-observed silent). Plain-pin withholds still synthesize rows:
   real reaches them via the slot-conflict solver whose record tail
   runs ungated even at bt0 (proven by the #90 S2 two-target cell,
   which prints its notice at `--backtrack=0` on both sides).
   Classification is on the verbatim `raw_atom` (both bound forms of
   one pin carry a slot operator there; the normalised `atom` strips
   the built binding).
4. Doc touch: the `backtrack_max` "disables backtracking entirely"
   comment now names the #209 exception.
5. Rust unit test
   `backtrack_zero_feeds_satisfiable_reverse_dep_pins_in_pass` (r25
   shape, seeded consumer, bt0): no `r25lib` merge, `skipped_updates`
   empty, `restarts` 0, no conflicts, `r25up U 2.0` + `r25target N 1.0`
   intact. Verified non-vacuous (fails on the pre-fix tree). One
   existing test updated to the renamed variant
   (`collect_feedback_keeps_an_enforced_pin_single` now expects
   `RevDep`; same working-copy assertions).

Behavior spot-checks (debug build, ad-hoc ROOTs): r25-bt0 now
`[U] r25up-2.0 + [N] r25target-1.0`, silent, rc 0, restarts 0/0;
default r25 unchanged (rows + warning + restarts 1); bt0 `=r25lib-2.0`
unchanged (residual, rc 1); whpin-reachable bt0 now withholds
`whblocker-2.0` with the notice, rc 0 (matches real's ungated solver
path); slotconflictparent/btparent/two-target/blk0 bt0 cells unchanged.

## S2 — cells + pins (pmtest; no new fixtures — r25 family reused)

- `differential-test-bed/atomlists/l0-fixture-oracle-r25.txt`: new bt0
  cell on the existing list/knob (no runner change).
- `test_oracle_209_backtrack_zero_enforces_a_satisfiable_consumer_pin_in_pass`:
  exact two rows, rc 0, no skip/rebuild text on stdout+stderr, `--json`
  backtrack `{"restarts": 0, "max": 0}`.
- `test_oracle_209_backtrack_zero_reports_an_unsatisfiable_consumer_pin`:
  rc 1 + `_assert_residual_slot_conflict_block` (nit disclosed above).

## Gates (all green)

- `cargo fmt --check` clean; `cargo clippy --release --all-targets`
  zero warnings.
- `cargo test --release` (whole workspace): all suites ok (incl.
  portage-repo 856 lib tests with the new pin).
- pmtest full suite: `2101 passed, 37 skipped, 5 xfailed`, rc 0, no
  corpus drift (nothing blessed, per rule 7).
- No fixture changes, so no md5-cache work; no oracles/allowed lists
  touched.

## BED-PENDING (coordinator)

- `differential-test-bed/run/l0-fixture-oracle-all.sh` from the pmtest
  worktree (covers the new r25-bt0 cell): expect 7/7 green, 0
  unexplained. Would confirm real 3.0.82.2 withholds silently at bt0
  (CAVEAT 1's remaining version gap on the satisfiable half).
- `differential-test-bed/run/l0-resolver.sh` (Track G guard): expect
  identical-to-or-better than the last green L0 row by row; any lost
  row stops the slice.

## Commits (pmtest first)

- pmtest: S2 cell + pins.
- portuale: S1 port (quotes pmtest short sha).
- Entry #209 NOT flipped (brief instruction).
