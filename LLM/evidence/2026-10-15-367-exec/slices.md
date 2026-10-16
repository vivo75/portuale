# Slices S1+S2 (#367) — work log, 2026-10-15/16

> NOTE (duplicate dispatch): a second session with the identical task brief,
> branch, evidence dir and scratch tag (`x367a`, opencode PID 666236) is
> editing the same files concurrently. The S1 fix, the first S1 test copy and
> all S2 pins below were written by that session, not by me; I verified,
> de-duplicated once, and stopped editing when its 23:57 UTC edit re-broke
> compilation (E0428 duplicate test definitions). Nothing below is committed
> (no commit grant). Sibling run log: `/var/tmp/x367a/baseline.log`.

## S1 — #367 kill + switch fix (portuale repo)

Files:
- `crates/portage-repo/src/solver_bridge.rs` — fix line + 4 tests (see state note)
- `LLM/code-notes/portage-repo/solver_bridge.md` — #367 S1 delta bullet under
  `## fn resolve_pubgrub`; stale S6 "Uncovered" line corrected (plan allows);
  `python3 LLM/tools/code_notes_index.py` re-run.

State of the fix: `provider.set_prefer_newest_slot((deep && req.update) || req.empty);`
(`solver_bridge.rs:3226`), attribution comment
(`// Ported from portage-cli src/query/depgraph/round.rs (MIT, Luca Barbato)`) kept.
Both S0 stop rules are OPEN per `LLM/evidence/2026-10-15-367-s0/SUMMARY.md`
(Portage `-pvuD` = `NS s-2`, `-pvD` = `N top`, `-pvu` = `N top`), so the fix
gate is open and the default-`-pvu` residue stays a strict xfail with
reason `#362` (no new backlog number). Fixtures `pg367-top/s/itop` + md5-cache
+ shared-vdb entries verified present in `pmtest/fixtures` (untracked, kept).

Tests (`mod tests`, Portage-grounded docstrings citing the S0 cap files):
`pg367_top_pubgrub_pvud_takes_the_newest_slot`,
`pg367_top_pubgrub_pvd_keeps_the_installed_slot`,
`pg367_top_pubgrub_pvu_keeps_the_installed_slot`,
`pg367_top_pubgrub_pve_takes_the_newest_slot`.
My own duplicate insertion (a `pg367_request` helper copy) was removed again;
two `/// // Portage:` doc-comment warts removed (citations live in the note).

Commands run and results:
- Failing-first (fix line hand-reverted, no git ops): `cargo test --release
  -p portage-repo pg367` → 3 passed, 1 failed —
  `pvd_keeps_the_installed_slot` got `["dev-libs/pg367-s-2:2",
  "dev-libs/pg367-top-1:0"]`, the predicted pre-fix divergence. Fix restored →
  4/4 pass.
- `cargo mutants --in-place -f crates/portage-repo/src/solver_bridge.rs
  --re 'in pubgrub_provider' --timeout 300` → 10 mutants listed (plan said 9;
  the new `&&` adds `3226:43`), **10/10 caught**, source sha256 unchanged
  (`03649334…`). Full log: `s1-mutants-pubgrub-provider.txt` (this dir).
- `cargo fmt --check` → clean (at that tree state).
- `cargo build --release` → ok; live 12-cell probe of the release binary
  (fixture env, `--getbinpkg=n --usepkg=n` on pubgrub cells) matches every S0
  Portage row: pubgrub `-pv`/`-pvu`/`-pvD` = `N top`,
  `-pvuD`/`-pve`/`-pvuDN` = `NS s-2 + N top`; default same except `-pvu` =
  `NS s-2 + N top` (the #362 divergence).
- `cargo clippy --release --all-targets` → output contained 1 line matching
  "warning" — NOT resolved/identified (tree changed under me right after).

## S2 — #367 pins (pmtest repo; written by the sibling session, verified by me)

File: `pytests-contract-suite/test_emerge_pretend_contract.py` (+155 lines):
- 6 `CASES` entries (default solver, rc 0, next to the pg34 lines).
- 6 pubgrub pins `test_pg367_top_pubgrub_pv/pvu/pvd/pvud/pve/pvudn_*` via
  `_pg34_run`, docstrings cite the S0 cap files; expected rows equal the
  Portage rows (cross-checked against my live probe above, same order).
- 5 default-solver lock pins (all modes where default matches Portage) +
  `@pytest.mark.xfail(strict=True, reason="#362")`
  `test_pg367_top_default_pvu_plans_parent_only` pinned to Portage's `N top`.
- Deviation noted, accepted: default pins pass `--getbinpkg=n --usepkg=n`
  (pg34 precedent omits them; output-neutral here, identical to the S0 argv).
- No corpus change in S2 (no new fixture files) → no drift source, no bless.

Suite results: sibling's full run (`--basetemp=/var/tmp/x367a-baseline`,
started 23:45 UTC, i.e. BEFORE the pins were added at 23:51) → **3889 passed,
37 skipped, 3 xfailed**, identical to the pre-edit baseline. That run therefore
does NOT cover the 12 new pins + 6 CASES entries; a post-pin full run is still
required (compare by test NAME; the xfail must count as xfailed).

## Left undone and why (BLOCKED)

1. At 23:57 UTC the sibling added a second copy of the 4 S1 tests (new
   `pg367_s1_request` helper) → `solver_bridge.rs` currently does NOT compile
   (E0428 ×4, seen in `cargo test --release`). Reconcile to ONE test copy
   (either fold is plan-conformant) before any gate; I stopped editing to avoid
   an edit war.
2. After reconcile, still required: `cargo fmt --check`, `cargo clippy
   --release --all-targets` (0 warnings, incl. identifying the 1 warning line
   seen above), `cargo test --release` (workspace), then in `../pmtest`
   `python3 -m pytest pytests-contract-suite -q --basetemp=/var/tmp/x367a-pt`
   with by-name comparison (expect baseline + 12 pins + 6 CASES params, xfail
   as xfailed). Bed scripts: none required by S1/S2 (plan S4 owns the two
   pubgrub L0 modes; not run here).
3. Scratch: `/var/tmp/x367a-pt` is mine (delete with `sudo rm -rf`, root-owned
   files); `/var/tmp/x367a`, `/var/tmp/x367a-baseline` belong to the sibling
   session — left alone.

## Surprises

- The brief's "tree has no product changes" was stale on arrival: the S1 fix
  and first S1 test copy were already in the tree (sibling's work).
- The `:*` spelling was not re-captured (per S0 notes); pins use the bare dep.
- Duplicate dispatch of this exact task is the reason S1/S2 cannot be closed
  out green in this session despite all substance being present and verified.

## Close-out (post-gate, 2026-10-16 ~01:15 UTC — second session)

The two BLOCKED items above are resolved; nothing below rewrites the log above.

1. **E0428 reconcile: done, single copy kept.** `solver_bridge.rs` holds ONE
   copy of the 4 S1 tests (the `pg34_s4_request` fold; the `pg367_request`
   helper copy is gone) plus the fix line
   `set_prefer_newest_slot((deep && req.update) || req.empty)` (:3226).
   `md5sum` stable; no `mutants.out` left in the tree.
2. **Full verification green (gate.sh run `367`, finished 01:13 UTC):**
   - `cargo fmt --check` → rc=0.
   - `cargo clippy --release --all-targets` → `Finished`, zero
     warning/error lines (the 1 "warning" line seen mid-work was captured
     subprocess text in the interim tree, not a lint; the suspect
     `helpers::xpak/doins` tests were re-run directly: ok).
   - `cargo test --release` (whole workspace) → every `test result:` line
     `ok` with 0 failed.
   - pmtest `python3 -m pytest pytests-contract-suite -q
     --basetemp=/var/tmp/gate-367 -p no:cacheprovider` →
     **3906 passed, 37 skipped, 4 xfailed** (517 s). Baseline (pre-edit):
     3889 passed, 37 skipped, 3 xfailed. Delta = +17 passed / +1 xfailed:
     exactly the S2 additions (12 pin tests + 6 CASES params; the strict
     xfail `test_pg367_top_default_pvu_plans_parent_only` counts as
     xfailed, proving the #362 divergence is real). Skipped count
     identical; no failures by name or count. (Baseline log
     `/var/tmp/x367a/baseline.log` was removed by gate.sh's own
     `sudo rm -rf`; totals are per the task brief.)
   - S2 subset re-run by me (`-k pg367`, `--basetemp=/var/tmp/x367a-pt`):
     17 passed, 1 xfailed. `--depclean/prune/installed` subset:
     250 passed, 1 xfailed — the shared-vdb additions cause no corpus
     drift failure, so nothing to bless.
3. **Bed scripts:** none run — none required by S1/S2 (plan S4 owns the two
   pubgrub L0 modes).
4. **Scratch:** `/var/tmp/x367a-pt` (mine) deleted with `sudo rm -rf`
   (root-owned files); `/var/tmp/x367a`, `/var/tmp/x367a-baseline` removed
   by gate.sh; `/var/tmp/x367a-final` belongs to the sibling session, left.
5. **Final tree (no commit grant; nothing committed, nothing added):**
   portuale: `M crates/portage-repo/src/solver_bridge.rs` (fix + 4 tests),
   `M LLM/code-notes/portage-repo/solver_bridge.md` (S1 bullets),
   `?? LLM/evidence/2026-10-15-367-exec/` (`s1-mutants…txt` + this file);
   pmtest: `M pytests-contract-suite/test_emerge_pretend_contract.py`
   (+155: 6 CASES + 12 pins), `??` S0 fixtures `pg367-top/s/itop` (+md5 +
   shared vdb, kept) plus other plans' `pg359-*` paths (untouched).
   S3–S5 (mutants triage beyond the S1 `--re`, bed, docs) are out of scope
   for this S1+S2 task.

## Corroboration (sibling session -- the "first" worker, 2026-10-16 ~01:30 UTC)

The E0428 reconcile in close-out item 1 was my `sed -i '4838,4942d'`
deletion of my own duplicate block (the `pg367_s1_request` helper + 4 tests);
the peer's `pg34_s4_request`-fold copy is the one kept. Independent
verification of the reconciled tree, all in the foreground:

- `cargo fmt --check` -> rc=0. `cargo clippy --release --all-targets` ->
  `Finished`, no warning lines (corroborates the close-out lint verdict).
- `cargo test --release -p portage-repo pg367` -> 4/4 pass.
- Full pmtest suite, separate run
  (`--basetemp=/var/tmp/x367a-final`, `-rf`, 522 s) ->
  **3906 passed, 37 skipped, 4 xfailed** -- identical totals to the
  gate-367 run (517 s). Baseline pre-edit: 3889/37/3. Delta +17/+1 with
  0 failures by name: the S2 additions, strict xfail counting as xfailed.
  No corpus drift failure -> no bless. Scratch `/var/tmp/x367a-final`
  deleted afterwards with `sudo rm -rf` (root-owned merge-test residue);
  `/var/tmp` now holds no `x367a*`/`gate-367` tag.
- Per-crate Rust coverage (whole-workspace equivalent): `portage-repo`
  1077 passed; `portuale` 1056 passed; every other member `ok`, 0 failed.
- Two invocation notes, both resolved, neither a product issue:
  (a) a bare `cargo test --release` (default 41 threads) did not finish
  inside a 60-min tool timeout -- it ran while the peer's gate.sh was
  concurrently building/testing the same tree, so contention (target-dir
  lock + a pytest-triggered rebuild) is the likely cause; the
  single-threaded `cargo test --release -p portuale -- --test-threads=1`
  completes green in ~267 s;
  (b) one apparent failure
  (`merge_binpkg_regenerates_a_curated_merge_time_vdb_environment`) came
  from running the test binary directly (missing `CARGO_MANIFEST_DIR`);
  via `cargo test` it passes in 0.18 s.
- Pre-fix failing-first for `pvd_keeps_the_installed_slot` is established
  by the S0 capture, not a revert: pre-fix pubgrub `-pvD` is
  `NS s-2 + N top` (`cap-top-pvD-pubgrub.txt`) vs the test's `N top` only.

Final tree matches the close-out inventory above; nothing committed
(no grant), no branches created, no other plans' fixtures touched.

## S3 — #370 scoped mutation pass (portuale repo, 2026-10-16)

Preconditions (plan S3, all met): portuale tree clean (`git diff --quiet`,
HEAD `8bb24c01` = committed S1); no pmtest suite, bed, or other cargo
process running (only my own `oc-x367b` session scope; the leftover
`oc-s0-367` scope holds infra only, no builders); sha256 of
`crates/portage-repo/src/solver_bridge.rs` before = after the scoped run
(`03649334…`, == S1 committed state); backup copy kept at
`/var/tmp/x367b/solver_bridge.rs.bak` in case a SIGTERM-mangled tree had
needed a non-git restore (not needed). cargo-mutants 27.1.0. `--list`
first: **41 mutants** (19+3+4+8+7, exactly the plan's count; far below the
200 stop line). Full list: `s3-list.txt`; command + provenance: `s3-cmd.txt`.

Files:
- `crates/portage-repo/src/solver_bridge.rs` — +189 lines, test-only
  (0 deletions): 3 new tests (see below). No product line touched.
- `LLM/code-notes/portage-repo/solver_bridge.md` — new `## #370 mutants`
  section (plan Q7); `python3 LLM/tools/code_notes_index.py` re-run.
- This dir: `s3-cmd.txt`, `s3-list.txt`, `s3-run.txt`, `s3-caught.txt`
  (24), `s3-missed.txt` (13), `s3-timeout.txt` (2), `s3-unviable.txt` (2),
  `s3-rerun-targeted.txt`, `s3-rerun-or-branch-standing.txt`,
  `s3-rerun-walk-plain-children.txt`, `s3-rerun-simulate-walk-order.txt`.
- pmtest: untouched (only the other plans' pre-existing untracked
  `pg359-*` fixture paths, left alone). No pmtest file added, so no
  pmtest commit pairs with this slice.

Scoped run (foreground, `timeout 3600`, `--timeout 300`):
`41 mutants tested in 15m: 13 missed, 24 caught, 2 unviable, 2 timeouts`
(exit 3 = missed found, expected). The 15 min (vs ~5 predicted) is the
two 300 s timeouts. Missed: 5× `or_branch_standing` (2507:53 `&&→||`,
2507:46 `==→!=`, 2523:52 `<→<=`, 2524:18 `delete !`, 2566:16 `delete !`),
3× `walk_plain_children` (2666:27, 2681:28 `&&→||`, 2681:67 `!=→==`),
3× `simulate_walk_order` (2768:61 `&&→||`, 2785:22 `+=→-=`/`*=`) and
2× `compute_or_overrides` (2928:52 `||→&&`, 2928:34 `!=→==`).
Timeouts (= caught per plan Q8): `walk_add` 2716:34 `&&→||` and
`simulate_walk_order` 2751:32 `delete !`. Unviable: `or_branch_standing`
2480:9 match-arm deletion and 2566:13 let-chain `&&→||` (do not build).

Triage (Q5: record each claim, no cap needed — 4/13 equivalent, not
equivalence-heavy, no flag; continue, never stop to ask):
- Killable → killed (9, each failing-first verified): one direct
  `or_branch_standing` test with two calls
  (`s6_or_branch_standing_weighs_taken_edges_and_unreached_members`:
  self-edged + non-root does not qualify, killing 2507:53; other-parent
  edge + empty walk qualifies, killing 2507:46 and the fallback-arm
  2524:18; docstring cites Portage `depgraph.py:778-793`, never portuale
  output); one direct `walk_plain_children` test
  (`s6_walk_plain_children_collects_only_active_plain_atoms`, "internal,
  no oracle": inactive/blocker/`||`/USE-off dropped, `!flag?` negated —
  kills 2666:27, 2681:28, 2681:67); one direct `simulate_walk_order`
  test over hand-built views
  (`s6_simulate_walk_order_steps_roots_then_frames`, "internal, no
  oracle": exact `added={m:0,a:2,b:3}` / `resolved={(m-1,1,1):1}` with an
  inactive group sharing the class — kills 2768:61 via the shifted extra
  resolution, and 2785 `-=`/`*=` via the collapsed step counts).
- Equivalent (one-line reason each, in the code note): 2523:52 `<→<=`
  (one counter hands out every add/resolve step, equality never
  happens); 2566:16 (inner record lookup repeats the earlier same-query
  slot lookup, empty `uds` is vacuously satisfied); 2928:34 + 2928:52
  (trace-gate only, inserted facts identical).
- Uncovered: none. No residue number needed (nothing to file; backlog
  files untouched per the brief).
- Timeouts inspected, not pinned: 2716:34 hangs (every in-plan
  `walk_add` re-pushes, the stack never drains — the plan predicted a
  kill, the mechanism is a hang instead, still caught); 2751:32 is the
  predicted instant spin.

Verification of the kills: targeted `--re`
`2507:(46|53)|2524:18|2666:27|2681:(28|67)|2768:61|2785:22` → **9/9
caught** (`s3-rerun-targeted.txt`); per-function `--re`: or_branch_standing
19 → 15 caught / 2 missed-equivalent / 2 unviable; walk_plain_children 3
→ 3 caught; simulate_walk_order 8 → 7 caught + 1 timeout, **0 missed**.
`mutants.out/` (and the rotated `mutants.out.old/`) removed.

Gate (slice gate only; pmtest suite left to the coordinator):
- `cargo fmt --check` → clean (one `cargo fmt` reflow of the new tests).
- `cargo clippy --release --all-targets` → `Finished`, zero
  warning/error lines.
- `cargo test --release -p portage-repo` → **1080 passed, 0 failed**
  (1077 pre-S3 + the 3 new tests).

Surprises:
- The plan predicted 2768:64 `delete !` caught and 2847:12 `delete !`
  caught — both held (in the 24 caught, not in missed).
- The plan predicted 2785:22 `+=→*=` equivalent and `+=→-=` a possible
  panic; both were MISSED (no panic: the step is ≥1 at every resolve
  insert, so no underflow) and both turned out killable via exact step
  counts — the equivalence argument I first sketched (uniform shift)
  fails once a multi-group frame lands between an add and a later
  resolve, so a test was written instead of a claim.
- The plan predicted 2716:34 `&&→||` killable ("a re-add moves the step
  later"); it actually hangs the simulation (re-push loop) and counts as
  caught via timeout instead.
- cargo-mutants 27.1.0 rotates the previous output to `mutants.out.old/`
  instead of overwriting — both dirs were removed.

Left undone and why: nothing for S3. S4 (gates + 2 pubgrub bed modes)
and S5 (docs) belong to other slices; the pmtest suite run belongs to
the coordinator. Nothing committed (no grant).

## S4 note + S5 — docs and close-out (portuale + pmtest docs, 2026-10-16)

Arrival state (differs from the brief in one point): portuale HEAD is
already `7a6eb2cf` (the S3 commit, "pin the walk-order functions with
nine new tests (#370)"), i.e. S3 is committed, not "in the working
tree". The portuale tree was clean on arrival, so `solver_bridge.rs`
needed no touch. pmtest HEAD is `7606111` (the S0+S2 commit named in
the brief); its only untracked paths are other plans' `pg359-*`
fixtures, left alone.

Files (portuale):
- `LLM/what-this-proves.md` — one appended paragraph (#367 DONE,
  `-pvD --solver=pubgrub dev-libs/pg367-top`), with the live output
  pasted below and the binary provenance stated honestly.
- `LLM/backlog-tasks-2026-10.md` — new `## Tier 4` section (per batch
  plan G2, "Tier 4 (da creare)"): #367 and #370 rewritten as
  `DONE 2026-10-16` with branch + commit shas (portuale `8bb24c01`
  S1 / `7a6eb2cf` S3, pmtest `7606111` S0+S2; original entries kept
  after "Original entry follows"), moved out from under
  `## Deliberate cuts` (old lines deleted; #363–#366, #368, #369 stay).
  Residues filed after a cross-tier scan (`grep -nE '^[0-9]+\. \*\*'`
  over `LLM/backlog-tasks*.md`: max was #370, no dupes; 371–373 left
  free for 02.346): **#374** the two pubgrub L0 bed modes not run (no
  image), **#375** `--update-if-installed` accepted but ignored (plan
  G7; the S1 code note already promised its number at S5). The
  default-solver `-pvu` new-slot divergence takes no new number (same
  class as #362, pinned as strict xfail `reason="#362"` in S2).
- `LLM/code-notes/portage-repo/solver_bridge.md` — the three S5
  bullets were verified, not rewritten: `## fn resolve_pubgrub`
  already carries the #367 delta with all three Portage citations
  (`depgraph.py:8452-8457`, `:7304-7334`, `:7859-7865`); `## S6
  mutants` "Uncovered" line already corrected by S1; `## #370 mutants`
  (added by the S3 worker) is coherent with this file's S3 section
  (41 listed, 13 missed / 24 caught / 2 unviable / 2 timeouts; 9
  killed, 4 equivalent, none uncovered). Only edit: the dangling
  "residue, numbered at S5" pointer now reads "#375". Then
  `python3 LLM/tools/code_notes_index.py` (rc=0, index unchanged).
- `LLM/04.367-pubgrub-mutants.opus.md` — Status header flipped to
  `done 2026-10-16` with slice range S0–S5, branch and commits (the
  only plan edit; AGENTS.md rule 11).

Files (pmtest):
- `differential-test-bed/findings/l0-pubgrub.md` — appended the dated
  S4 entry: #367 changes the switch value only for deep-without-update,
  both bed modes have update=true so counted categories are unchanged
  by construction, the run itself is NOT done (no bed image) and is
  left for the owner (#374).

Commands run and results:
- Live example (no build, per brief; existing release binary
  `target/release/portuale`, mtime 2026-10-16 11:00 UTC — after the S1
  fix commit 01:49, while the later S3 commit 11:50 adds unit tests
  only, so the binary matches HEAD behaviorally; the output itself
  confirms the fix is in):
  `PORTAGE_CONFIGROOT=$F ROOT=$F PORTAGE_RUNNING_ROOT=$F
  DISTDIR=$F/distfiles target/release/emerge --solver=pubgrub -pvD
  --getbinpkg=n --usepkg=n dev-libs/pg367-top` (`F=../pmtest/fixtures`)
  → rc=0,
  `[ebuild  N     ] dev-libs/pg367-top-1::testrepo to …/fixtures`,
  `Total: 1 package (1 new), Size of downloads: 0 KiB` —
  Portage's S0 row for the same argv (`cap-top-pvD-portage.txt`).
- `python3 LLM/tools/code_notes_index.py` → rc=0.
- `graft build` → rc=0 (8706 nodes, 18774 edges, 538 cards).
- Number-freedom scans (two greps above) → max #370 pre-filing, no
  duplicate numbers after filing (#374, #375 new, in-block).
- No `cargo` / `pytest` / bed runs in this slice: docs-only, no product
  file touched, and the brief forbids builds and bed runs. S1/S2/S3
  gates stay as recorded above in this file.

Surprises:
- The brief's "S3 is in the working tree" was stale on arrival: S3 was
  already committed (`7a6eb2cf`), so per the brief's "S3 commit (see
  git log) placeholder if unknown" the #370 DONE entry cites the real
  sha instead of a placeholder.
- The brief's S5(b) residue list names only the bed run; #375
  (`--update-if-installed`) was additionally filed because the plan
  cuts section demands a real number for it and the S1 code note
  already promised "numbered at S5" — leaving it unnumbered would
  dangle. Flagged for the coordinator in case a single-residue reading
  was intended (deleting #375 is one line).

Left undone and why:
- Commits (both repos): no grant — the coordinator reviews and commits.
  Portuale tree: the 4 files above modified, nothing else (no
  `solver_bridge.rs` touch, no `git add`). pmtest tree: only
  `findings/l0-pubgrub.md` modified, plus the pre-existing other-plan
  `pg359-*` untracked fixtures (untouched).
- The #374 bed run itself: no `test-portuale:latest` image on this
  host — left for the owner, by design.
- Scratch: none created (no `/var/tmp/<tag>` used; the live run reads
  the committed fixture tree and the binary only, writing nothing).
