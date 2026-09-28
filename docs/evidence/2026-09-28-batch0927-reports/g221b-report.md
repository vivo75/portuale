# g221b report — round 2 for #221 (reconcile with #216, review fixes)

Worktree pair: `/home/vivo/repo/PORTUALE/wt-221-circular-choices/{portuale,pmtest}`,
branch `backlog/221-circular-choices` in both. Entry #221 NOT flipped (per brief).
Rule 13 observed: no container beds run; real grounding is via
`ResolverPlayground` (container-free) plus one source-read label proof.
No bless (`PORTUALE_CORPUS_BLESS=1` is the coordinator's call).

## 1. Merge + reconciliation (review items 1–6)

Merged `main` into the branch in both repos, pmtest first.
pmtest merge: one textual conflict (CASES rows: ours `ccd1c` + main's four
`g216*` rows) — kept both. portuale merge: 7 hunks in
`rust/portage-repo/src/lib.rs`, all resolved per the review:

1. **One field, structured children.** Deleted #216's
   `BacktrackParams::circular_dependency: HashMap<(Cp), Vec<String>>`;
   kept #221's `Vec<CircularDepChild>` (slot-aware `atom.match(child)`
   fidelity for `:slot` atoms). Migrated #216's `cycle_restartable`
   record site to construct `CircularDepChild`s (category/package/version
   from `split_cpv`, slot/sub-slot from the merge-bound entry behind the
   child cpv), with an explicit empty-version skip (M1).
2. **One parameter.** `disjunction_preference` keeps HEAD's
   `circular_dependency: &HashMap<(Cp), Vec<CircularDepChild>>` (plus
   HEAD's `circular_self` narrowing, which the review thought missing
   but is present). Renamed #216's `circular_dep` params to the unified
   name where they survive (test helper).
3. **One feedback variant: `Circular`.** The driver treats
   `Config|RevDep|Circular` identically at both match sites
   (budget-free re-pass, `(... , 0)` mask-step; identical bt0 settle),
   so #221's restart arm now emits `BacktrackFeedback::Circular` like
   #216's arm. Budget-free + `--backtrack=0` settle preserved for both
   triggers (all four bt0 pins green).
4. **Both triggers kept.** `cycle_restartable` (fresh single-node
   self-loops) + `find_restart_cycles` (multi-node rings) coexist;
   `find_restart_cycles` still skips self-edges and `cycle_restartable`
   still gates to fresh self-loops, so the triggers are disjoint by
   construction. `find_hard_cycles` already carries both #216's
   `owner_self_loop` and #221's `circular` param (auto-merged); its
   remaining 3-arg callers (one production, four test) now pass the map.
   #193's `find_virtual_cycles` passes `&HashMap::new()` (pre-map
   behaviour, pins byte-identical).
5. **Installed walk sites: empty at both, structurally.** Both
   now-unused map params were REMOVED (not just emptied), so a
   re-threading needs a signature change:
   - `collect_unwalked_installed_blockers`: the scan processes only
     installed packages whose cp was never walked, while both record
     sites key by merge-bound cycle-node cps (every merge-bound cp is
     walked) — the lookup provably always misses.
   - `enqueue_dependencies`: its parents are `AlreadyInstalled` entries
     only (call gated on that outcome; merge-bound upgrades go through
     the main loop), while recorded nodes are merge-bound-only on both
     sides — real records serialize-digraph nodes that stranded on
     unmet buildtime deps (an installed node is always a leaf there),
     keyed by node identity (`Package.__hash__ = Task.__hash__`,
     `_emerge/Package.py:27`, no `__eq__` override). Real's lookup for
     such a parent therefore always misses; portuale's cp-keying would
     over-demote on an installed-v1/merging-v2 cp collision real never
     exhibits. So #216's live-threading was the over-eager shipping and
     #221's empty map was faithful at both sites. No distinguishing test
     can exist with correct record sites (any test passes under either
     map); the structural removal plus this proof is the decision
     record. A `--deep` installed-collision fixture (the only shape that
     could exercise site 2 at all) is left as follow-up below.
6. **Re-verified post-merge.** All #216 pins (`g216top`, `g216comp`,
   both bt0 cells) and all #221 pins (ccd mergelists + bt0 cells) green
   on the merged tree (focused run: 7 passed), plus the two new I3 tests
   and the M3 tree pin (see below).

## 2. C2/I1 — `backtrack.restarts 0→1` drift, execution-grounded per family (NO bless)

Re-listed after the merge: the same 22 expanded `--pretend --json`
cells (no contract drift). A field-level diff of stored vs fresh JSON
for all 22 (`/tmp/opencode/g221b/drift_diff.py`) shows rc identical and
exactly one changed field per cell: `.backtrack.restarts: 0 -> 1`.

Real's retry count per family, run on playground replicas of the
fixture rings (parsed verbatim from the fixture ebuilds; KEYWORDS
rewritten amd64→x86 for the x86-only playground profile; no installed
packages / package.use / world — verified absent for these families;
`ResolverPlayground(debug=True)`, counting `backtracking try N` lines
== final `backtracked`):
`/tmp/opencode/g221b/probe_restarts.py` (+`.log`).

| family | probe cell | real (fail unless noted) | portuale `--pretend --json` |
|---|---|---|---|
| abort-* | abort-cycle-mid | fail, retries=1 | rc 1, restarts=1 |
| abort-* | abort-cycle-last | fail, retries=1 | rc 1, restarts=1 |
| cyc4* | cyc4a | fail, retries=1 | rc 1, restarts=1 |
| fucycle* | fucyclea | fail, retries=1 | rc 1, restarts=1 |
| fucycle* | fucycleb | fail, retries=1 | rc 1, restarts=1 |
| fucycle* | fucyclec | masked, retries=0 | masked rc 1, no JSON (no counter on either side) |
| gpcycle* | gpcyclec | fail, retries=1 | rc 1, restarts=1 |
| hardcycle* | hardcyclea | fail, retries=1 | rc 1, restarts=1 |
| rdrcyc* | rdrcyc (plain) | fail, retries=1 | rc 1, restarts=1 |
| rdrcyc* | rdrcyc --root-deps | (success cell, no retry either side; portuale rc 0 restarts 0 — not a drift cell) | — |
| usecycle* | usecyclea | fail, retries=1 | rc 1, restarts=1 |
| ccd (ref) | ccd1c/ccd4a/ccd5a | success + real mergelists, retries=1 | pins green |

Every probed family: real retries exactly once — even unbreakable rings
(real records + restarts, then `unsolved_cycle` settles; the counter
still increments, `depgraph.py:12192–12240`). Portuale matches exactly.
The drift is correct new behaviour, grounded per cell, NOT a bug — but
per the brief it is NOT blessed here; the table above is the
coordinator's bless input.

## 3. I3 — unsolved-with-backtracking-ON leg: FIXED with fixture + tests

New fixtures (pmtest commit): `dev-libs/sbrA-1.0` (`BDEPEND`+`RDEPEND`
on `sbrB`) + `dev-libs/sbrB-1.0` (`BDEPEND` on `sbrA`), EAPI 8, real
md5-cache entries. The dual edge softens the hard reporter's arm to
`(true, true)` so `find_hard_cycles` stays empty, but the walk still
strands and the trigger still fires; the retry re-strands identically
(no `||` to switch), the ring is unsolved, and the settled pass reports
it through `assemble_result`'s persisting-ring leg (hard report empty,
map non-empty) — rc 1 WITH backtracking on.
Real grounding (`/tmp/opencode/g221b/probe_sbr.py`, same harness):
backtracking on fails after exactly 1 retry; `--backtrack=0` fails with
none. The `(buildtime)` label on the dual edge is source-grounded:
real's message prints `priorities[-1]` (`circular_dependency.py`), the
list is `bisect.insort`-sorted (`digraph.py:add`), and
`DepPriority.__int__` ranks buildtime (-1) above runtime (-3).
Tests: unit `softened_buildtime_edge_hides_from_hard_cycles_but_not_restart_cycles`
(hard empty / restart sees the ring); fixture
`softened_ring_reports_the_persisting_cycle_with_backtracking_on`
(backtracking ON: not `Complete`, `circular_deps == [[sbrA-1.0,
sbrB-1.0]]`; bt0: non-empty too); pmtest CASES row (rc 1) +
`test_softened_build_time_cycle_reports_the_persisting_ring` (exact
stdout/stderr pin). The review's `assemble_result` claim is now covered
instead of dropped.

## 4. I2 / I4 / I5 / M1–M3

- **I2 (`virt_parent` half, `dep_check.py:673–678`): NOT fixed — residue.**
  Real sets `virt_parent` when expanding a virtual's own RDEPEND
  (`dep_check.py:236–252`) and chains both keys at `:673–678`.
  Portuale has no virtual-parent provenance in the walk (QueueItem /
  entries carry the direct owner only); threading it is a
  walk-architecture change, not small. pg5 (the virtual case) passes
  without it — the parent half suffices there (real mergelist matched).
- **I4 (suppression installed-exemption version/slot-only,
  `merge_order.rs`): NOT fixed — residue.** Threading `Config` for the
  full `atom_matches_installed` rule cascades through
  `suppressed_alt_edges` → `kept/resolved_*` → `find_hard_cycles` and
  every display caller: not small. Corner (USE-mismatched installed
  instance + cycle re-opening the phantom edge) reached by no fixture.
- **I5 (flat-list blocker disposition): FIXED (display half).**
  `trailing/collect/count_blocker_lines` now take the settling map like
  `--tree` already did (threaded from `&result.circular_dependency`;
  `--resume --pretend` passes empty — no resolve ran there). Full suite
  green = no regression; the demoted-branch + Replacement-rows corner
  is still fixture-unreached (follow-up: a cycle+blocker fixture). The
  walk-site half resolves to empty/structural per §1 item 5.
- **M1 (empty-version `circular_child_candidate`): FIXED.** Both record
  sites skip empty versions explicitly instead of recording dead edges.
- **M2 (all-demoted fallback citation): FIXED.** `merge_order.rs` now
  cites `dep_check.py:392–401` (bins, `other` last) + `:804–808`
  (first-`all_available`-choice selection), with an inline residue note:
  the in-bin promotion (`:738–802`) also runs over `other`, so two
  demoted branches at different versions could order non-first-listed
  while suppression keeps first-listed — no fixture shapes it.
- **M3 (`--tree` pin on a post-backtrack cell): FIXED.** New CASES row
  (`--tree dev-libs/ccd4b`, rc 0) +
  `test_ccd4b_tree_follows_the_cycle_breaking_branch` pinning the exact
  tree (`ccd4b → ccd4a → ccd4c`; order grounded in the g221 S0 probe:
  requesting `pypy-exe` merges `[exe-bin, pypy, exe]`).

## 5. Gates

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
  zero warnings.
- `cargo test --release` (whole workspace):
  `/tmp/opencode/g221b/cargo-test-final.log`: 1864 passed, 0 failed
  (portage-repo lib 894 incl. the 2 new I3 tests + all migrated #216
  tests; portuale bin 722; all other crates green).
- pmtest full suite, fresh private basetemp
  (`--basetemp=/var/tmp/pmtest-g221b-final -p no:cacheprovider`,
  `/tmp/opencode/g221b/pmtest-final.log`): 2157 passed, 37 skipped,
  4 xfailed, 0 failed (incl. the 2 new pins + 2 new CASES rows and all
  #216 cells).
- Corpus drift: the 22 restarts-only items of §2, NOT blessed.
- Residual risk notes: none beyond the residues in §4.

## 6. Commits (paired, pmtest first)

- pmtest `144a765` (`contract: reconcile #221 with #216 + I3/M3 pins
  (sbr, ccd4b-tree)`; merge `72a7bc8` below it): merge main + sbr
  fixtures/md5-cache + CASES rows (sbrA, ccd4b --tree) + 2 pin tests.
- portuale `b191765a` (`resolver: reconcile #221 circular backtrack
  with #216 (merge main)`, quotes pmtest `144a765`): merge main (2nd
  parent `738acfa6`) + reconciliation items 1–6 + I3/I5/M1–M3 changes
  + 3 new Rust tests.
- Nothing pushed (per rules).

## Residues (no backlog numbers — coordinator assigns)

1. I2: `virt_parent` half of the `circular_atom` site unported (no
   virtual-parent provenance in the walk). pg5 passes without it.
2. I4: suppression's installed exemption is version/slot-only (no
   `[use]` check); threading `Config` is a signature cascade.
3. I5-follow-up: demoted-branch + Replacement-rows corner still
   fixture-unreached (display threading landed, zero drift); wants a
   cycle+blocker fixture with a real oracle.
4. Item-5 follow-up: a `--deep` installed-cp-collision fixture (only
   shape that could exercise the installed deep-walk site at all).
5. M2-follow-up: in-bin promotion among all-demoted branches
   (`dep_check.py:738–802` over `other`) vs suppression's
   first-listed fallback — no fixture shapes it.
6. Corpus bless for the 22 restarts-drift items (table in §2;
   `PORTUALE_CORPUS_BLESS=1` is the coordinator's call, rides in
   pmtest's commit).

## Status

DONE (with BED-PENDING beds for the coordinator; 6 numbered residues
above for the coordinator to file; corpus bless declined per brief).
portuale `b191765a` / pmtest `144a765` on `backlog/221-circular-choices`, both unpushed.
