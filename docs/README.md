# Documentation

Project documentation for `portuale`. The root
[`README.md`](../README.md) is the concise human overview; this directory
holds everything else.

## For contributors / agents

| Doc | What it is |
|---|---|
| [`agent-context.md`](agent-context.md) | **Read first for any development work.** Goals, hard constraints, architecture decisions, the bash-backend resolution, and pointers to the live state + backlog. |
| [`../AGENTS.md`](../AGENTS.md) | The "next slice" workflow and the verification / commit rules. |
| [`scope-backlog.md`](scope-backlog.md) | What real portage does that portuale doesn't (either side), the standing non-goals, the distance to a drop-in replacement. |
| [`lessons-of-backlog-ops-2026-10-03.md`](lessons-of-backlog-ops-2026-10-03.md) | **Read before any backlog slice.** Dense, deduplicated how-to-work lessons (oracle method, gates, beds, numbering, performance) condensed from the 09-19 → 10-03 batches. |
| [`recap-of-backlog-ops-2026-10-03.md`](recap-of-backlog-ops-2026-10-03.md) | Per-item status / residue / deliberate-cut / trap index for closed items; mechanism lives in the `backlog-tasks.md` entry and `evidence/`. |
| [`backlog-tasks.md`](backlog-tasks.md) | The same open work as a flat, one-line-per-task list with code/doc pointers — for picking up a slice without a full context load. |
| [`what-this-proves.md`](what-this-proves.md) | The append-only per-slice record — every shipped feature with its real-portage source grounding, plus a runnable example. Large; `git log` is the same history per-commit. |
| `<tier>.<item>-<slug>.md` | Per-item implementation plans are written when an item is scoped (AGENTS.md rule 11 flips their `Status:` on shipping) and may be **deleted in a later cleanup**, as on 2026-10-03: the outcome moves into `backlog-tasks.md`, `what-this-proves.md`, `evidence/` and the two `*-of-backlog-ops-2026-10-03.md` files (older plans: `git log --diff-filter=D`). |
| [`glep-compliance-review.md`](glep-compliance-review.md) | Per-GLEP compliance audit (78 binary containers, 82 `layout.conf`, 74/59/61 sync) with a prioritised gap list — the source of backlog items #55, #56 and #58. |

## Reference

| Doc | What it is |
|---|---|
| [`brush-pin.md`](brush-pin.md) | The `brush` (embedded bash) dependency pin, the staged upstream fixes, and the re-pin checklist. |
| [`remote-merge.md`](remote-merge.md) | The `mrg`-only remote binary-package merge over SSH: design, real-code grounding, the six-slice plan (all shipped), the open questions. |
| [`diagrams/operation-diagrams.md`](diagrams/operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code; per-operation detail in `diagrams/emerge-*.md`. |
| [`diagrams/emerge-source-merge.md`](diagrams/emerge-source-merge.md), [`diagrams/emerge-unmerge.md`](diagrams/emerge-unmerge.md), [`diagrams/emerge-getbinpkgonly.md`](diagrams/emerge-getbinpkgonly.md), [`diagrams/emerge-pretend-world.md`](diagrams/emerge-pretend-world.md) | Per-operation code walkthroughs (companions to `diagrams/operation-diagrams.md`). |
| [`on-disk-caches.md`](on-disk-caches.md) | Source-grounded audit of every on-disk cache/database (`/var/db/pkg`, `/var/cache/edb`, `$PKGDIR`, `/var/lib/portage`, logs) and which alternative backends are worth it — informs the `mrg-director` DB/cache slots. |
| [`performances-tuning.md`](performances-tuning.md) | The `perf` + call-counter investigation that took `emerge -puD --getbinpkg` from 77 s to 4.5 s (17×, ~3.5× faster than real `emerge`), with the remaining incremental items. |
| `solver-backends-analysis.md` (deleted, in git history) | The `--solver=portage\|pubgrub\|resolvo` backend comparison and why the lu-zero bridges are reused. Real-tree gaps: `scope-backlog.md` §J. |
| [`../../pmtest/differential-test-bed/README.md`](../../pmtest/differential-test-bed/README.md) | The container-based real-system differential test beds (L0 resolver parity, L1 merge parity), in the sibling `pmtest` repo since `f982876`. Controls, triage, and the L2–L5 designs stay here: [`real-world-testing.md`](real-world-testing.md). |
| [`real-world-testing.md`](real-world-testing.md) | Determinism controls, report-triage guidance, archive-comparison and forward-layer (L2–L5) designs extracted from the retired `real-world-testing.md` planning doc — the live companion to the differential bed's own `README.md` (now in the sibling `pmtest` repo). |

## History

`docs/history/` (superseded plans, snapshots, closed explorations) and the
per-phase / per-batch plan files were deleted on 2026-10-03; their useful
content is condensed into the two `*-of-backlog-ops-2026-10-03.md` files.
The rest is recoverable from git history.

Co-located READMEs stay next to their code:
[`../bin/README.md`](../bin/README.md),
[`../3rdparty/README.md`](../3rdparty/README.md). The differential test
bed's own README moved with it, to
[`../../pmtest/differential-test-bed/README.md`](../../pmtest/differential-test-bed/README.md).
