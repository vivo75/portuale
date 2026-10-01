# Pin table — what the root split moves, by name (B/C/D's guard checklist)

"Real bytes" below means the fresh captures in `probes/` (real 3.0.82.2,
2026-10-01). Portuale's current bytes are unchanged by this slice; where a
pin already records them, the pin name is the reference.

## Moves in B (build-time deps resolve against the running root)

| Finding / test name | What moves, and toward what |
|---|---|
| `test_circular_dependencies_upstream_pg0_real_text[=dev-libs/cyc0z-1]` (strict-xfail, pmtest `test_emerge_pretend_contract.py`) | Flips to passing: `backtrack: 1/20`, Total 3, cycle `cyc0z-3 ↔ cyc0y-1`, suggestion `cyc0z-3 (+bar -foo)` — `probes/default-cyc0z-1.txt`. Same for `=dev-libs/cyc0z-2` (its CASES label; no separate real-text pin). |
| Batch-2 CASES `circular: upstream test_circular_dependencies pg0 =cyc0z-{1,2,3}` + `=cyc0w-{1,2,3}` labels | Re-pinned from playground-oracle wording to the default-staging real text where B changes portuale's output (exit codes already agree). |
| `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1` — missed line only | Missed line `USE="" ELIBC="glibc"` → `USE="" ABI_X86="(64)"` (`probes/default-blk0-bca.txt` / `-cba.txt`); parents and package lists already match. Retires the missed-line half of the allowlist below. |
| `skipped-updates-cross-root-missed-line` (pmtest `differential-test-bed/compare/known-divergences-fixture-oracle.yaml`, `a7dba52`) | Narrowed to dead, then removed: the missed-line USE half lands in B, the parent-suffix half in D. The `match:` regex's `portuale-only (1)` arm goes in B with the row-set change. |
| Fixture-oracle blk0 rows (`l0-fixture-oracle.txt`, 6 orders) | The 4 block-carrying cells move toward real (missed-line USE); `acb`/`cab` stay clean. |
| New: hermetic running-root-`DEPEND` fixture cells (B S3) + Rust unit/e2e tests (B S2) | Added in B; fail without B's S1, pass with it. |

## Moves in C (one merge list spanning two roots)

| Finding / test name | What moves, and toward what |
|---|---|
| `l0-fixture-oracle-g216.txt` (4 rows) | Run under default staging: `FX_HOST_ROOTS=1` dropped with a one-line header reason. Real bytes: `probes/default-g216top.txt` (5 rows, dual `g216comp`), `probes/default-g216comp.txt` (3 rows), `probes/default-g216top-b0.txt` / `-g216comp-b0.txt` (Total 4 + self block). Host-exact controls: `probes/hostroots-g216*.txt`. |
| `test_or_pick_direct_target_resolves_to_bootstrap` / `test_or_pick_direct_target_backtrack0_reports_the_self_cycle` (pmtest, M3 `c9f6e8a`) + the 4 g216 CASES | Re-pinned to the dual-row + Total 4 default-staging bytes. |
| `l0-fixture-oracle-slotop.txt` (18 rows) | `FX_HOST_ROOTS=1` dropped per-row-group with header reason (C S3; `-g215` and `-slotop` after `-g216`). The `@world` order class stays allowlisted (below). |
| `l0-fixture-oracle-g215.txt` (4 rows) | Same knob drop; prediction from this inventory: clean under default staging (RDEPEND-only) — BED-PENDING pass confirms. |
| `Total:` counter pins wherever dual rows land | Count both roots (real: Total 4 on the g216 b0 shape). |

## Moves in D (display: `to '<root>'` vs bare, cross-root USE paint)

| Finding / test name | What moves, and toward what |
|---|---|
| Every merge-list row pin under a non-`/` ROOT | Gains real's suffix: unquoted `to <EROOT>` on merge-list rows (`output.py:462/475/861`), quoted `to '<ROOT>'` inside skipped-block detail lines (`Package.__str__`). Includes the cyc0z/cyc0w/blk0/g216 merge rows captured here, the blk0 parents in `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`, and every other `-p` contract pin run under `fixture_env`'s staged (non-`/) ROOT — D's S0 enumerates them pin by pin (B9 conditional stop applies if the bare form turns out to encode a wider deliberate rule). |
| `_norm_skipped_detail` (`resolve-compare.py:131`, pmtest `2963703`) | Retire, keep, or keep-then-retire decided in D per the pin table: safe to retire only once portuale emits the suffixes. |
| Corpus (`pytests-contract-suite/corpus/`) | Bless whatever D's bytes move, drift reviewed first, in the pmtest commit (`PORTUALE_CORPUS_BLESS=1`). |
| Docs | `what-this-proves.md` paragraph (live-verified example); `scope-backlog.md` edge-by-edge row narrowed; #242 entry flip prepared for P-Z. |

## Must not move (any movement stops the slice — B10)

| Name | Why |
|---|---|
| `l0-resolver.sh` — every row | Single-root (`ROOT=/`, like the `FX_HOST_ROOTS=1` lists). Identical row by row. |
| `test_circular_dependencies_upstream_pg0_real_text_cyc0w3` (passing, ex-#208(a) via #278) | Cycle text identical under both stagings (this inventory's §1b); B/C/D must not touch it. |
| `test_circular_dependencies_upstream_pg0_real_text_cyc0b1` (passing, #206) | Same reason; bare-node text pinned. |
| g212 `somaskvisparent` cell (+ all RDEPEND-only oracle cells) | Annotated "no staged-ROOT cross-root split (#242) is involved". |
| #161 `lib.rs` driver unit tests | Regression net, green throughout. |
| `slotop-world-complete-graph-order` allowlist | #17 merge-order family, orthogonal to roots. |
| `g216-b0-cycle-abort-tree-ancestors` allowlist | Owned by backlog #245 (needs real's `--debug` digraph); X notifies, does not move it. |
| `fixture-masked-*` / `fixture-unmet-requirements-wording` / `fixture-problem-resolving-header` presentation entries | Staging-path artifacts, orthogonal to roots. |
| `l0-fixture-oracle-host.txt` (`=dev-libs/disjtarget-2.0`) | Intrinsic host-exact shape (§2); stays `FX_HOST_ROOTS=1`. |
| Small-case oracle, `l31`/`l32` controls | Comparison points for P-Z. |
