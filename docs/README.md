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
| [`what-this-proves.md`](what-this-proves.md) | The append-only per-slice record — every shipped feature with its real-portage source grounding, plus a runnable example. Large; `git log` is the same history per-commit. |

## Reference

| Doc | What it is |
|---|---|
| [`brush-pin.md`](brush-pin.md) | The `brush` (embedded bash) dependency pin, the staged upstream fixes, and the re-pin checklist. |
| [`remote-merge.md`](remote-merge.md) | The `mrg`-only remote binary-package merge over SSH: design, real-code grounding, the six-slice plan (all shipped), the open questions. |
| [`operation-diagrams.md`](operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code; per-operation detail in `emerge-*.md`. |
| [`emerge-source-merge.md`](emerge-source-merge.md), [`emerge-unmerge.md`](emerge-unmerge.md), [`emerge-getbinpkgonly.md`](emerge-getbinpkgonly.md), [`emerge-pretend-world.md`](emerge-pretend-world.md) | Per-operation code walkthroughs (companions to `operation-diagrams.md`). |
| [`on-disk-caches.md`](on-disk-caches.md) | Source-grounded audit of every on-disk cache/database (`/var/db/pkg`, `/var/cache/edb`, `$PKGDIR`, `/var/lib/portage`, logs) and which alternative backends are worth it — informs the `mrg-director` DB/cache slots. |
| [`performances-tuning.md`](performances-tuning.md) | The `perf` + call-counter investigation that took `emerge -puD --getbinpkg` from 77 s to 4.5 s (17×, ~3.5× faster than real `emerge`), with the remaining incremental items. |
| [`solver-backends-analysis.md`](solver-backends-analysis.md) | The `--solver=portage\|pubgrub\|resolvo` backend comparison and why the lu-zero bridges are reused. Real-tree gaps: `scope-backlog.md` §J. |
| [`../TEST/README.md`](../TEST/README.md) | The container-based real-system differential test beds (L0 resolver parity, L1 merge parity). L2–L5 plan: `scope-backlog.md` §I + `history/real-world-testing.md`. |

## History (`history/`)

Superseded planning docs, closed explorations, and pre-compaction
snapshots — kept for the derivation trail, not current guidance:
`porting-strategy-prompt.md`, `running-it.md` (per-slice examples),
`real-world-testing.md` (the L0–L5 test-bed plan + §1 methodology
critique), `emerge-pretend-debug.md`, `dfs-graph-backtracker-exploration.md`
(dead end), `refactor-HIGH.md` / `refactor-CRITICAL.md` (rust-skills
audits), the `scope-backlog-*` / `agent-context-*` snapshots, and the
early per-feature plan docs.

Co-located READMEs stay next to their code:
[`../bin/README.md`](../bin/README.md),
[`../3rdparty/README.md`](../3rdparty/README.md),
[`../TEST/README.md`](../TEST/README.md).
