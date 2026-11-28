# crates/portuale/src/emerge_build/tests/scheduler.rs
Scheduler pins: merge-wait scope shapes, the cycle unblock, and the pretend replay order (#512).

## fn merge_wait_scope_deep_holds_a_system_merge_until_builds_drain (#512, item (1))
- Oracle: `LLM/evidence/2026-10-23-512/captures/01-scope-deep.portage.*`.

## fn merge_wait_scope_none_merges_everything_in_parallel (#512, item (1))
- Oracle: `04-scope-none.portage.*`.

## fn cycle_post_merge_dep_does_not_gate_the_build (#512, item (3))
- Oracle: `09-cycle.portage.*`.

## fn pkg_pretend_replays_in_merge_list_order (#512, item (2))
- Oracle: `10-pretend.portage.*`.

## fn pkg_pretend_failure_aborts_before_anything_builds (#512, item (2))

## fn merge_wait_scope_system_holds_only_system_members (#512, item (1))
- Oracle: `07-system-closure.portage.*`. Review follow-up I2 (the `deep`/`none` pair alone would not catch a mis-wired scope set).

## fn package_env_merge_wait_holds_one_packages_merge (#512, item (1))
- Review follow-up I1.

## fn run_build_scheduler_single_failure_counts_failed_exactly_once
- Backlog numbers removed from the comment (original text): Backlog #197 fix round 1: one failed `-jN` build fires each failure event exactly once. Real `_build_exit`'s failure arm assigns absolutely (`Scheduler.py:1662`, `failed = len(self._failed_pkgs)` -- 1 for a single failure), then drops the freed build slot (`running`, `:1663-1665`): exactly two `>>> Jobs:` lines, ending at `failed == 1`. The pre-fix code fired `job_failed` + `build_finished` twice (once in the `Err` arm, once in the `Some` arm), reaching `failed == 2`. Drives the real scheduler (`run_build_scheduler`) over the `schedbad` fixture (its `src_install` dies) with a caller-owned display, so the counters pin the wiring, not just the display.

## fn a_panicking_build_thread_becomes_a_failure_message
- Backlog numbers removed from the comment (original text): Backlog #376: a build thread that panics reports a failure for its entry instead of leaving the scheduler waiting forever on its result.
