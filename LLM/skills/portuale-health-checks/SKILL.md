---
name: portuale-health-checks
description: Use when you must show that a change keeps the installation healthy, when adding or extending a check for one of the charter's five properties, when reading an L0 health report or a health finding, or when deciding which program verifies a behaviour instead of comparing with Portage.
---

# Health checks

The charter's goal is a healthy installation, and it wants each of its five
properties "checked by a program". This skill says which program checks which
property, how to run it, how to read it, and what is not covered yet. Start
from `docs/charter.md` for the properties themselves.

## Which program checks which property

| Property | Checked by | Not covered |
|---|---|---|
| 1. Dependencies hold (runtime and post-merge atoms matched, no blocker, `REQUIRED_USE` holds, one package per slot) | `health_checker.py` `check_final_state`, on `emerge -pv --json` (a plan) and, with `--installed ROOT`, on the VDB a real merge left. Also `output_invariants.py` (every owner is an entry or installed, every entry reachable, no duplicate slot without a reported conflict) and the dependency-drop guard (see `portuale-resolver`) | |
| 2. Order is legal (build dependencies merged earlier and usable, PMS 8.1) | `health_checker.py` `check_order`; `output_invariants.py` "merges after its owner" | 2a: an unserializable cycle is only *noted* (`cycle-noted`); building a package twice is not modelled |
| 3. Newest visible version that every constraint allows | `health_checker.py` `check_best_versions` (needs the VDB and `-v --json`). Candidates are the repositories' ebuilds and the binary packages of the local index (`PKGDIR/Packages`), as far as the plan's `options` (`usepkg`, `usepkgonly`) let the run use them; a binary whose USE differs from the configured one is not a candidate | a remote binhost (`--getbinpkg`) is not read (a note); entries absent from the metadata cache or the index (`checker-limitation`) |
| 4. Removal keeps the libraries remaining programs link | `health_checker.py` `check_removal_plan` on `-C`, `--depclean`, `--prune` with `--pretend --json`; `--installed ROOT` checks that every installed ELF still finds its libraries after a real merge (a preserved library is a file on disk); Rust tests `ebuild_merge/tests/plib_*.rs` for the preserve-libs mechanics | a real unmerge on a real tree is compared with Portage (the L32 cells), not gated |
| 5. Messages name the cause and an action | **no program** | everything; only pinned text exists, and it fixes known messages, not the rule |

`health_checker.py` lives in `../pmtest/pytests-contract-suite/`. Its atom
matcher, version comparison and dependency evaluator are written from the PMS
and share no code with portuale or Portage, so it cannot inherit their bugs.
(`docs/CONTRIBUTING.md` and the pmtest README still say it covers properties 1
and 2 only; the code and `l0-health.sh` cover 1–4.)

## Running it

```
# one plan, by hand (a Gentoo tree with a VDB)
portuale emerge -pv --json <atoms> > plan.json
python3 ../pmtest/pytests-contract-suite/health_checker.py --vdb /var/db/pkg plan.json

# every plan of the L0 atom list, in the container (Portage is not run)
../pmtest/differential-test-bed/run/l0-health.sh [atomlist]
#   -> differential-test-bed/logs/l0-health-<stamp>/health-report.txt
#   exit 0: no plan portuale would run has a failure it introduced; 2: setup error

# the system a real merge left: before and after, only what the merge introduced fails
python3 ../pmtest/pytests-contract-suite/health_checker.py --installed / > before.jsonl   # before merging
python3 ../pmtest/pytests-contract-suite/health_checker.py --installed / --baseline before.jsonl
#   the L1 bed (run/l1-merge-from-binpkg.sh, incl. the glibc+bash gate) does this and exits 1 on a failure

# the checker's own tests, and its mutation tests
python3 -m pytest pytests-contract-suite/test_health_checker.py      # from ../pmtest
python3 ../pmtest/differential-test-bed/compare/health-mutants.py --vdb /var/db/pkg plan.json...
python3 ../pmtest/differential-test-bed/compare/health-state-mutants.py /      # the installed-state check
```

## Reading a finding

A finding is `(property, package, atom, reason, pre_existing)`.

- It is a **failure** unless the property is `cycle-noted`, `checker-limitation`,
  `removal-deps` or `private-library`. Those are notes: a cycle the charter
  allows, a blind spot of the checker, a dependency a removal leaves unmatched
  (the user may remove it; portuale warns), and a library installed outside the
  linker's search path (a JDK, systemd), which its own program finds.
- `pre_existing` means the system was already broken before the plan; it does
  not count against the plan.
- A plan is **`refused`** when portuale itself would not run it (slot conflict,
  a needed configuration change, an unsolvable blocker). Its findings are
  expected, and listed apart.
- **`checker-limitation` is never "healthy".** It says the checker could not
  see, for lack of dependency data, the VDB or cache metadata. Count them.

## Extending or adding a check

- Write it **from the PMS or the charter**, not from Portage's code and not by
  calling portuale's. A check that shares code with the thing it checks agrees
  with its bugs.
- Give it **mutants**: add an operator to `health-mutants.py` that breaks a clean
  plan so the new check must fire. The run exits 1 when a mutant survives, which
  marks a blind spot. Add unit tests to `test_health_checker.py`, including a
  healthy-plan control that must stay clean.
- Make the failure/note classification explicit and keep finding names stable;
  the report groups on them.
- Prefer a check that needs **no expected value** to another pinned string. A
  pin records what was printed; a check records what must be true.
- A fix for a health defect starts from a red test whose expectation comes from
  the checker or the spec, not from Portage (see the pin that cites
  `health_checker.py` in `test_emerge_pretend_contract.py`).

## Closing a slice against the charter

Name the property the slice serves and the program that checks it. If the
behaviour has no program, say so in the close-out and file the gap under a real
backlog number. Until a check exists, code that merely copies Portage stays
(charter); once a check shows that doing without it is at least as good, it can
go.

## Gaps, as of 2026-11-08

1. Property 5 has no checker.
2. The installed state after a real merge is checked for properties 1 and 4 only
   (`--installed`); a final state does not record order (2) or visibility (3).
   It runs in the L1 bed and in one hermetic merge test, not after the L3 source
   builds or the L32 life-cycle cells.
3. `health_checker.py` runs on the L0 plans and a few fixtures, not over every
   contract case.
4. Property 2a (building a package twice to break a cycle) is not modelled.
5. Property 3 reads only the local binary index, not a remote binhost (#689).

## History

The checker landed on 2026-11-07, with the charter. The plans and reviews behind
the other beds are archived: see [`LLM/ARCHIVE.md`](../../ARCHIVE.md).
