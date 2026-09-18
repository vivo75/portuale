# Backlog tasks — 2026-10

> Paths and identifiers written before 2026-10-10 may predate the #336 refactor: see [`renames.tsv`](renames.tsv).

One line per open task. Each has a code/doc pointer so a fresh model can
start without a full context load. This file continues
[`backlog-tasks.md`](backlog-tasks.md) (last entry there: #298); numbering
carries on from it. Source of truth for detail:
[`scope-backlog.md`](scope-backlog.md), `what-this-proves.md` (what already
shipped), `git log`, and the memory notes under
`.claude/projects/-home-vivo-repo-PORTUALE-portuale/memory/`.

Rules that apply to every task: take expected output from real Portage
(container bed, host `emerge`, or an upstream resolver test), pin it in
`pytests-contract-suite/test_emerge_pretend_contract.py` /
`test_portuale.py`, keep `test_output_invariants.py` green, and run the
full verification pass (`AGENTS.md` step 8). The suite and the container
bed live in the sibling `pmtest` repo (`agent-context.md`, "Where the tests
live"). The Python mirror was removed 2026-09-15 —
[`history/second_python_copy_removal.md`](history/second_python_copy_removal.md).

---

