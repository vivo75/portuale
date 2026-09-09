# Documentation

Project documentation for `portuale`. The root
[`README.md`](../README.md) is the concise human overview; this directory
holds everything else.

## For contributors / agents

| Doc | What it is |
|---|---|
| [`agent-context.md`](agent-context.md) | **Read first for any development work.** Goals, hard constraints, architecture decisions, the bash-backend investigation, current state, and the open backlog. |
| [`../AGENTS.md`](../AGENTS.md) | The "next slice" workflow and the verification / commit rules. |
| [`scope-backlog.md`](scope-backlog.md) | Real portage behaviour not yet ported (either side), standing non-goals, and the honest distance to a drop-in replacement. |

## Reference

| Doc | What it is |
|---|---|
| [`what-this-proves.md`](what-this-proves.md) | The living, append-only per-slice record — every shipped feature with its real-portage source grounding. |
| [`running-it.md`](running-it.md) | Runnable, live-verified examples for every shipped slice. |
| [`brush-pin.md`](brush-pin.md) | The `brush` (embedded bash) dependency pin, the two fixes it used to carry, and the re-pin checklist. |
| [`real-world-testing.md`](real-world-testing.md) | Plan for the container-based differential test bed (Portuale vs Portage: build / package / binhost / merge / unmerge / `mrg` remote), the normalised-comparison methodology, the L0–L5 test layers, and an honest critique of the "byte-diff everything" approach. **Planning only.** |
| [`remote-merge.md`](remote-merge.md) | The `mrg`-only remote binary-package merge over SSH: design, grounding in real code, the six-slice plan (5 shipped), and open questions. |
| [`operation-diagrams.md`](operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code, plus per-operation detail pages. |
| [`on-disk-caches.md`](on-disk-caches.md) | Source-grounded audit of every on-disk cache/database (`/var/db/pkg`, `/var/cache/edb`, `$PKGDIR`, `/var/lib/portage`, logs): when each is written, read, and modified in place, and which alternatives are worth it. |
| [`emerge-pretend-debug.md`](emerge-pretend-debug.md) | Real portage's `emerge --pretend --debug` resolver trace — every message with its upstream source, the deliberate divergences, and the six-stage port (**implemented 2026-09-07**). |
| [`performances-tuning.md`](performances-tuning.md) | The `perf` + call-counter investigation of a live `emerge -puD --getbinpkg` and the six memoisation fixes that took it from 77 s to 4.5 s (17×, ~3.5× faster than real `emerge`), with the remaining ~1–2 % items. |
| [`refactor-HIGH.md`](refactor-HIGH.md) | Source-grounded audit of the whole `rust/` workspace against the 56 HIGH `rust-skills` rules — mostly deliberate-N/A verdicts; the one live recommendation is `lto = "thin"` + `codegen-units = 1`. |
| [`solver-backends-analysis.md`](solver-backends-analysis.md) | Comparison of resolution backends (portuale's own, pubgrub, resolvo) behind the `--solver=` switch, and why the lu-zero bridges are reused. |

## History

| Doc | What it is |
|---|---|
| [`history/porting-strategy-prompt.md`](history/porting-strategy-prompt.md) | The original porting-strategy prompt, superseded by `agent-context.md`. Kept for the original derivation. |
| [`history/scope-backlog-2026-09-03.md`](history/scope-backlog-2026-09-03.md) | `scope-backlog.md` before the 2026-09-03 compaction (its shipped-item narrative). |
| [`history/agent-context-open-backlog-2026-09-03.md`](history/agent-context-open-backlog-2026-09-03.md) | `agent-context.md`'s "Open backlog" section before it was replaced by a pointer (2026-09-03). |

Co-located READMEs that stay next to their code:
[`../bin/README.md`](../bin/README.md),
[`../3rdparty/README.md`](../3rdparty/README.md),
[`../TEST/README.md`](../TEST/README.md).
