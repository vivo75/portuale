# crates/portuale/src/emerge_build/status.rs
Progress and status lines of the build scheduler: counters, background mode, `>>> Jobs:` and progress lines.

## fn scheduler_cp_version
- Same outcome→version mapping `merge_one_source_entry` does inline. A removal is unreachable via `scheduler_needs_build` (backlog #72 B3).

## fn entry_counts_toward_progress
- Ports Portage's `Scheduler._pkg_count`: `Scheduler.py:296-304` counts only `operation == "merge"` mergelist entries (uninstalls and nomerge nodes are not counted and print no progress line). `AlreadyInstalled`/`Uninstall`/`NoVisibleCandidate` neither count nor print, exactly like the mergelist entries Portage skips in `MergeListItem._start` (`pkg.installed` returns early).
- Portuale's equivalents are the outcomes that actually merge: `New`/`Reinstall`/`Upgrade`/`Downgrade`.

## fn merge_progress_map
- Ports Portage's `Scheduler._pkg_count`, copied per `MergeListItem` at task creation after `curval += 1`, `Scheduler.py:2273`/`2355`: `max` is the count of merging entries, `cur` the 1-based ordinal in merge order. One snapshot per entry position, keyed positionally, not by `(category, package)` (`out[i]` belongs to `entries[i]`; a non-merging entry carries `MergeProgress::single()`, which it never prints), so two merging entries for the same cat/pkg in one run (e.g. two slots of one package) keep their own `cur` instead of collapsing onto one map slot (review of backlog #177). Callers pass `progress[i]` into the merge functions, so the run-level loops own the only scan.
- Narrowing: Portage assigns `curval` in task-start order, which under `--jobs` can differ from merge-list order when independent packages start out of order. Portuale's scheduler dispatches in index order (the `find` scans `0..n`), so list ordinal and start ordinal agree whenever dependencies force the order, and in the common serial case they are identical by construction. Portage also recomputes `maxval` per `--keep-going` pass (`Scheduler.py:1307-1312`); portuale keeps the run's original total (a failure-path-only difference).

## fn progress_color
- The colouriser for the merge progress lines: Portage `actions.py:2816-2828` resolves module-global `havecolor` from `--color y|n` over `NO_COLOR`/`NOCOLOR`/`TERM`/isatty (`color::resolve_havecolor` ports exactly that gate). The merge path threads no `--color` value down (unlike the pretend renderer, which takes `color_opt`), so this resolves the ambient default: colour on a tty, off when piped. Either way the line shape is Portage's; only the escapes differ.

## struct StatusMode
- Backlog #197: ports Portage's `Scheduler._background_mode` (`3rdparty/portage/lib/_emerge/Scheduler.py:470-531`) as a value. `background` is Portage's `Scheduler._background`: a parallel `--jobs` run, `--quiet`, or `--quiet-build=y`, except that a one-package mergelist without `--quiet`/`--quiet-build=y` resets to serial and non-background. Portage also resets `max_jobs` for a one-package mergelist; portuale keeps its dispatch and only takes the display half (a scheduled build under `-jN` still captures its phase output). `show_jobs` is Portage's `JobStatusDisplay.quiet == false`, i.e. `background && (!quiet || verbose)` (`Scheduler.py:524-527`): `--quiet` without `--verbose` suppresses the `>>> Jobs:` lines but not the status lines themselves. Two deliberate cuts, both stated in code: interactive packages (Portage forces non-background with a `>>> Sending package output to stdio` notice; portuale models no `interactive` PROPERTIES) and the `--pretend`/`--fetchonly`/`--fetch-all-uri` exemption.

## fn scheduler_status_mode
- Backlog #197: ports Portage's scheduler-display predicate. Portage's `main.py` normalizes bare/`=y` `--quiet-build` both to `"y"` before `_background_mode` reads it, so bare counts.

## struct StatusDisplay
- Backlog #197: ports Portage's `_emerge/JobStatusDisplay.py` (the `>>> Jobs:` line) plus the `Scheduler._status_msg` half (`Scheduler.py:2383-2397`) that prefixes every status line with a blank line unless in background mode. Every `println!` still happens under its own call, exactly like the existing merge-path prints.
- `maxval`: Portage counts every `operation == "merge"` mergelist entry; portuale's is the build-bound count (see `scheduler_needs_build`). The two agree whenever every entry builds; the installed-noop counting quirk is out of scope. Portage's `merge_wait` arm is implemented but always reads 0: portuale serializes every vdb merge immediately behind its build and models neither the merge-wait queue nor `FEATURES=merge-wait`.
- `displayed`: Portage's `displayMessage` re-displays the current status after every status line once one is on screen (`JobStatusDisplay.displayMessage`, verified in the n197 probe: each `Emerging`/`Installing`/`Completed` is followed by a `>>> Jobs:` repeat).

## impl StatusDisplay methods
- `status` ports Portage's `Scheduler._status_msg` + `JobStatusDisplay.displayMessage` (non-tty half): the leading blank unless in background mode, the `>>> ` line itself on stdout, then the current `>>> Jobs:` status again when live and showing (the n197 probe shows the repeat after every status line). Portage prints plain lines on a tty too for this half; only the tty `\r`-redraw half is not ported.
- `job_started` ports Portage's `_schedule_tasks_imp` `running = self._jobs` on task start: print the `>>> Jobs: 0 of N complete, 1 running` line before the worker thread prints its `>>> Emerging` line, so the order matches Portage's event order.
- `build_finished` ports Portage's `_build_exit` tail (`running = self._jobs` after the token release).
- `merge_finished` ports Portage's `_merge_exit` `curval += 1`.
- `job_failed` ports Portage's `_build_exit`/`_do_merge_exit` `failed = len(self._failed_pkgs)`.
- `reset_for_resume` (#444) restarts the counters over a resumed `--keep-going` pass (Portage resets `_pkg_count` and the status display in its resume loop); the live display stays live.

## fn jobs_line
- Backlog #197: ports Portage's `JobStatusDisplay._display_status` on a non-tty (`JobStatusDisplay.py`): `>>> Jobs: {cur} of {max} complete` plus the `, {r} running` / `, {f} failed` / `, {w} merge wait` arms only when nonzero, padded with spaces to the 68-column jobs field (Portage's `max_display_width - 32`). `self._jobs_column_width = width - 32` at the non-tty `width = max_display_width = 100`; the tty term-width half is cut (portuale always uses the non-tty rule). Portage appends `Load avg: {triple}` after the padding; portuale cuts it exactly like #185 cut the resolution seconds (repeated-run determinism is a jointly-owned gate; nondeterministic values ride `--json` only; coordinator ruling B14, batch-2026-09-27). The counters wear `INFORM`, like Portage's `number_style`; the padding is measured on the plain text, like Portage's `plain_output` gauge.
- `JobsLineArgs` / `jobs_line`: the pure shape, split out for unit tests. Portage's `Load avg:` trailer is cut (see `StatusDisplay::jobs_line`); the padding that precedes it is kept. The first status-test case is the n197 probe's s2 line up to the cut.

## fns emerging_line / installing_line / completed_line
- Backlog #197. `emerging_line` ports Portage's `_emerge/MergeListItem.py::_start` (`MergeListItem.py:60-85`): `{Emerging|Emerging binary|Fetching} ({cur} of {max}) {cpv}::{repo}`. Portage prints it via the scheduler's `statusMessage` path, which prefixes `>>> ` (`JobStatusDisplay._format_msg`) on stdout, hence the literal prefix here.
- `installing_line` ports Portage's `_emerge/PackageMerge.py::_start` (`PackageMerge.py:32-54`): `Installing ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}` when `ROOT != "/"` (note: `to`, not the `for` the Emerging line uses). Same colours as `emerging_line`. Portage suppresses it under `--fetchonly`/`--pretend`/`--buildpkgonly` (`_should_show_status`); portuale's merge functions never run in those modes, so no gate is needed at the call sites.
- `completed_line` ports Portage's `_emerge/PackageMerge.py::_install_exit` (`PackageMerge.py:56-79`): `Completed ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}` when `ROOT != "/"`. Same colours and suppression as `installing_line`.
- Colour escapes are `color.rs`'s port of `output.py:68-92` (`MERGE_LIST_PROGRESS` = yellow, `PKG_MERGE` = darkgreen, `PKG_BINARY_MERGE` = purple); only the two counters and the `cpv::repo` wear escapes, exactly Portage's `colorize()` call sites.
- `emerging_line` adds ` for {root}/` when `ROOT != "/"`. The counters wear `MERGE_LIST_PROGRESS`, the `cpv::repo` wears `PKG_MERGE` (source) or `PKG_BINARY_MERGE` (binary); everything else is uncoloured. `version` is the entry's merge version.
- Backlog #566: all three lines render the root through `root_with_trailing_slash` (exactly one trailing slash, idempotent). Portage needs no such helper: `LocationsManager.set_root_override` normalises every non-`/` ROOT to `rstrip(sep) + sep` at config load (`_config/LocationsManager.py:383-386`), so `pkg.root` (EROOT) and `settings['ROOT']` always carry the slash on every merge path (`PackageMerge._make_msg` covers Installing/Completed on the source, binary and getbinpkg paths; `MergeListItem._start` covers Emerging/Fetching). Live proof: `probe-merge.sh` `14-merge-match` Portage output prints `for <root>/` + `to <root>/` on Emerging/Installing/Completed (re-captured 2026-10-26 in `/var/tmp/t566`, tail byte-identical after the fix).

## merge-wait scope plumbing (#512)
- `--merge-wait-scope`: which merges barrier on idle builders. `Deep` is Portage's default.

## fn fetching_line (trailing slash history)
- The `Fetching ({cur} of {max}) {cpv}::{repo}[ for {root}]` line read `[ for {root}]` before the #566 trailing-slash fix; it now renders `[ for {root}/]` like the other progress lines.
