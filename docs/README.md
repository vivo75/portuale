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
| [`backlog-tasks.md`](backlog-tasks.md) | The same open work as a flat, one-line-per-task list with code/doc pointers — for picking up a slice without a full context load. |
| [`second_python_copy_removal.md`](second_python_copy_removal.md) | Why the Python copy of the resolver was removed (2026-09-15), the checks that replaced it, and which are still open. |
| [`what-this-proves.md`](what-this-proves.md) | The append-only per-slice record — every shipped feature with its real-portage source grounding, plus a runnable example. Large; `git log` is the same history per-commit. |
| `<tier>.<item>-<slug>.opus.md` / `.md` | **Per-item implementation plans**, one per backlog item, named for the tier and item number they implement (e.g. [`history/06.057-directly_requested_hard_atom_conflict.opus.md`](history/06.057-directly_requested_hard_atom_conflict.opus.md) is Tier 6 #57). Each carries its own §0 slice rules, owner decisions, slice table and file-conflict map. Their `Status:` header says `proposed` or `done <date>` — a `done` plan is kept for the derivation trail; `backlog-tasks.md` is what actually landed. Legacy plans use the `.opus.md` suffix, current ones plain `.md` (`ls docs/history/0[0-9].*` lists the retired set). The six reviewed in [`history/review-46-53-54-57-55-56.md`](history/review-46-53-54-57-55-56.md) plus `07.058`/`07.60`/`07.61` are done; the 2026-09-16 batch (notably `01.063`/`01.064` done, `01.014`/`01.067`/`01.068`/`02.059`/`02.065`/`02.066` proposed) is current. |
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
| [`solver-backends-analysis.md`](solver-backends-analysis.md) | The `--solver=portage\|pubgrub\|resolvo` backend comparison and why the lu-zero bridges are reused. Real-tree gaps: `scope-backlog.md` §J. |
| [`../../pmtest/differential-test-bed/README.md`](../../pmtest/differential-test-bed/README.md) | The container-based real-system differential test beds (L0 resolver parity, L1 merge parity), in the sibling `pmtest` repo since `f982876`. Controls, triage, and the L2–L5 designs stay here: [`real-world-testing.md`](real-world-testing.md). |
| [`real-world-testing.md`](real-world-testing.md) | Determinism controls, report-triage guidance, archive-comparison and forward-layer (L2–L5) designs extracted from `history/real-world-testing.md` — the live companion to the differential bed's own `README.md` (now in the sibling `pmtest` repo). |

## History (`history/`)

Superseded planning docs, closed explorations, and pre-compaction
snapshots — kept for the derivation trail, not current guidance:
`porting-strategy-prompt.md`, `running-it.md` (per-slice examples),
`real-world-testing.md` (retired planning doc — its live content now
resides in [`real-world-testing.md`](real-world-testing.md):
controls, triage, L2–L5 designs; what remains here is the §1
methodology critique, image specs, and slice history), `emerge-pretend-debug.md`, `dfs-graph-backtracker-exploration.md`
(dead end), `refactor-HIGH.md` / `refactor-CRITICAL.md` (rust-skills
audits), the `scope-backlog-*` / `agent-context-*` snapshots, and the
early per-feature plan docs.

Co-located READMEs stay next to their code:
[`../bin/README.md`](../bin/README.md),
[`../3rdparty/README.md`](../3rdparty/README.md). The differential test
bed's own README moved with it, to
[`../../pmtest/differential-test-bed/README.md`](../../pmtest/differential-test-bed/README.md).
