---
name: portuale-oracle-probing
description: Use when you need to establish what is actually true before writing code in portuale — deciding what correct is for a behaviour, running a Portage comparison probe, capturing evidence for a contract pin, reproducing a divergence in ResolverPlayground, reading a differential-test-bed log, or deciding whether a difference from Portage matters at all.
---

# Establishing what is true

Portage is **not the oracle** (`docs/charter.md`). What counts as correct comes
from, in this order:

1. **The PMS, the EAPIs and the Devmanual**; where they are silent, the owner
   decides.
2. **The charter's health properties** — dependencies hold, order is legal,
   packages are as new as possible, removal keeps running programs working,
   messages help. Prefer a check that needs **no expected value**: the
   pmtest `output_invariants.py` checks every contract case and every L0 plan
   for a dependency merged after its owner, two packages in one slot, wrong
   counters and similar, without comparing to anything.
3. **Portage's behaviour**, as a *comparison and a hint*: it tells you what
   another careful implementation does, and where a difference is likely to
   hurt a user. It is never the last word, and its bugs are not copied
   (`LLM/portage-quirks.md`).

A pin's docstring says which of the three its expected text came from. A value
derived from **portuale's own output** proves only that portuale agrees with
itself, and a value copied from Portage proves only that the two agree — say
so, do not present either as correctness.

## Where a Portage comparison comes from

Three sources, in this order of convenience:

1. **`lib/portage/tests/resolver/` playground cases** — cheapest, hermetic,
   and reproducible. Translating an upstream case into a fixture and diffing
   beats reasoning from `depgraph.py` ("run it, don't read it").
2. **The container differential bed** (`../pmtest/differential-test-bed/`) —
   a real Gentoo tree, both sides, recorded logs.
3. **The host `emerge`** — the widest surface, the least controllable.

Record the Portage version with every capture (the current pin is
`portage-3.0.83`; older captures say 3.0.82.2). A capture is a comparison for
that version, not a fact about "Portage".

## Capture discipline

These four cost a slice each when missed, so they are not optional:

```
PYTHONUNBUFFERED=1 <cmd> > log 2>&1      # merged streams
PYTHONHASHSEED=0                          # set-order wobble is real, not flakiness
PORTAGE_GNUPGHOME=<chmod 700 keyring copy>
```

- **Merge the streams.** Separating stdout and stderr hid walk/solver/dump
  ordering and produced a wrong theory that cost two implementation attempts.
- **Record argv and the binary's mtime** alongside the output, or the capture
  cannot be replayed.
- **Stop the `gpg-agent`** the probe spawns and delete its `/tmp` home, or the
  leaks accumulate across runs.
- **Determinism lives in resolved config**, never in the calling shell's env.
  A `make.conf` change and an exported variable can produce identical CLIs and
  silently different settings.

## Probing on the host

- Always `-p` first. Confirm on **two consecutive runs** before a probe becomes
  a claim.
- A genuine rebuild is `-e` plus `--usepkg=n`.
- A test file may rebind `test_cases` right before the consuming loop — verify
  what actually executes before citing "upstream proves".

## The bed is read-only

A recorded differential-bed log is evidence, not a scratchpad. While you are
analysing it, do not overwrite it: a re-run goes to a **new** log, and a
finding is quoted from the log you analysed, not from a later run. (A bed run
after a resolver, merge-order or phase change is still required; see
`portuale-tests-harness`.)

## Reproducing a divergence

Hunt one axis at a time: mutate a playground shape toward the real-tree form
along a single dimension — versioned arms, nesting depth, USE-conditional
wrapping, request shape — until exactly one of them explains the behaviour. If
two explanations survive, the reproduction is not finished.

If a cell shows portuale already matching where the plan predicted a
divergence, stop: the premise is wrong, and there is nothing to fix.

## A parity check is not a version difference

Before attributing a behaviour to a Portage version bump, probe both tags. The
`02.442` errata records a reference line that claimed 3.0.82.2 lacked a fix which
the 3.0.82.2 tag already had — the "difference" was a misreading, and the probe
was a parity check (`LLM/02.442-port-portage-3.0.83.errata.md`).

## Quirks: record, do not copy

A confirmed Portage bug does not become portuale behaviour
(`LLM/portage_bug_list_2026-11-07.html` lists the confirmed upstream ones).
Record a suspect in `LLM/portage-quirks.md` (Q-numbers) and make the omission
explicit in the slice — e.g. Q16, the gpkg basename mismatch warning, is
recorded and deliberately not implemented.

## When a difference does not matter

A difference from Portage is a defect only if the user is worse off: wrong
packages, an order that breaks a build, a message that does not name the cause
and an action. Different wording, a different but legal order, and a quirk
nobody hits are not slices. Ask first which charter property, if any, the
difference violates; if none, record it (`known-divergences.yaml` classifies
each as a portuale bug, a Portage bug, or environmental) and move on.

## The recipe

```
1. State the question as a discriminating predicate
   ("does Portage keep X when Y is masked?"), not as "how does it work".
2. Build the smallest shape that answers it (playground first).
3. Capture both sides with argv + mtime, merged streams, fixed hash seed.
4. Re-read the capture as the coordinator: a worker's verdict is not a fact.
5. Pin the expected text in the contract suite (pmtest) and name its source
   in the docstring: spec text, a charter property, or a Portage capture with
   its version. Never derive it from portuale's own output.
6. Add the hermetic Rust test; run it red, then implement.
```

## Traps

- **A claim from a trace, a subagent or an earlier plan is not a fact** until
  the coordinator has checked it against source or a probe.
- **Re-running to confirm** destroys the evidence you are citing.
- **A capture without argv** cannot be reproduced next month.
- **Timestamps and PIDs in a pinned output** make the pin fail on a different
  day; mask them, do not re-derive the pin.
- **Assuming a divergence exists** because a plan says so: three items in one
  batch had misread `depgraph.py` and the "gap" did not exist.
- **Treating "same as Portage" as "correct"**: where Portage is wrong, or merely
  different but harmless, matching it is not a goal.

## Where to look

| Question | Source |
|---|---|
| How to run the bed, its cells and verdicts | `../pmtest/differential-test-bed/README.md` |
| Known Portage bugs not to copy | `LLM/portage-quirks.md` |
| Contract pins and how they are re-derived | `../pmtest/pytests-contract-suite/` |

## History

The plans, reviews and captures this rule came from are archived: see
[`LLM/ARCHIVE.md`](../../ARCHIVE.md); `git log` and the backlog entries in
`LLM/backlog-tasks.md` hold the per-item story.
