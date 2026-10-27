# Owner question: Task 7 (#581), system-package runtime-deps gate

Date: 2026-10-27. Plan 02.589 Task 7, S0 fallback ("If no shape can show
it on the bed, write OWNER-QUESTION-t7.md (port from source with unit
tests only, or park) and go on to phase-1 end").

## Where it stands

- **The port exists and is green, but not committed.** It is saved as
  `t7/port.patch` (applies cleanly on `1fe54fb9`, the phase-1 tip). On that
  tree it passed:
  - the full gate: fmt, clippy with 0 warnings, 3165 Rust tests and 4552
    pmtest tests, no failures and no corpus drift;
  - the glibc + bash merge-path gate, run `l1-20261027T014327Z`: both
    packages merged on both sides, merge_rc 0, 0 findings, 0 payload
    diffs.
- **What it does.** It ports `_system_merge_started` /
  `_unsatisfied_system_deps` (`lib/_emerge/Scheduler.py:1686-1730`, the
  gate at `:2469-2476`, the drain at `:1870-1872`, the `ROOT == "/"` skip
  at `:1699-1702`):
  - the set is filled when a `@system` (deep) package's merge lands. The
    merges run serially on the main thread, so this is the same as
    Portage filling it when the merge starts;
  - it is drained as the children merge;
  - while the set is non-empty and a build is running, no new build
    starts.

  The worker also checked that system packages already go to the
  merge-wait queue with `FEATURES=-merge-wait` (`:1833-1835`), and pinned
  that with a test. Three unit tests in all
  (`t7/report.md`).
- **What is missing is the S0 bed proof.** `t7/shape.md` gives the only
  shape that shows the gate: a `PDEPEND`/`RDEPEND` cycle S↔X with S in
  `@system`, an unrelated U, `--jobs 2`, `ROOT=/`. Running it needs an L3
  cell with its own overlay and an `@system` profile entry inside the bed
  container. No current runner supports that (`run/l3-source-parity.sh`
  takes tree atoms only), so Portage's start/land order has not been
  captured.

## The choice

1. **Port with unit tests only.** Apply `t7/port.patch` and commit it as
   `scheduler: hold new builds while a system package waits for its
   runtime deps (#581)`. #581 stays open for the bed proof, or that proof
   is filed as its own item. (Recommended: the source mapping is direct,
   the patch is gated green, and the behaviour only matters at `ROOT=/`
   with a dependency cycle.)
2. **Build the bed cell first.** Add overlay and `@system` support to an
   L3 runner (a pmtest change), capture Portage's order, then commit the
   port with the cell.
3. **Park #581.** Leave it open with this evidence.
