---
name: portuale-slice-delivery
description: Use when turning a backlog entry into a finished piece of work in portuale — scoping a slice, writing its plan, deciding what blocks, running the oracle→fixture→red→green rhythm, choosing which gates to run, or closing it out across the two repositories. Load it before starting a slice, not after the code is written.
---

# Delivering a slice

`AGENTS.md` states the rhythm; this skill is the operating detail that the
rhythm leaves implicit — the parts that cost a slice when they are missed.

## The shape of a slice

A slice is one backlog entry taken to *closed*: code, fixtures, contract pins,
docs, and a green gate. Anything not finished gets a **new backlog number**
before the slice is called done — never a silent remainder
(`LLM/2026-10-19-open-backlog-plan.md`, "No push. No work outside §1 without
the owner. A residue gets a real number (#421+) before the slice is called
done.").

The rhythm, unchanged across every plan in this tree:

```
oracle → fixture → red test → code → green test → gates → close-out
```

`LLM/superpowers/plans/2026-10-21-backlogs-444-445-455-456.md` names it
"oracle → fixture → contract-pin + rust-unit". The order is not stylistic: a
test written after the code proves the code does what it does, which is
worthless.

## S0 — the capture-only slice

Every investigation slice starts with **zero product code**: one failing run
with full logs, plus the oracle capture of what correct looks like
(`LLM/superpowers/plans/2026-10-29-626-hyperfine.md`: "S0 is capture-only (no
product code): one failing run with full logs plus the Portage success slice
as oracle").

S0 has a stop rule. If the capture contradicts the entry's premise, the slice
STOPs and reports `NEEDS_CONTEXT` with the traces — it does not build a
fixture to a shape that does not exist. `LLM/02.652-novel-demoter.opus.md` §0
is the worked example: the plan's Task 1 exists to discover whether the
premise holds, and Tasks 2–3 are conditional on that answer.

## Planning: what blocks and what takes a default

A batch plan divides every open question into **[BLOCCA]** — without an answer
that part cannot start or cannot close — and **[default sicuro]** — the
executor takes the default and proceeds
(`LLM/2026-10-15-batch-plan.md`). The point is that an unasked question never
silently becomes a blocker three tasks later.

Every step in a plan names **one test or one function/file, the command, and
the expected output**. A step that says "handle this properly" is a step the
reviewer cannot check (`LLM/superpowers/plans/2026-10-21-backlogs-444-445-455-456.md`,
"Step scan: every step names one test or one function/file + the command and
expected output").

## Concurrency: one writer

Exactly one agent edits the portuale tree at a time. Parallelism is allowed
only in read-only work or against a frozen binary — the captures on the
Portage side can run in parallel with the host because they compile nothing
(`LLM/2026-10-19-open-backlog-plan.md`, "strict single-writer policy to
prevent tree corruption, allowing parallelism only in read-only or frozen-binary slots").

Parallel *implementers* are allowed when the tasks touch disjoint subsystems,
because then "two writer agents never edit the same file or crate"
(`LLM/2026-10-19-open-backlog-plan.review-gemini.md`). The pattern is: a fresh
subagent implements one task, a fresh reviewer checks it, then the next task
starts, then a whole-branch review at the end.

## Backlog numbers

Numbers are global across tiers and **never reused**. Before filing, scan every
tier file for the next free number. Batch plans hand each plan a *block* of
numbers and forbid straying outside it, because "three plans all claimed 371"
(`LLM/2026-10-15-batch-plan.md`).

## Two repositories

- The same branch name is opened in `portuale` and `../pmtest` **at the same
  time**, before the first task. A branch in one repo only is already drifted.
- Commits are paired: **pmtest first**, then portuale quoting the pmtest short
  sha; pmtest's message names the portuale branch and slice.
- Corpus blesses ride in the pmtest commit, never here.
- Pushes go together, portuale first.

Source: `LLM/2026-10-15-batch-plan.md` ("Ogni branch va aperto **in entrambi i
repo contemporaneamente, con lo stesso nome**").

## Gates

The full pass — `cargo fmt --check`, `cargo clippy --release --all-targets`,
`cargo test --release`, then the pmtest suite — runs before a slice is done.
Two extra rules:

- **Merge-path safety gate.** Any change touching the merge path requires a
  successful test merge of `sys-libs/glibc` and `app-shells/bash`. Plans
  declare it up front (`LLM/feat-157-authoritative-vdb-database.md`: "for s4
  and s5 (glibc + bash test merge"), and a batch plan escalates it to "every
  slice from S2 onwards" once the path is in play.
- **Differential bed** after big merges and after resolver/merge-order/phase
  changes. L0 first, L1 when merge behaviour moved.

## Close-out

1. Flip the backlog entry, and the plan's own `Status:` header, in the **same
   commit** that lands the last slice.
2. Append a paragraph to `LLM/what-this-proves.md` — never rewrite an earlier
   one.
3. Give every deferred residue a real, free backlog number.
4. Commit the pair; do not push unless asked (`LLM/superpowers/plans/2026-10-31-636-loopback.md`:
   "push unless asked (separate request). pmtest stays commit-free.").
5. Run `python3 LLM/tools/doc_status_check.py` before committing: it checks
   plan headers against the backlog, duplicated numbers, tier prefixes and the
   shrink-only `ALLOW` list.

## Traps

- **A residue without a number is a lost residue.** It disappears from the
  backlog and resurfaces as a "new" bug months later.
- **A contract pin derived from portuale's own output** proves only that
  portuale is consistent with itself. Pins come from a live Portage capture
  (`LLM/superpowers/plans/2026-10-29-626-hyperfine.md`).
- **A plan whose premise is wrong must stop, not adapt.** Rewriting the fixture
  to fit the implementation is how a wrong premise becomes a shipped bug.
- **A batch plan accumulates uncommitted slices** unless it names the evidence
  file each plan writes its diff stat and gate into
  (`LLM/2026-10-15-batch-plan.md`).
- **Two writers on one crate** corrupt the tree in a way no test detects; the
  failure shows up as a mysterious revert later.

## Where to look

| Question | Source |
|---|---|
| The rhythm, rules, numbering, two-repo pairing | `AGENTS.md` |
| Hard-won working lessons, one idea per bullet | `LLM/lessons-of-backlog-ops-2026-10-03.md` |
| Per-item status, residues, cuts for closed items | `LLM/recap-of-backlog-ops-2026-10-03.md` |
| Open work and its numbers | `LLM/backlog-tasks.md` |
| A worked batch plan with blocking/default decisions | `LLM/2026-10-15-batch-plan.md` |
| Subagent-per-task execution | `LLM/superpowers/plans/2026-10-21-backlogs-444-445-455-456.md` |
| Doc/tooling invariants | skill `portuale-docs-and-charter` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
