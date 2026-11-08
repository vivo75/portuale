# LLM/

Working material for coding agents: workflow context, the backlog, plans,
the per-slice record, evidence captures and the agents' own tools. Humans
can read it, but it is written for agents doing backlog work. Human
documentation lives in [`../docs/`](../docs/README.md); the operating rules
are in [`../AGENTS.md`](../AGENTS.md).

Paths and identifiers in documents written before 2026-10-10 may predate
the #336 refactor: [`renames.tsv`](renames.tsv) maps each old path or name
to its current one.

## The knowledge is in the skills

The durable, reusable knowledge of this tree lives in [`skills/`](skills/), one
skill per kind of work — delivery, oracle probing, the resolver, ebuild and
merge, the test harness, code conventions, the output contract, the Portage
source map, and the document rules. Each is loaded on demand; start from
`skills/<name>/SKILL.md`. The per-item plans and the capture corpus are
**record**, not reference: the plans stay because the backlog links to them,
and everything that was raw capture is in
[`ARCHIVE.md`](ARCHIVE.md) (89 MB out of the tree, restorable from git or from
one tarball).

| Doc | What it is |
|---|---|
| [`agent-context.md`](agent-context.md) | **Read first for any development work.** Goals, hard constraints, architecture decisions, pointers to the live state and backlog. |
| skill `portuale-slice-delivery`, `portuale-oracle-probing` | **Read before any backlog slice.** The condensed how-to-work lessons (oracle method, gates, beds, numbering, performance) were distilled into those two skills; the source file is in the archive. |
| [`backlog-tasks.md`](backlog-tasks.md) | The open work, one line per task, with code and doc pointers. Item numbers are global and never reused. |
| [`scope-backlog.md`](scope-backlog.md) | What Portage does that portuale doesn't (and the reverse), the standing non-goals. |
| [`ARCHIVE.md`](ARCHIVE.md) | What left the tree on 2026-11-08, where it went, and how to restore it. Per-item status, residues and deliberate cuts for closed items are in the archived `recap-of-backlog-ops-2026-10-03.md`. |
| [`what-this-proves.md`](what-this-proves.md) | Append-only per-slice record with Portage source grounding and a runnable example. |
| `<tier>.<item>-<slug>.opus.md` | Per-item implementation plans (AGENTS.md rule 11). |
| [`plans/`](plans/) | Plans for cross-cutting work (e.g. the #336 readability refactor) with their reviews and decision logs. |
| archived `glep-compliance-review.md` | Per-GLEP compliance audit, source of backlog #55, #56, #58. |
| archived `feat-157-authoritative-vdb-database.md` | Design notes for the authoritative VDB database (#157). |
| archived `Paragone_solver_portage.md` | Why Portage's merge order is not a topological sort (Italian); distilled into skill `portuale-resolver`. |
| [`evidence/`](evidence/) | The prose reports still cited by backlog entries. The raw captures (86 MB) are archived; see [`ARCHIVE.md`](ARCHIVE.md). |
| [`superpowers/`](superpowers/) | Design specs written with the superpowers workflow. |
| [`tools/`](tools/) | Agent tooling: `gate.sh` (full verification pass), link checker/relinker, bed comparison helpers, refactor scripts. [`tools/portuale-distill/`](tools/portuale-distill/README.md) is the pipeline that produced the skills and the archive. |
| [`tools/scope/`](tools/scope/README.md) | Run agents and long jobs in systemd user scopes and stop them without `pkill` (`oc.sh`, `scoperun.sh`, `scopewait.sh`, `ocstatus.sh`, `ocgrep.sh`, `ocstop.sh`). |
| [`reference/`](reference/) | Background: the local distfile mirror (`distfile-mirror.md`), related Rust Portage projects, the prompts behind `docs/cleanroom/`. |
| archived `ideas.md` | The owner's idea list (Italian/English). |
| [`renames.tsv`](renames.tsv) | Old → new paths and identifiers since 2026-10-10. |
