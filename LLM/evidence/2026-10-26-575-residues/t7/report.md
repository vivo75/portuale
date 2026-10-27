# Task 7 report: hold unrelated builds while a system package has unmerged runtime deps (#581)

Date: 2026-10-27. Branch `fix/2026-10-26-575-residues` (both repos, no
commits made — manager commits).

## Files changed (portuale only; no pmtest half — no fixtures touched)

- `crates/portuale/src/emerge_build/scheduler.rs`
  - `UnsatisfiedSystemDepsArgs` + `unsatisfied_system_runtime_deps`: the
    direct runtime children of a landed system entry still needing a
    merge. Direct children only, `runtime || runtime_post` edges no
    installed package satisfies, entries still to merge, empty off `/`.
    The `satisfied` bit lives on the merge-order digraph's edge copies
    only, so the check asks `dep_edge_satisfied_by_installed` against the
    target root's vdb at fill time (equivalent outcome — see code-notes).
    `required_by`-only fallback edges count as runtime, like the digraph's
    synthesized edge.
    Portage: `_emerge/Scheduler.py`, `_system_merge_started` (`:1686-1730`,
    skip at `:1699-1702`).
  - `system_deps_hold_new_builds(in_flight, set)`: `in_flight > 0 &&
    !set.is_empty()`.
    Portage: `_schedule_tasks_imp` (`:2469-2476`).
  - `run_build_scheduler`: per-run `unsatisfied_system_deps:
    BTreeSet<usize>` state (function-local, so returning drops it the way
    `_main_loop_cleanup` clears Portage's at `:1988`); `NoteLandedArgs`
    carries `deps`/`wait_set`/the set; `note_landed` drains the landed
    entry and fills when a `wait_set` member lands (success arm and the
    postinst arm, which also lands, like `_task_complete`); the dispatch
    loop breaks before choosing when the hold predicate fires.
  - The brief's check is answered yes: `wait_set` (`merge_wait_set`,
    `:744`) is scope-derived and independent of the `hold` (merge-wait
    feature) vector, so system packages queue with merge-wait off exactly
    as `Scheduler.py:1833-1835` requires. No change needed there; the new
    test `system_package_waits_even_without_merge_wait` pins it.
- `crates/portuale/src/emerge_build/tests/scheduler.rs`: the three RED
  tests (below).
- `LLM/code-notes/portuale/emerge_build/scheduler.md`: marked the old
  "deliberate cut" paragraph superseded, added `## fn
  unsatisfied_system_runtime_deps, fn system_deps_hold_new_builds`.
  Ran `python3 LLM/tools/code_notes_index.py` (index in sync).

## What each test pins (Portage citation in each docstring)

- `unrelated_build_waits_for_system_runtime_deps` (pure, `ROOT == "/"`):
  S (system) with unmerged runtime child X, U unrelated. Fill == `{X}`
  (U never enters); gate blocks at `in_flight > 0`, lets the idle build
  through, never blocks on an empty set; after X lands the fill is empty
  and dispatch reopens. Cites `_system_merge_started`, the `:2472` gate,
  the `:1872` drain.
- `system_deps_gate_skipped_off_root`: same shape at a tempdir ROOT —
  fill is empty — then the full scheduler runs it (`--jobs 2`, S
  wait-queued as the `@system` member, X slow via `schedwait`, U freed by
  S's landing). U lands before X, proving no hold off `/`. With the `/`
  check broken U would wait behind X and land after it, so this fails on
  a regression of the skip. Cites `:1699-1702`.
- `system_package_waits_even_without_merge_wait`: S+X cycle plus slow
  unrelated U, `FEATURES` without merge-wait. S's merge lands after the
  slow build drains. Cites `:1833-1835`. This one was green before the
  fix (characterization of the brief's check, not a RED).

## Commands and results

- RED: `cargo test --release -p portuale --bin portuale scheduler` —
  failed to compile (new API missing): the expected RED for tests 1–2;
  test 3's body compiles without the new API but asserts pre-existing
  behavior.
- After the fix: `cargo build --release`, then the same filter — 27/27
  pass (includes a first-attempt failure in test 3: my wiring had put U
  in S's `required_by`, making U S-gated instead of unrelated; fixed by
  leaving U disconnected).
- Blast radius: `cargo test --release -p portuale --bin portuale
  emerge_build` — 103/103 pass (needed one rebuild after `cargo fmt`
  touched the source: the stale-binary guard fails scheduler tests until
  `target/release/portuale` is newer than the sources).
- `cargo fmt --check` clean (after `cargo fmt`); `cargo clippy --release
  --all-targets` zero warnings.
- pmtest: no pmtest files touched, so no pmtest runs. Bed cell: spec'd in
  `t7/shape.md`, run deferred (manager's gate).

## Open doubts / OWNER QUESTION

None needing an owner ruling. Two deliberate deviations, both documented
in the code-notes:

1. Fill runs when the system merge *lands*, not on a start-listener:
   portuale serializes merges on the main thread, so merge start and land
   are atomic — no concurrent merge can observe the difference.
2. Function-local state instead of an explicit clear: the set cannot
   outlive the run, matching `_main_loop_cleanup`'s clear.

One residue candidate for the manager (§R triage, phase-2-shaped, not
filed by me): the `satisfied` check reads the live vdb at fill time
rather than the resolve-time digraph bit. Outcome-equivalent today, but
if the resolver ever marks an edge satisfied for a reason other than an
installed match, the two drift. Suggested entry if filed: scheduler
satisfied-bit parity, Portage `DepPriority.py` + `Scheduler.py:1707-1717`.
