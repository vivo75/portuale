# crates/portuale/src/emerge_build/tests/status.rs
Progress-line and scheduler-display unit tests.

## fn merge_progress_map_counts_only_merging_entries_in_order
- Backlog #177: the merge progress counters, Portage's `Scheduler._pkg_count` (`Scheduler.py:296-304`): only merging outcomes count, and each merging entry's `cur` is its 1-based ordinal in merge order. The map is positional (`out[i]` belongs to `entries[i]`), so two merging entries for the same cat/pkg (two slots of one package in a single run) keep their own `cur` (review of #177). Grounded in the cited Portage source (there is no Python mirror); the `testrepo` repo name is this module's own `source_entry` fixture value.

## fn merge_progress_lines_match_portage_exact_text
- Backlog #177: `Emerging`/`Emerging binary` from Portage's `_emerge/MergeListItem.py::_start` (`MergeListItem.py:60-85`), `Installing`/`Completed` from Portage's `_emerge/PackageMerge.py` (`PackageMerge.py:32-79`). `ROOT != "/"` appends ` for {root}` (Emerging) vs ` to {root}` (Installing/Completed). Colour escapes are `color.rs`'s port of `output.py:68-92` (`MERGE_LIST_PROGRESS` = yellow `\x1b[33;01m`, `PKG_MERGE` = darkgreen `\x1b[32m`, `PKG_BINARY_MERGE` = purple `\x1b[35m`, reset `\x1b[39;49;00m`); only the two counters and the `cpv::repo` wear escapes, exactly Portage's `colorize()` call sites.

## fn scheduler_status_mode_matches_portages_background_gates
- Backlog #197: ports Portage's `Scheduler._background_mode` (`Scheduler.py:470-531`): the serial shapes stay foreground with no Jobs display, anything parallel/quiet/captured goes background, and `--quiet` without `--verbose` kills the Jobs display but keeps the (unblanked) status lines. `--quiet-build=y` is Portage's `myopts.get("--quiet-build") == "y"` arm; bare `-j` is Portage's `max_jobs is True`. A one-package mergelist without quiet resets `max_jobs` to 1 and leaves background mode. The `--quiet --verbose` serial case is the n197 probe's s3 shape.

## fn jobs_line_matches_portages_display_status_shape
- Backlog #197: ports Portage's `JobStatusDisplay._display_status` on a non-tty. Portage's `Load avg:` trailer is cut (coordinator ruling B14; see `StatusDisplay::jobs_line`); the first case is the n197 probe's s2 line up to the cut (including all 36 pad spaces). Coloured: the padding is measured on the plain text (Portage's `plain_output` gauge).

## fn status_display_counters_feed_the_jobs_line
- Backlog #197: dispatch / freed build slot / landed merge / failure map to Portage's `_schedule_tasks_imp` / `_build_exit` / `_merge_exit`. The line is deterministic (no live trailer, B14).

## fn merge_progress_roots_carry_portages_trailing_slash (#566)
- Portage prints its normalised ROOT (exactly one trailing slash; `LocationsManager.set_root_override`) on the merge progress lines, via `pkg.root` on every merge path. Oracle: the `probe-merge.sh` `14-merge-match` Portage output (`LLM/evidence/2026-10-23-509/probe-merge.sh`).
