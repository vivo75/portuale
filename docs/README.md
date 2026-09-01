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
| [`operation-diagrams.md`](operation-diagrams.md) | Block diagrams tracing four representative `emerge` invocations through the code, plus per-operation detail pages. |

## History

| Doc | What it is |
|---|---|
| [`history/porting-strategy-prompt.md`](history/porting-strategy-prompt.md) | The original porting-strategy prompt, superseded by `agent-context.md`. Kept for the original derivation. |

Co-located READMEs that stay next to their code:
[`../bin/README.md`](../bin/README.md),
[`../3rdparty/README.md`](../3rdparty/README.md),
[`../TEST/README.md`](../TEST/README.md).
