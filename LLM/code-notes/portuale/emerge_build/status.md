# crates/portuale/src/emerge_build/status.rs
Progress and status lines of the build scheduler: counters, background mode, `>>> Jobs:` and progress lines.

## fn scheduler_cp_version
- Same outcome→version mapping `merge_one_source_entry` does inline. A removal is unreachable via `scheduler_needs_build` (backlog #72 B3).

## fn entry_counts_toward_progress
- Ports Portage's `Scheduler._pkg_count`: only `operation == "merge"` mergelist entries count (uninstalls and nomerge nodes are not counted and print no progress line). `AlreadyInstalled`/`Uninstall`/`NoVisibleCandidate` neither count nor print, exactly like the mergelist entries Portage skips in `MergeListItem._start` (`pkg.installed` returns early).

## fn merge_progress_map
- Ports Portage's `Scheduler._pkg_count`, copied per `MergeListItem` at task creation after `curval += 1`. Keyed positionally (`out[i]` belongs to `entries[i]`; a non-merging entry carries `MergeProgress::single()`, which it never prints), so two merging entries for the same cat/pkg in one run (e.g. two slots of one package) keep their own `cur` instead of collapsing onto one map slot (review of backlog #177). Callers pass `progress[i]` into the merge functions, so the run-level loops own the only scan.
- Narrowing: Portage assigns `curval` in task-start order, which under `--jobs` can differ from merge-list order when independent packages start out of order. Portuale's scheduler dispatches in index order (the `find` scans `0..n`), so list ordinal and start ordinal agree whenever dependencies force the order, and in the common serial case they are identical by construction. Portage also recomputes `maxval` per `--keep-going` pass; portuale keeps the run's original total (a failure-path-only difference).

## fn progress_color
- Ports the gate Portage resolves from `--color y|n` over `NO_COLOR`/`NOCOLOR`/`TERM`/isatty (`color::resolve_havecolor` ports exactly that gate). The merge path threads no `--color` value down (unlike the pretend renderer, which takes `color_opt`).

## struct StatusMode
- Backlog #197: ports Portage's `Scheduler._background_mode`. Portage also resets `max_jobs` for a one-package mergelist; portuale keeps its dispatch and only takes the display half (a scheduled build under `-jN` still captures its phase output). `show_jobs` is Portage's `JobStatusDisplay.quiet == false`, i.e. `background && (!quiet || verbose)`: `--quiet` without `--verbose` suppresses the `>>> Jobs:` lines but not the status lines themselves. Two deliberate cuts, both stated in code: interactive packages (Portage forces non-background with a `>>> Sending package output to stdio` notice) and the `--pretend`/`--fetchonly`/`--fetch-all-uri` exemption.

## fn scheduler_status_mode
- Backlog #197: ports Portage's scheduler-display predicate. Portage's `main.py` normalizes bare/`=y` `--quiet-build` both to `"y"` before `_background_mode` reads it, so bare counts.

## struct StatusDisplay
- Backlog #197: ports Portage's `_emerge/JobStatusDisplay.py` (the `>>> Jobs:` line) plus the `Scheduler._status_msg` half that prefixes every status line with a blank line unless in background mode. Every `println!` still happens under its own call, exactly like the existing merge-path prints.
- `maxval`: Portage counts every `operation == "merge"` mergelist entry; portuale's is the build-bound count (see `scheduler_needs_build`). The two agree whenever every entry builds; the installed-noop counting quirk is out of scope. Portage's `merge_wait` arm is implemented but always reads 0: portuale serializes every vdb merge immediately behind its build and models neither the merge-wait queue nor `FEATURES=merge-wait`.
- `displayed`: Portage's `displayMessage` re-displays the current status after every status line once one is on screen (`JobStatusDisplay.displayMessage`, verified in the n197 probe: each `Emerging`/`Installing`/`Completed` is followed by a `>>> Jobs:` repeat).

## impl StatusDisplay methods
- `status` ports Portage's `Scheduler._status_msg` + `JobStatusDisplay.displayMessage` (non-tty half): the leading blank unless in background mode, the `>>> ` line itself on stdout, then the current `>>> Jobs:` status again when live and showing (the n197 probe shows the repeat after every status line). Portage prints plain lines on a tty too for this half; only the tty `\r`-redraw half is not ported.
- `job_started` ports Portage's `_schedule_tasks_imp` `running = self._jobs` on task start.
- `build_finished` ports Portage's `_build_exit` tail (`running = self._jobs` after the token release).
- `merge_finished` ports Portage's `_merge_exit` `curval += 1`.
- `job_failed` ports Portage's `_build_exit`/`_do_merge_exit` `failed = len(self._failed_pkgs)`.

## fn jobs_line
- Backlog #197: ports Portage's `JobStatusDisplay._display_status` on a non-tty. `self._jobs_column_width = width - 32` at the non-tty `width = max_display_width = 100`; the tty term-width half is cut (portuale always uses the non-tty rule). Portage appends `Load avg: {triple}` after the padding; portuale cuts it exactly like #185 cut the resolution seconds (repeated-run determinism is a jointly-owned gate; nondeterministic values ride `--json` only; coordinator ruling B14, batch-2026-09-27). The counters wear `INFORM`, like Portage's `number_style`; the padding is measured on the plain text, like Portage's `plain_output` gauge.
- `JobsLineArgs` / `jobs_line`: the pure shape, split out for unit tests. Portage's `Load avg:` trailer is cut (see `StatusDisplay::jobs_line`); the padding that precedes it is kept. The first status-test case is the n197 probe's s2 line up to the cut.

## fns emerging_line / installing_line / completed_line
- Backlog #197. `emerging_line` ports Portage's `_emerge/MergeListItem.py::_start`: `{Emerging|Emerging binary|Fetching} ({cur} of {max}) {cpv}::{repo}`. Portage prints it via the scheduler's `statusMessage` path, which prefixes `>>> ` (`JobStatusDisplay._format_msg`) on stdout, hence the literal prefix here.
- `installing_line` ports Portage's `_emerge/PackageMerge.py::_start`. Portage suppresses it under `--fetchonly`/`--pretend`/`--buildpkgonly` (`_should_show_status`); portuale's merge functions never run in those modes, so no gate is needed at the call sites.
- `completed_line` ports Portage's `_emerge/PackageMerge.py::_install_exit`.
- Colour escapes are `color.rs`'s port of `output.py:68-92` (`MERGE_LIST_PROGRESS` = yellow, `PKG_MERGE` = darkgreen, `PKG_BINARY_MERGE` = purple); only the two counters and the `cpv::repo` wear escapes, exactly Portage's `colorize()` call sites.
