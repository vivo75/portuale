---
name: portuale-slice-delivery
description: Use when turning a backlog entry into a finished piece of work in portuale — scoping a slice, writing its plan, deciding what blocks, running the ground→fixture→red→green rhythm, choosing which gates to run, or closing it out across the two repositories. Load it before starting a slice, not after the code is written.
---

# Delivering a slice

`AGENTS.md` states the rhythm; this skill is the operating detail that the
rhythm leaves implicit — the parts that cost a slice when they are missed.

## The shape of a slice

A slice is one backlog entry taken to *closed*: code, fixtures, contract pins,
docs, and a green gate. Anything not finished gets a **new backlog number**
before the slice is called done — never a silent remainder.

The rhythm, unchanged across every plan in this tree:

```
ground → fixture → red test → code → green test → gates → close-out
```

*Ground* means: find out what correct is. Take it from the PMS, the EAPIs and
the Devmanual, and from the charter's health properties; use a Portage capture
as a comparison and a hint (`portuale-oracle-probing`). The order is not
stylistic: a test written after the code proves the code does what it does,
which is worthless. Name in the slice which charter property it serves and
which program checks it.

## S0 — the capture-only slice

Every investigation slice starts with **zero product code**: one failing run
with full logs, plus the evidence of what correct looks like (spec text, a
property, or a Portage capture as a comparison)
("S0 is capture-only, no product code").

S0 has a stop rule. If the capture contradicts the entry's premise, the slice
STOPs and reports `NEEDS_CONTEXT` with the traces — it does not build a
fixture to a shape that does not exist. A plan can make this explicit: its first
task exists to discover whether the premise holds, and the later tasks are
conditional on that answer.

## Planning: what blocks and what takes a default

A batch plan divides every open question into **[BLOCCA]** — without an answer
that part cannot start or cannot close — and **[default sicuro]** — the
executor takes the default and proceeds. The point is that an unasked question never
silently becomes a blocker three tasks later.

Every step in a plan names **one test or one function/file, the command, and
the expected output**. A step that says "handle this properly" is a step the
reviewer cannot check.

## Concurrency: one writer

Exactly one agent edits the portuale tree at a time. Parallelism is allowed
only in read-only work or against a frozen binary — the captures on the
Portage side can run in parallel with the host because they compile nothing
(a strict single-writer policy, with parallelism only in read-only or
frozen-binary slots).

Parallel *implementers* are allowed when the tasks touch disjoint subsystems,
because then "two writer agents never edit the same file or crate". The pattern is: a fresh
subagent implements one task, a fresh reviewer checks it, then the next task
starts, then a whole-branch review at the end.

## Backlog numbers

Numbers are global across tiers and **never reused**. Before filing, scan every
tier file for the next free number. Batch plans hand each plan a *block* of
numbers and forbid straying outside it, because "three plans all claimed 371".

## Two repositories

- The same branch name is opened in `portuale` and `../pmtest` **at the same
  time**, before the first task. A branch in one repo only is already drifted.
- Commits are paired: **pmtest first**, then portuale quoting the pmtest short
  sha; pmtest's message names the portuale branch and slice.
- Corpus blesses ride in the pmtest commit, never here.
- Pushes go together, portuale first.

## Gates

`sh LLM/tools/gate.sh` runs the whole pass: the document checkers, the pairing
and merge-path check (`pair_check.sh`), `cargo fmt --check`,
`cargo clippy --release --all-targets`, a release build, `cargo test --release`,
then the pmtest suite, and compares failures by test name. Never run it while a
bed run is active. Two extra rules:

- **Merge-path safety gate.** Any change touching the merge path requires a
  successful test merge of `sys-libs/glibc` and `app-shells/bash`. A plan
  declares up front which slices touch the path, and a batch plan escalates it
  to every slice once the path is in play.
- **Differential bed** after big merges and after resolver/merge-order/phase
  changes. L0 first, L1 when merge behaviour moved.

## Close-out

1. Flip the backlog entry, and the plan's own `Status:` header, in the **same
   commit** that lands the last slice.
2. Append a paragraph to `LLM/what-this-proves.md` — never rewrite an earlier
   one.
3. Give every deferred residue a real, free backlog number.
4. Commit the pair; do not push unless asked. pmtest stays commit-free.").
5. Run `python3 LLM/tools/doc_status_check.py` before committing: it checks
   plan headers against the backlog, duplicated numbers, tier prefixes and the
   shrink-only `ALLOW` list.

## Traps

- **A residue without a number is a lost residue.** It disappears from the
  backlog and resurfaces as a "new" bug months later.
- **A contract pin derived from portuale's own output** proves only that
  portuale is consistent with itself. A pin's docstring names where its
  expected text came from: spec text, a charter property, or a Portage capture
  with its version. A Portage capture shows what Portage does, not that it is
  right.
- **A plan whose premise is wrong must stop, not adapt.** Rewriting the fixture
  to fit the implementation is how a wrong premise becomes a shipped bug.
- **A batch plan accumulates uncommitted slices** unless it names the evidence
  file each plan writes its diff stat and gate into.
- **Two writers on one crate** corrupt the tree in a way no test detects; the
  failure shows up as a mysterious revert later.

## Where to look

| Question | Source |
|---|---|
| The rhythm, rules, numbering, two-repo pairing | `AGENTS.md` |
| Open work and its numbers | `LLM/backlog-tasks.md` |
| Why a past slice was done as it was | `LLM/ARCHIVE.md` (archived plans), `git log` |
| Doc/tooling invariants | skill `portuale-docs-and-charter` |

## History

The plans, reviews and captures this rule came from are archived: see
[`LLM/ARCHIVE.md`](../../ARCHIVE.md); `git log` and the backlog entries in
`LLM/backlog-tasks.md` hold the per-item story.
