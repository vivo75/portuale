# SDD ledger — plan: docs/batch-2026-09-29.md

Session: 2026-09-30, coordinator (frontier) executing the batch plan.
Plan is a coordinator/orchestrator plan (§0.11): implementation is dispatched
to opencode `muse-*` agents; this session plans, reviews, allocates numbers,
owns the bed queue, commits and merges. Ledger format follows executing-plans.

## Pre-flight shared interfaces

- Z0 → all tracks: Z0's recorded baselines are the comparison reference
  (§0 baselines block). Z0 step 3 must land before any track's guard runs.
  Found: baselines block already names 09-28 P-Z numbers as "the reference
  for Z0"; Z0 re-takes and replaces them. Clean.
- R1 → R2–R8: changing the default `--backtrack` to 20 moves `/10`
  denominators and can move `get_best_run` rows later slices pin against
  (plan §1.3, B7). Found: R2's pin text already says `backtrack: 3/20`
  "once R1 has landed" — consistent. R1 is serial-first by design.
- C1 → D2: #273's exact probe text moves into #265's entry so D2 has one
  source (§5, §6 D2). Found: D2 S0 already names the three cells including
  the `--autounmask-use=n` one. Clean.
- O1 S0 → Track X Slice B: if the cycle rotation disappears under
  `FX_HOST_ROOTS=1`, #278 folds into Slice B's pin table (§8, §10). Found:
  §10 "O1's fold-in" carries the same rule. Clean.
- Track R merge → Track X: X starts only after R has merged (§1.4); X.0
  re-takes L0 + fixture oracle on that `main`. Found: §10 X.0 says the same.
  Clean.
- B1 beds → all bed steps: B1's `l3-core` runs need beds exclusively
  (§0.7); later tracks' bed steps queue through the coordinator. Found:
  ordering puts B1 alone before the parallel tracks. Clean.

Pre-flight: 6 interface rows, no conflicts; 0 rulings needed at scan time.

## Rulings

- H0 step 2: `git branch -d` refused `backlog/259-260-test-hygiene` (merged
  into `main` as `0891030d`, but the untouched remote-tracking ref made
  `-d` balk). Deleted with `-D`. Plan authorizes local delete only (B3);
  remote left alone. Cost if wrong: one local branch, recoverable from
  `0891030d`/`40598d07`.
- H0 step 4: `plan-reviewer` (the plan's named opus agent) is not a
  registered task type in this session. Dispatched as `general` with the
  plan-reviewer brief verbatim. Cost if wrong: a slightly different reviewer
  persona; the findings list is what matters and it is in §R.
- H0 step 4 corrections: false claims are corrected in the **bodies** of
  `batch-2026-09-29.md`, `_242.md` and #278's backlog entry, not only in
  §R — §R is not in the read-first list, so a body-only errata would be
  invisible to implementers. Cost if wrong: bodies diverge from the draft
  text; §R preserves the review record.
- Review "V1 rows" (R2 S2) is undefined in #269/#253. Ruling: V1 = the
  probe-variant-1 capture of the `mmprov`/`mmcons` shape R2 S0 names (only
  `mmcons` requested). Cost: documentation-only label.
- Review `sct`/`oldc`/`newc`: abbreviations of the `slotconflict*` fixtures.
  Ruling: treat as those fixtures' short names. Cost: one pin name.
- Review M1 commit-message rule: conditional — required only if S1 changes
  how `mrg` writes the saved file (the filter fallback), not for a pure
  scoping fix. Cost: a missing commit sentence, caught at review.
- The superpowers `task-start`/`task-done` scripts are not used: this is a
  coordinator/batch plan with its own brief + report mechanism (§0.11), not
  a writing-plans per-task plan. The ledger format is kept. Cost: less
  structured per-task bookkeeping; the plan's reports + this ledger cover it.
- Cargo workspace root is `rust/` (no `Cargo.toml` at the repo root) —
  `cargo fmt`/`clippy`/`test` run from there. Cost: a wasted smoke-test
  invocation, already spent.

## Task log

Task H0: complete (no product commits; branch `backlog/259-260-test-hygiene`
deleted locally (was 40598d07); next free **#279** confirmed by scanning
`docs/backlog-tasks.md` (max #278); plan review dispatched (7 Important, 6
Minor, 6 declined — all recorded in `batch-2026-09-29.md` §R and the bodies
corrected); agents smoke test green — `muse-verify-pass` started, read
`pmtest/USAGE.AGENTS.md` and `portuale/rust/Cargo.toml`, wrote
`/tmp/opencode/h0-smoke.txt`.)

Task Z0-step1: complete (full gate on `main` @ `c1421d22`, release).
fmt PASS, clippy zero warnings, `cargo test --release` FAIL 1922/2 —
`pretend::tests::ask_read_news_true_spellings_prompt_like_real` and
`pretend::tests::ask_read_news_without_eselect_prints_real_hint`, both
`watchdog: pty child exceeded 60s` at `pretend.rs` `wait_pty_output`. No
product commit since the 09-28 P-Z baseline's 1925/0, so this is a flake
or an environmental finding, not a regression.

Task Z0-fixup: complete (branch `backlog/z0-0929-fixup`, portuale `9a3d39e5`,
merged `--no-ff` as `da4d4882`; no pmtest counterpart — the branch had no
commits and was deleted). **Root cause of the #259 "intermittent race":**
`read_prompt_line` was a raw `libc::read` with no userspace leftover — a
multi-line chunk became ONE answer, and a line still queued in the kernel
was readable by any child spawned with inherited stdin (`eselect`/`renice`/
`ionice`, all `.status()`). Real is protected by Python's buffered
`sys.stdin`. Proven by a forked-grandchild experiment (the grandchild read
`b"No\n"` off the shared fd; the second `read` blocked). Fixed with
`PromptLineBuffer` (one line per call + `drain_available` non-blocking
drain of the kernel into the leftover). TDD: `prompt_line_buffer_returns_exactly_one_line_per_call`
RED→GREEN (`Some("No\nYes\n")` → `Some("No\n")`), `prompt_line_buffer_drains_the_kernel_so_a_child_cannot_steal_the_next_answer`
RED→GREEN (kernel read `b"Yes\n"` → EAGAIN). The #259 watchdog capture is
the evidence it was waiting for. Residual window filed in the commit body
(not a new number — speculative): a child spawned *before* the first prompt
read could still steal the first answer; the three `.status()` helpers are
candidates for `Stdio::null()`. Gate after the fix: fmt PASS, clippy zero
warnings, `cargo test --release` whole workspace green (portuale bin 741/0),
pmtest suite **2215 passed / 0 failed / 37 skipped / 4 xfailed** — equal to
the 09-28 P-Z baseline, no corpus drift.

Task Z0-step2: complete (bed controls on `main` @ `da4d4882`).
`muse-bed-runner` two invocations (oracle+L0 two-at-a-time; l31 then l32
serial/alone). Fixture oracle `l0-fx-20260930T132118Z..133016Z` 13/13
green 0 unexplained. L0 `l0-20260930T133054Z` byte-identical to
`l0-20260930T102811Z` apart from `date_utc` -- no row moved (120/101/
0.842/8/31, the 31 unexplained are standing rows). l31 control
`l31-20260930T134230Z` rc 0 0/0/0; candidate `l31-20260930T134425Z`
3 hard / 3 unexplained = exactly #262's rows (the `declare -- x=""`
saved-`environment` leak on `porttest/{installmask,phases,setuid}-1.0`),
unchanged from `l31-20260930T101720Z`. l32 control `l32-20260930T134555Z`
+ candidate `l32-20260930T135846Z` 0/0 on all 7 cells (C1-C4, F1-F3).
Report `.superpowers/sdd/batch-2026-09-29/z0-controls-report.md`.
Nothing moved vs the 09-28 P-Z references.

Task Z0-step3: complete (docs commit `3fe4d50f` on `main`, portuale only --
the message says pmtest has no counterpart). H0's review corrections ride
here as the plan requires (§R + the body fixes to this plan, `_242.md`
and #278's entry). Z0's guard-reference baselines recorded in §0's
"Z0 baselines" block. Z0 is DONE.

Task C1: complete (portuale docs commit, no pmtest counterpart). #273
flipped to `CLOSED 2026-09-30 — duplicate of #265(b)` (owner B6), original
entry kept as history; #265(b) extended with #273's exact probe text (the
`for <root>/` line and the absent `(dependency required by…)` lines) so D2
has one source. `muse-closeout-scribe` prepared the edit and correctly
declined to commit (its role forbids it); the coordinator committed it
verbatim as the brief authorized.

Task B1-S0: **STOPPED PARTIAL by owner (2026-09-30 ~20:28, "stop it too
much time").** 1 of the 3 planned `l3-core` runs completed before the
stop. **Ruling: B1 is parked** — #261 stays OPEN with the partial
evidence below; S1's tolerance-vs-pin decision (owner B8) is deferred
until there are enough runs to decide, and **no further L3 bed is run in
this batch without a fresh owner ask** (the L3 runs cost ~5h27m each, far
past what the owner wants to spend). Cost if wrong: #261's intermittent
control-noise class stays un-triaged and any future L3 run can show it
again; the comparator's getconf handling is unchanged.

Evidence from the one completed run (`l3-20260930T141623Z`, `L3_JOBS=28`,
`L3_CONTROL=1`, `l3-core.txt`, started 14:16:23Z, report 19:43:21Z —
5h27m wall):
- **control: 17 hard / 1 explained / 16 UNEXPLAINED = 8 CONTENTS + 8
  MISSING** — exactly #261's `getconf` split-debug signature. The split
  **recurred** at `-j28` (first seen `l3-20260928T195717Z`, absent in
  `l3-20260929T082541Z`), so it is confirmed intermittent at `-j28`, not a
  one-off.
- **candidate: 19 hard / 19 explained / 0 UNEXPLAINED** — the portuale
  side is clean; this is real-against-real noise, as #261 says.
- payload diffs 151 / 153, mtime-only 59447 / 59445 (both tolerated).
Run 2 (`l3-20260930T194424Z`, `L3_JOBS=28`) was in control-a (~32 min in)
when the owner stopped it; its container `porttest-l3-portage-3272791` was
SIGKILLed and removed, the `l3-source-parity.sh` processes killed. Run 3
(`L3_JOBS=20`, the discriminator B8 needs) was never started.

Task R1: complete (branch `backlog/263-backtrack-20`; pmtest `c1fab11`,
portuale <this commit's sha> quoting it). #263: default `--backtrack` is
now 20 like real (`DEFAULT_BACKTRACK_MAX` is the single source for the
`run()` initializer and the `--help` line; the old comment claiming real
defaults to 10 corrected). Pins un-masked `N/M` -> `N/20` by name (the
aub0 order texts, the three `"max"` pins, the help pin); no corpus case
records the denominator. TDD: `backtrack_default_matches_real_portage_twenty`
RED->GREEN. portuale bin 742/0. BED-PENDING: L0 re-run.

Task D1: complete (branch `backlog/264-slot-conflict-use-expand`; pmtest
`8132c87`, portuale `c53feb59` quoting it). #264: the slot-conflict
USE= renders real's USE_EXPAND groups (`USE="foo" ELIBC="glibc"`) by
delegating `pkg_use_display_for` to #230's `skipped_update_use_display_for`
(one definition, the same bytes as the skipped-update block). Pins
rebuilt from HEAD + the precise `ELIBC="glibc"` insertion the
2026-09-28-244 real captures show (on `scheduled for merge` lines only,
not the `[ebuild …]` rows), with caret-marker lines re-padded to
`above_len + 1` (real's own rule, measured in those captures).
Coordinator repair during review: the implementer wrote the aub0
literals as raw multi-line strings (SyntaxError) and its first repair
over-merged stdout+stderr into one literal -- rebuilt from HEAD with a
deterministic transform instead. Corpus bless reviewed and riding in
pmtest `8132c87` (contract + expanded: the slot-conflict and
REQUIRED_USE-violation cells). TDD:
`slot_conflict_use_display_renders_use_expand_groups` RED->GREEN. Suite
2215/0/37/4 xfailed = the Z0 baseline. BED-PENDING: fixture-oracle
(slot-conflict cells only).

Ruling (R1+D1 guards): the guards run against the merged `main`, not
per-slice. R1 and D1 both re-anchor the same `aub0` literals (R1 un-masks
the denominator to `/20`, D1 adds `ELIBC="glibc"`), so each slice's guard
is only meaningful in combination; the bed registry resolves one
`PM_REPO` and re-pointing it per slice triples the bed time for no extra
signal. Cost if wrong: a bad merge sits on local `main` for one bed run
(~30 min) and is reverted locally (no push, B3).

Task B1-S1: complete (pmtest `f71de02`, merged `2683540`). #261's filed
resolution landed as `compare/diff.py`'s `HARDLINK_DEBUG_ID =
"#261-hardlink-debug-race"` tolerance (owner B8: tolerance, not an
L3_JOBS pin). TDD `compare/test-diff-tolerance.py` 4/4 (the race
explained + three near-misses stay hard: two .debug for one set, a lone
.debug outside a set, a plain non-debug MISSING). Offline re-diff of the
saved `l3-20260930T141623Z` control pair: hard 17 / explained 17 /
UNEXPLAINED 0 (was 17/1/16) -- all 16 getconf rows named
`(#261-hardlink-debug-race)`, no other row changed class. Live `l3-smoke`
`l3-20261001T010343Z` rc 0, 0 unexplained (the race did not trigger that
run; the offline re-diff is the proof it handles the class).

Guards (all three slices, against the merged `main`):
- L0 `l0-20261001T005535Z` -- **row-by-row identical** to
  `l0-20260930T133054Z` apart from `date_utc` (120/101/0.842/8/31). R1's
  stop condition (a row moving away from real) did not trigger.
- Fixture oracle `l0-fx-20261001T005535Z..005635Z` -- 13/13 green, 0
  unexplained. 12/13 byte-identical; `g216` has one line-order artifact
  inside the same explained cell (no class change). Note: D1's expected
  slot-conflict movement did NOT appear in the oracle -- the aub0
  display cells live in the contract suite (re-pinned + blessed there),
  not in the oracle lists, so nil movement is correct.
- l3 smoke `l3-20261001T010343Z` -- rc 0, hard 1 / explained 1 /
  UNEXPLAINED 0.
Ruling recorded earlier (guards against the merged `main`) held.

Coordinator repair during D1 review: the implementer wrote the aub0
literals as raw multi-line strings (SyntaxError) and its first repair
over-merged stdout+stderr; rebuilt from the pre-slice base with a
deterministic transform (ELIBC insertion + marker re-padding to
`above_len + 1` + `/M` -> `/20`). Merge conflict on those same literals
between R1 and D1 resolved the same way (both transforms).

Wave 2 (after the L3 fast path unblocked things):

Task R1-corpus: complete (pmtest `4d0de7c`). R1's report claimed "no
corpus case records the denominator" from a grep over the corpus *files*
-- but they are lzma-compressed. Read properly: 11 contract + 461
expanded cases carry `"max":10`, 51 contract cases a literal
`(backtrack: N/10)`. All move under R1; reviewed bless of exactly that.
Ruling: the corpus is data inside `.xz`, so a grep over the corpus dir
proves nothing -- always load the JSON.

Task M1: complete (portuale `3ab54bdc`, merged `3b7b8490`; no pmtest half).
#262's `declare -- x=""` is NOT an unscoped variable: `install_file_atomic`'s
SSH arm staged into `TempDir::new(...).keep()` and since #260 that is a
*directory*, so `std::fs::write` failed `Is a directory` on every SSH
install and the vdb kept the build-time `environment.bz2` (which carries
the stray `x=""`). Fix: stage into `server_dir.join("regen.env")`.
Explains the intermittency exactly (09-26 leaked pre-#171 no regen;
09-27/28 clean; 09-29/30 leaked after #260). TDD
`install_file_atomic_ssh_stages_bytes_before_transport` RED->GREEN.
Merge-path note in the commit: the client install is still `mv tmp dest`
(rename, old inode never written). BED-PENDING: l31 0 unexplained, l32
unchanged, glibc+bash merge gate + l3-smoke.

Task R2: complete (portuale `e74455d3`, pmtest `4f6d085`, merged `af183232`
/ `c340a25`; corpus bless `4d0de7c`-follow-up in pmtest). #269: the pair
push and the provider-side `r` are gated on the same-slot arm (real
`new_child_slot is None`), and the new-slot arm ranges over every
*available* package like real's `_slot_operator_update_probe` with
`new_child_slot=True` + `_iter_similar_available`. TDD
`slot_operator_rebuild_scan_newslot_arm_fires_without_a_scheduled_provider`
RED->GREEN; `cargo test -p portage-repo --lib` 932/0. Evidence
`docs/evidence/2026-09-30-269/`. BED-PENDING: L0 + fixture-oracle.

Task D2: complete (portuale `4c52610d`, pmtest `0fba00e`, merged `f81c85bd`
/ `7634ff2`). #265 (a)+(b) with #273: the `missing_use` loop counts
conditional flags toward `Missing IUSE:` (real's `use.required` takes
every no-default token), and the top-level argument-atom miss prints
real's `there are no ebuilds built with USE flags … for <root>/` block
with no chain lines. The dependency arm does not move. TDD 3 new Rust
tests RED->GREEN + 1 re-pinned; 4 pmtest pins (the entry's three cells
plus `test_installed_dependency_use_dep_flag_only_in_built_use_is_kept`,
freshly oracled). Wording note: real prints only `Missing IUSE: foo` for
mia0a-2 -- `Change USE: +bar` was pre-fix portuale's own text. Evidence
`docs/evidence/2026-09-30-265/`. BED-PENDING: fixture-oracle (mia0 /
useflagpkg cells).

Suite on merged `main` after wave 2: **2218 passed / 0 failed / 37 skipped
/ 4 xfailed** (was 2215 at Z0; +3 new pins). One residual corpus entry
(`test_oracle_slotop_conflict_mass_rebuild#0` = R2's intended movement)
reviewed and blessed.

Coordinator repairs in wave 2: D2's first run left its S1 stashed and its
brief's mia0a-2 wording was wrong (the evidence resolved it: real prints
`Missing IUSE: foo` only). R2's worktree showed 3 hard "failures" that
were only R1's un-blessed `max: 10` corpus drift -- re-checked with
BLESS=1 (41/0) before concluding the change was sound.

Wave 3:

Task D3: complete (portuale `e9a7b308`, pmtest `8306ec2`, merged both
mains). #271: `show_merge_list` gains the re-show arm behind
`autounmask_only_reshows_merge_list(pretend, autounmask_only, has_changes)`
-- real's `_display_autounmask` -> `_show_merge_list` -> `display`.
Test renamed `test_autounmask_only_suppresses_the_merge_list` ->
`test_autounmask_only_reshows_the_merge_list` (the old name is in the
commit message). TDD RED->GREEN. portuale bin 744/0. Suite 2218/0/37/4
xfailed. Deliberate cut: non-`--pretend` `--autounmask-only` keeps the
suppression (real re-shows there too, unprobed) -- candidate residue.
Corpus note: the old test name's harvested key is orphaned by the
mandate rename; re-harvest/drop rides in the coordinator's corpus commit.
BED-PENDING: fixture-oracle (useflagpkg / autounmaskkeywordpkg cells).

Task R3: **BLOCKED (owner question filed).** #270's stop condition (B9)
fired: the decline is NOT local to the slot-operator handling. It is the
**ungated general** in-pass slot-conflict solver
(`_solve_non_slot_operator_slot_conflicts`, `depgraph.py:1774-2115`, which
explicitly *excludes* slot-operator conflicts at `:1789-1793`) plus
general request semantics (`_want_installed_pkg` / `_iter_atoms_for_pkg`'s
higher-visible-slot skip). A port needs three non-local changes (the bt0
walk exemption; `direct_solve_arg_mode` + the higher-slot arg semantics;
in-pass `_create_graph` re-run). Gating only the first would flip one
wrong answer for another -- exactly the partial approximation B9 forbids.
No code, no pins, no commit (both worktrees clean). Question filed in
Italian at `~/repo/PORTUALE/morning-questions-2026-10-01.md` (options:
park #270 as researched residue / charter the general-solver port as its
own slice / accept a documented partial). S2's target pin named:
`test_oracle_prune_rebuilds_conflict_missed_updates` (its bt0 control,
`TBD-270` comment).

Task O1: **in progress (LOCAL decision).** The rotation persists under
`FX_HOST_ROOTS=1`, so #278 is a local display choice, not a cross-root
artefact. Real's `circular_dependency_handler` prints `shortest_cycle[0]`
(`_prepare_circular_dep_message`). Uncommitted diff in
`portage-repo/src/{lib,merge_order}.rs` (+230/-78) with
`find_hard_cycles_starts_where_reals_get_cycles_starts`; **missing**: the
`docs/evidence/2026-09-30-278/` captures, the pmtest pins, and the full
gate. Needs a continuation run.

Task B9-decision: **PARKED as a researched residue** (owner, 2026-10-01,
option a). #270 flipped to `PARKED 2026-10-01 … researched residue, not a
local fix` with the full S0 chain (the general solver
`_solve_non_slot_operator_slot_conflicts` + `_iter_atoms_for_pkg`'s
higher-visible-slot skip; the three non-local changes; why the partial is
forbidden). Plan R3 + B9 row updated; the Italian question marked
decided. Docs commit `0bd676ae`. The pin to extend when un-parked is
`test_oracle_prune_rebuilds_conflict_missed_updates`'s `TBD-270` bt0
control.

Bed guards waves 1–2 (compiled from the run reports -- the bed-runner
invocation was interrupted before writing its own report):
- L0 `l0-20261001T040818Z` 120/101/0.842/8/31 -- **identical by name**
  to `l0-20261001T005535Z`; R2 moved nothing.
- Fixture oracle 13 lists `l0-fx-20261001T040818Z..040917Z` -- **13/13,
  UNEXPLAINED 0**; no delta (D2's cells are contract-suite cells).
- **l31 `l31-20261001T041932Z` and `l31-20261001T042840Z`: 0/0/0** -- the
  three #262 `declare -- x=""` rows are GONE. M1 proven on the bed that
  found it.
- l32 `l32-20261001T042936Z` / `l32-20261001T045834Z`: 0/0/0, unchanged.
- l3 smoke `l3-20261001T051513Z`: hard 1 / explained 1 / UNEXPLAINED 0
  (the bash `environment` VDB row, `l3-vm-repo-revisions-vdb-env`);
  glibc+bash re-merged from source on both sides -- the SOURCE half of
  §0.6's gate is green for M1.
Still owed: the consume-reinstall half of the merge-path gate
(`l1-merge-gate.txt`, `L1_CONSUME_REINSTALL=1 L1_SKIP_BUILD=1`) -- the
latest `l1-*` run predates M1. Dispatched.

Task M1-gate: complete. The consume-reinstall half of §0.6's merge-path
gate, `l1-20261001T072430Z` (build pass `l1-20261001T065013Z`): rc 0,
merged_count 2/2 (bash-5.3_p15 + glibc-2.43-r2), files 4006 both sides,
0 hard / 0 explained / 0 unexplained, findings []. Delta vs the last
green gate `l1-20260928T194128Z`: only the non-fatal mtime-only counter
(1417 -> 1415). M1 (#262) is fully proven -- both halves green. #262
flipped DONE; the plan's M1 checkboxes flipped (a second docs commit
`ea4e382e` after a search-string typo missed them the first time).

Task O1: complete (portuale `58eab98b`, pmtest `456d0f4`, merged both
mains). #278: S0 showed the rotation **persists** under `FX_HOST_ROOTS=1`
-- LOCAL, no fold into Track X. Real's `circular_dependency_handler`
prints `shortest_cycle[0]`; portuale's `find_hard_cycles` now starts the
printed ring there (earliest-inserted node of the smallest cycle),
display only -- not the cycle set, not the merge order. TDD
`find_hard_cycles_starts_where_reals_get_cycles_starts` RED->GREEN;
`cargo test --release -p portage-repo --lib` 936/0. Evidence
`docs/evidence/2026-09-30-278/` (four start-node captures) + the new
`u278json` fixture. The #249/#251 cycle pins do not move. The
`test_abort_path_au_cycle_orders_use_block_after_circular` corpus
entries move with the start node -- reviewed bless after the merge.
BED-PENDING: L0 + fixture-oracle (the u249 cycle cells).

Docs: `8e58c285` (#262 DONE + the wave 1-2 bed report) and `ea4e382e`
(M1's plan checkboxes).

Track R wave 2 (R4-R7) + D4 landed 2026-10-01. D4 (#272) portuale
`d26fe15c` + pmtest `0151da2`. R4 (#268) portuale `ea5d57ca` + pmtest
`0f8ab6c` (the one pin file for both cuts of the #252 gate). R5 (#277)
portuale `resolver_trace` + pmtest oldslot fixtures/pins. R6 (#276)
`slot_operator_unsatisfied_probe` backtrack rebuild + u276 fixtures. R7
(#266) `minimize_children` on the candidate's effective USE + r266
fixtures + evidence `docs/evidence/2026-10-01-266/`. All four merged to
both mains. O1's fixture set completed (`3093042` -- the three missing
u278 partners) and its corpus blessed (`748e4ea`).

Environment note: one pmtest run hit `OSError: [Errno 28] No space left
on device` on /tmp (transient -- /tmp has 58G free; the 282
`test_portuale.py` errors in that run are environmental: `unshare` is
unavailable in this container, not a regression). The contract suite
(`test_emerge_pretend_contract.py` + `test_output_invariants.py`) is
green on merged main: 1485 passed / 4 xfailed.

Remaining: R8 (#274), X.0 + X.A-D (#242), P-Z, and the accumulated
BED-PENDING guard set (L0 + fixture-oracle for D3/R2/O1/D4/R4-R7).

**Track R wave 2 (R5-R7) REVERTED 2026-10-01** -- landed in parallel, they
collided in `portage-repo/src/lib.rs` and the merged main showed 119-128
contract failures (including O1's `test_278` and R6's own new pin). The
three merges were reset off both mains (`git reset --hard` to the R4
merge, portuale `7616db35` / pmtest `c4bbf4d`), which is green again
(`portage-repo` 937/0). The slice branches are intact
(`backlog/277-slot-conflict-nodes`, `backlog/276-bt-rebuild`,
`backlog/266-minimize-use`) and must be re-landed **one at a time**,
rebased onto the previous one's merge, with the contract suite green
between each. This is the batch's "max 2 at once" rule being exceeded
(three at once) -- the lesson is recorded here.

D4 (#272) and R4 (#268) remain landed and green.
