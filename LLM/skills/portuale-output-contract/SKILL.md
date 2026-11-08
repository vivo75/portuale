---
name: portuale-output-contract
description: Use when changing what emerge or mrg prints — display rows, colours, --tree, counters, error and warning wording, stream assignment, the failure summary, news notices, or when a pinned output row moved and you need to know what the contract actually is.
---

# The output contract

What the user reads is part of the behaviour. The charter's property 5 is the
rule: **every error names the cause (which package, which atom, which flag) and
says at least one thing the user can do about it.** A wrong or unhelpful message
is a defect; a moved pin is a decision, not an accident.

`portuale emerge` may differ from Portage in wording and order. The pins
below mostly record what Portage printed and portuale reproduces today; they
are a regression guard, not a statement that Portage's text is right. Improve
a message through a reviewed pin move, and keep the cause and the action.

## Three properties define a row

Every output change is a change to one of:

1. **Text** — the exact words, including punctuation and trailing newlines.
2. **Stream** — stdout or stderr. Portage prints the failure summary on stderr
   only when multiple packages fail or under `--keep-going`; the per-package
   failure line goes to stdout after the phase.
3. **Order and gating** — which rows appear under which flags, and where in the
   run. A warning placed before the config load is a different contract from
   the same warning after it.

Pins encode all three. A pin that compares only text misses the other two.

Some properties are checked with **no expected text at all**:
`../pmtest/pytests-contract-suite/output_invariants.py` runs over every contract
case and the harvested corpus (counters add up, a dependency is not merged
after its owner, no slot twice, plain, `--tree`, `--quiet` and `--json`
describe the same graph). Prefer adding an invariant there to adding another
pinned string.

## What is genuinely tty-only

Only three things should differ between a pipe and a terminal:

- **the spinner frames** — the count depends on wall-clock time, so they are
  never byte-stable and must be excluded from pins;
- **cursor hide/show sequences**;
- **the terminal title update**, which is written to stderr only when `TERM`
  matches and `notitles` is unset.

Anything else that differs on a tty is a bug. In particular `NO_COLOR` in the
caller's environment does not turn colour off on a tty in a fixture.

## Root and attribution

- Portage's **running root is always the host**; when `ROOT != /` it also loads
  the target root's config. Portuale paints every node from **its own**
  `root_config`.
- The root suffix is not a wider rule: Portage prints it in `--columns` too.
- Root paths in pins are **normalised**; entry lines are compared as a
  root-normalised multiset plus verbatim anchors.

## The messages people notice

- Per-package failure:
  `>>> Failed to emerge <cpv> for <ROOT>/, Log file:` on **stdout**.
- Under `--keep-going`: the resume banner and the dependency-resolution timing
  line before continuing.
- Failure summary on stderr only when several failed or `--keep-going` was on.
- Masked blocks: `!!! The following installed packages are masked:` — a block
  portuale has historically omitted on some paths.
- Post-merge notices go through the same print-point machinery as everything
  else; the four print-point families must match in gate, text, stream **and**
  order.

## News notices

The news machinery (`FilesystemNews`, `run_check_news`) has no `FEATURES` gate
of its own; the resolved `FEATURES` must contain `news` or nothing is printed.
The `--pretend`-end arm is deliberately **not** ported: printing there would
write `.unread`/`.skip` into the shared, git-tracked fixture root during
contract tests (`LLM/evidence/2026-09-28-batch0927-reports/n196-report.md`).

## Error text

For unrecognised arguments and invalid parameters, portuale currently passes
Portage's message through **unchanged**, including trailing newlines and
phrasing, and recycles unrecognised trailing arguments into argv as Portage
does. That is pinned so it does not drift; it is not a requirement. A clearer
message is welcome when it still names the cause and an action.

## Helper exit codes

The `portageq`-family helpers are called by ebuilds and scripts, so their exit
codes are an interface to keep, and they are not uniform:

| Helper | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| `has_version` | matched | no installed match | invalid atom | insufficient parameters |
| `best_version` | always (found or not) | — | invalid atom | insufficient parameters |

Atom validation is strict only when `EBUILD_PHASE` is set; when `USE` is absent
conditionals stay unevaluated rather than failing.

## Traps

- **A pin that asserts empty stderr can be wrong**: Portage sometimes prints
  block information on the plain row, and the pin must be replaced rather than
  the output silenced.
- **Doubling a warning** to match a repo-profile miss-state is a Portage
  behaviour that was deliberately *not* copied; each distinct warning is
  emitted once.
- **Timestamp, PID and root differences** are masked, never "fixed" by
  re-deriving the pin from portuale.
- **A blank-line padding difference** is still a difference; record it or
  remove it, do not let it accumulate unmentioned.
- **A display change is a resolver change** when it changes which row is
  selected: check `portuale-resolver` before assuming it is cosmetic.

## Where to look

| Question | Source |
|---|---|
| Per-site audits and their matrices | `LLM/evidence/*-display-sites.md`, `LLM/evidence/*/matrix.md` |
| Failure-output contract | `LLM/evidence/2026-10-19-b426-design.md` |
| Pins and their re-derivation | `../pmtest/pytests-contract-suite/test_emerge_pretend_contract.py` |
| Output invariants run over every case | `../pmtest/pytests-contract-suite/test_output_invariants.py` |
| The display module's own notes | `LLM/code-notes/portuale/pretend/` |

## Provenance

The documents this skill was distilled from, and how many verified facts each
contributed: [`references/sources.md`](references/sources.md).
