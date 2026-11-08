---
name: portuale-oracle-probing
description: Use when you need to establish what is actually true before writing code in portuale — running a Portage probe, capturing expected output for a contract pin, reproducing a divergence in ResolverPlayground, reading a differential-test-bed log, or deciding whether a difference from Portage matters at all.
---

# Establishing the oracle

In this tree an **oracle** is an observed behaviour of real Portage, captured
verbatim, that the expected value of a test is derived from. It is not a
reading of Portage's source, and it is never portuale's own output
(`LLM/02.442-port-portage-3.0.83.opus.md`: "never from this plan's source
reading and never from portuale's own output").

## Where the oracle comes from

Three sources, in this order of convenience:

1. **`lib/portage/tests/resolver/` playground cases** — cheapest, hermetic,
   and reproducible. Translating an upstream case into a fixture and diffing
   beats reasoning from `depgraph.py`
   (`LLM/lessons-of-backlog-ops-2026-10-03.md`, "Run it, don't read it").
2. **The container differential bed** (`../pmtest/differential-test-bed/`) —
   a real Gentoo tree, both sides, recorded logs.
3. **The host `emerge`** — the widest surface, the least controllable.

Expected values come from **Portage 3.0.83-r1**. A probe that "confirms"
portuale matches itself proves nothing.

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
  cannot be replayed (`LLM/2026-10-19-open-backlog-plan.md`, "captures
  verbatim with argv and binary mtime").
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

A differential-bed log is evidence, not a scratchpad. **Never re-run the bed to
"confirm"**: a re-run re-grades, and the recorded log stops being the thing you
analysed (`LLM/02.645-real-tree-nested-virtual-s0.opus.md`, "read-only on the
bed: never re-run the bed to 'confirm'").

## Reproducing a divergence

Hunt one axis at a time. `LLM/02.645-real-tree-nested-virtual-s0.opus.md`
mutates a playground shape toward the real-tree form along exactly one
dimension — versioned arms, nesting depth, USE-conditional wrapping, request
shape — and requires that **exactly one** of them explains why the mechanism is
silent. If two explanations survive, the reproduction is not finished.

The prohibition is symmetric: if a cell shows portuale already matching where
the plan predicted a divergence, the slice's stop rule fires. You found the
premise is wrong, not a fix.

## A parity check is not a version difference

Before attributing a behaviour to a Portage version bump, probe both tags. The
`02.442` errata records an oracle line that claimed 3.0.82.2 lacked a fix which
the 3.0.82.2 tag already had — the "difference" was a misreading, and the probe
was a parity check (`LLM/02.442-port-portage-3.0.83.errata.md`).

## Quirks: record, do not copy

A confirmed Portage bug does not become portuale behaviour. Record it in
`LLM/portage-quirks.md` (Q-numbers) and make the omission explicit in the plan
— e.g. Q16, the gpkg basename mismatch warning, is recorded and deliberately
not implemented (`LLM/evidence/2026-10-25-review/t5/README.md`, "stderr
warning is recorded as portage quirk q16. portuale does not copy it.").

## When a difference does not matter

Portage is a teacher, not the oracle of record (`docs/charter.md`). A
difference is a defect only if the user is worse off: wrong packages, an order
that breaks a build, or a misleading message. Different wording, different
internal order with the same effect, and a quirk nobody hits are not slices.

## The recipe

```
1. State the question as a discriminating predicate
   ("does Portage keep X when Y is masked?"), not as "how does it work".
2. Build the smallest shape that answers it (playground first).
3. Capture both sides with argv + mtime, merged streams, fixed hash seed.
4. Re-read the capture as the coordinator: a worker's verdict is not a fact.
5. Pin the expected text in the contract suite (pmtest), never from portuale.
6. Add the hermetic Rust test; run it red, then implement.
```

## Traps

- **A claim from a trace, a subagent or an earlier plan is not a fact** until
  the coordinator has checked it against source or a probe.
- **Re-running to confirm** destroys the evidence you are citing.
- **A capture without argv** cannot be reproduced next month.
- **Timestamps and PIDs in a pinned output** make the pin fail on a different
  day; mask them, do not re-derive the pin.
- **Assuming a divergence exists** because the plans say so: three items in one
  batch had misread `depgraph.py` and the "gap" did not exist.

## Where to look

| Question | Source |
|---|---|
| Prose capture discipline, one idea per bullet | `LLM/lessons-of-backlog-ops-2026-10-03.md` |
| How to run the bed, its cells and verdicts | `../pmtest/differential-test-bed/README.md` |
| Known Portage bugs not to copy | `LLM/portage-quirks.md` |
| Contract pins and how they are re-derived | `../pmtest/pytests-contract-suite/` |
| The recorded S0 reports, per item | `LLM/evidence/*/SUMMARY.md`, `LLM/evidence/*/matrix.md` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
