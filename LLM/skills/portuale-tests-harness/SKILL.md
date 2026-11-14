---
name: portuale-tests-harness
description: Use when adding, changing or debugging a test in portuale — writing a fixture, adding a CASES entry, pinning contract output, running the pytest suite, reading corpus drift, choosing between L0/L1 and the fixture-oracle bed, or when a suite result looks wrong.
---

# The test harness

The suite is **not in this tree**. It lives in the sibling repository
`../pmtest`, which tests several package managers against one harness with
portuale as one entry in `managers/managers.yaml`. Its `USAGE.AGENTS.md` is the
operating manual — read it before changing anything there.

## The layout map

| Was here | Is now |
|---|---|
| `tests/` | `../pmtest/pytests-contract-suite/` |
| `fixtures/` | `../pmtest/fixtures/` — this tree's `fixtures` is a gitignored **symlink** |
| `TEST/` | `../pmtest/differential-test-bed/` |
| `python/*_harness.py` | `../pmtest/python-harness/` |
| `bench/` | `../pmtest/bench/` |
| `scripts/primitive_tree_differential.py`, `portage_repin_review.py` | `../pmtest/scripts/` |

**There is one fixture tree and it is pmtest's.** Every new or corrected
fixture is added there, and lands in the pmtest commit. A missing or wrong tree
fails loudly on both sides (`fixtures_tree_is_the_pmtest_checkout` in Rust, the
`_require_fixture_tree` session fixture in pmtest), rather than quietly testing
less.

`3rdparty/` stays here; it holds the pinned Portage checkout behind
`PORTUALE_PYTHON_HELPERS=real`. The two repos expect to sit next to each other.

## The rules that make a result trustworthy

- **Never edit the harness to make a package manager pass.** A red is fixed in
  the product; an accepted divergence goes in
  `../pmtest/differential-test-bed/compare/known-divergences.yaml`.
- **Compare by test name, never by count.** The suite has an order-dependent
  isolation bug, so a count change can hide a swapped verdict.
- **Re-take baselines on `main` before the first slice**, then each slice's
  guard is "row-identical to the baseline, or better on a row it names".
- **Never edit the harness to make an output pin move**: a moved pin is a
  reviewed decision, listed for the owner before the commit.
- **A strict-xfail that XPASSes fails the suite.** Convert it to a pin on the
  first XPASS; do not leave a boxed test.
- **Determinism has one seam**: every directory read goes through
  `portage_util::read_dir_entries`, sorted by default; the seeded shuffle is
  test-only (`PORTUALE_SHUFFLE_DIRS`). A new unsorted read is a flake waiting
  for a load average.

## Adding a test

The shape has three parts, and all three are cheap to do right:

1. **A fixture, by hand**, under `../pmtest/fixtures/repo/…` plus a real
   `metadata/md5-cache/…` entry (generated with `egencache` in the container,
   never hand-written). Check for name collisions first — prefixes are how
   collisions are avoided (`p491`-prefixed, `pg359-*`, …). A fixture that
   "passes" without isolating the new behaviour is worse than none.
2. **A `CASES` entry** in the contract suite, so the output invariants run over
   it automatically, plus **a pinned test function** in
   `test_emerge_pretend_contract.py` whose docstring names the capture file the
   expected text came from.
3. **A Rust unit test** here, in the crate that changed. Real-execution
   features get fixture-driven end-to-end tests instead.

`git add` the new fixture before any `git clean`; an untracked fixture is
invisible to worktrees and gets deleted.

## Corpus drift

`pytests-contract-suite/corpus/` records harvested output. A change that moves
a recorded row reports **corpus drift**. Read it line by line before blessing;
unexplained drift blocks the commit. Then accept it with
`PORTUALE_CORPUS_BLESS=1` **in the pmtest commit**, never in this repo's.

## The beds

| Bed | What it catches | When |
|---|---|---|
| `l0-resolver.sh` | merge lists, USE, order, errors, exit codes for ~120 real atoms | periodically, and after resolver/merge-order/phase changes |
| `l1-merge-from-binpkg.sh` | filesystem + VDB after merging an identical binpkg set | whenever merge behaviour changed — and it **must** run with the portage upgrade (`L1_SKIP_PORTAGE_UPGRADE=1` produces spurious `metadata` findings) |
| `l0-fixture-oracle-all.sh` | the same ground per fixture, compared with Portage | when a per-fixture question needs a real tree |

The beds are the only thing that catches a regression at real-tree scale; the
fixture suite cannot. **Read a bed result by the charter, not by equality**: a
finding is a portuale bug (fix it), a Portage bug, or environmental
(`compare/known-divergences.yaml`, with a `ticket:` or `reason:`), and a run is
green when every finding is explained. A difference that violates no charter
property is not a failure by itself. Properties that need no reference at all
are checked by `health_checker.py` (`run/l0-health.sh`) and by
`output_invariants.py` over the same output
(`differential-test-bed/compare/check-invariants.py`); see
`portuale-health-checks`. Container preflight:
`sudo differential-test-bed/create-container.bash`, run from pmtest.

A **frozen binary** is mandatory for an L0 re-run: any non-baseline probe that
moves is a regression — stop and file it, do not "re-run and see".

## Running

Run a long job through a named systemd scope, never as a bare background shell,
so its liveness can be checked without `ps`/`pgrep` (see
`LLM/tools/scope/README.md`):

```
sh LLM/tools/scope/scoperun.sh <slug> /tmp/<slug>.log ../pmtest/scripts/pytest-parallel.sh
sh LLM/tools/scope/ocgrep.sh <slug> [pattern]   # RUNNING / STOPPED, read-only
sh LLM/tools/scope/ocstatus.sh <slug> /tmp/<slug>.log [hung_secs]   # STOPPED / HUNG / RUNNING
sh LLM/tools/scope/scopewait.sh <slug> [secs]   # block until the scope is gone
sh LLM/tools/scope/ocstop.sh <slug>             # TERM then KILL the whole scope
```

Before a run that writes a lot (parallel suite, an L0/L1 bed, a corpus run),
`sh LLM/tools/check-work-space.sh` must pass: `/tmp` and `/var/tmp` each need
at least 50% free. One full suite leaves ~11 GB in its `PMTEST_BASETEMP` under
`/var/tmp` (48 GB), so stale runs fill it and the next suite fails with ENOSPC,
not with a real test failure. `pytest-parallel.sh` and `gate.sh` run the check
themselves; delete finished basetemps only, never one a live suite is using.

```
../pmtest/scripts/pytest-parallel.sh          # whole suite, 8 workers, ~90 s, rebuilds the binary
JOBS=n ../pmtest/scripts/pytest-parallel.sh   # change workers; extra args go to pytest
python3 -m pytest pytests-contract-suite -q   # serial fallback, ~8 min
```

## Traps

- **A test that lacks the pending entry** goes through the live fallback and
  discriminates nothing.
- **A harness that compares only post-merge `CONTENTS`** cannot see a file one
  side removed and the other kept.
- **A lint test that scans zero files** passes vacuously; assert the file list
  is non-empty.
- **Running the gate in a parallel worktree** builds and tests the main
  checkout instead.
- **A fixture without a real md5-cache entry** makes the pin meaningless.
- **A pin derived from portuale's output** proves self-consistency only; one
  copied from Portage proves agreement with Portage. Its docstring names the
  source (spec, charter property, or a Portage capture and version).

## Where to look

| Question | Source |
|---|---|
| The full operating manual for the sibling repo | `../pmtest/USAGE.AGENTS.md` |
| Layout, modes, beds, known-divergences | `LLM/agent-context.md`, "Where the tests live" |
| Bed cells and their atoms | `../pmtest/differential-test-bed/atomlists/` |
| The contract suite and its invariants | `../pmtest/pytests-contract-suite/` |

## History

The plans, reviews and captures this rule came from are archived: see
[`LLM/ARCHIVE.md`](../../ARCHIVE.md); `git log` and the backlog entries in
`LLM/backlog-tasks.md` hold the per-item story.
