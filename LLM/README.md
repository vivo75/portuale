# LLM/

Working material for coding agents: workflow context, the backlog, plans,
the per-slice record, evidence captures and the agents' own tools. Humans
can read it, but it is written for agents doing backlog work. Human
documentation lives in [`../docs/`](../docs/README.md); the operating rules
are in [`../AGENTS.md`](../AGENTS.md).

Paths and identifiers in documents written before 2026-10-10 may predate
the #336 refactor: [`renames.tsv`](renames.tsv) maps each old path or name
to its current one.

| Doc | What it is |
|---|---|
| [`agent-context.md`](agent-context.md) | **Read first for any development work.** Goals, hard constraints, architecture decisions, pointers to the live state and backlog. |
| [`lessons-of-backlog-ops-2026-10-03.md`](lessons-of-backlog-ops-2026-10-03.md) | **Read before any backlog slice.** Condensed how-to-work lessons (oracle method, gates, beds, numbering, performance). |
| [`backlog-tasks.md`](backlog-tasks.md), [`backlog-tasks-2026-10.md`](backlog-tasks-2026-10.md) | The open work, one line per task, with code and doc pointers. Item numbers are global and never reused. |
| [`scope-backlog.md`](scope-backlog.md) | What Portage does that portuale doesn't (and the reverse), the standing non-goals. |
| [`recap-of-backlog-ops-2026-10-03.md`](recap-of-backlog-ops-2026-10-03.md) | Per-item status, residues, deliberate cuts and traps for closed items. |
| [`what-this-proves.md`](what-this-proves.md) | Append-only per-slice record with Portage source grounding and a runnable example. |
| `<tier>.<item>-<slug>.opus.md` | Per-item implementation plans (AGENTS.md rule 11). |
| [`plans/`](plans/) | Plans for cross-cutting work (e.g. the #336 readability refactor) with their reviews and decision logs. |
| [`glep-compliance-review.md`](glep-compliance-review.md) | Per-GLEP compliance audit, source of backlog #55, #56, #58. |
| [`feat-157-authoritative-vdb-database.md`](feat-157-authoritative-vdb-database.md) | Design notes for the authoritative VDB database (#157). |
| [`Paragone_solver_portage.md`](Paragone_solver_portage.md) | Solver comparison notes (Italian). |
| [`evidence/`](evidence/) | Probe captures, inventories and bed reports cited by backlog entries. |
| [`superpowers/`](superpowers/) | Design specs written with the superpowers workflow. |
| [`tools/`](tools/) | Agent tooling: `gate.sh` (full verification pass), link checker/relinker, bed comparison helpers, refactor scripts. |
| [`tools/scope/`](tools/scope/README.md) | Run agents and long jobs in systemd user scopes and stop them without `pkill` (`oc.sh`, `scoperun.sh`, `scopewait.sh`, `ocstatus.sh`, `ocgrep.sh`, `ocstop.sh`). |
| [`reference/`](reference/) | Background: the local distfile mirror (`distfile-mirror.md`), related Rust Portage projects, the prompts behind `docs/cleanroom/`. |
| [`ideas.md`](ideas.md) | The owner's idea list (Italian/English). |
| [`renames.tsv`](renames.tsv) | Old → new paths and identifiers since 2026-10-10. |
