# crates/portuale/src/emerge_build/source_merge.rs
The source-merge loop: build, then merge, one entry at a time (serial), or via the parallel scheduler.

## struct RunSourceMergeArgs, fields capture_log / mode
- `capture_log` ports Portage's `Scheduler._background_mode`: redirect each package's build output to `${T}/build.log` instead of the terminal. Always true for `jobs` > 1 (a `-j` run must not interleave); otherwise driven by `--quiet-build=y` / `-q`. When set, the single-job path runs the same captured-build-then-serialized-merge split the scheduler uses.
- `mode` is backlog #197's scheduler-display mode (background + Jobs visibility). A serial captured run (`capture_log`) is always background (see `scheduler_status_mode`).

## fn run_source_merge
- Portuale's first source build-and-merge path for `emerge` itself. Runs the full `install` phase chain via `ebuild_merge::run_merge` (`pretend`→`setup`→…→`install` via embedded `brush` + `SRC_URI` fetch, then `merge_tree` + `pkg_preinst`/`pkg_postinst` + `env_update()`).
- `Upgrade`/`Downgrade`/`Reinstall`: `run_merge` merges the new version, then `ebuild_merge::unmerge_replaced_same_slot` (inside `merge_after_install`) unmerges the replaced same-slot version. That is Portage's `dblink.treewalk()` merge-then-unmerge order, with that version's own `pkg_prerm`/`pkg_postrm` run from its saved vdb environment.
- A `Binary` entry (only reachable with `--usepkg` without `--getbinpkg`) is a hard error; pass `--getbinpkg` for the mixed path (`emerge_getbinpkg::run_merge_plan`).
- One `JobStatusDisplay` per merge run (backlog #197); see `StatusDisplay` for the installed-noop narrowing of `maxval`.
- The `-jN` dispatch policy is one of the director's two `SchedulerPolicy` implementations: a bare `-j` runs uncapped (Portage's `max_jobs is True`), a numbered `--jobs=N` runs capped.
- The serial loop merges through the director's source engine (the same `SourceEngine::merge_entry` the mixed dispatcher runs per source unit) rather than calling `merge_one_source_entry` directly, so the merge-engine slot carries the production source-merge traffic.
- Per-run `Scheduler._pkg_count` (backlog #177): every entry prints its own positional snapshot.
- The captured serial split fires Portage's serial-background `>>> Jobs:` events (start, build-end, merge-land) around the same halves the `-jN` scheduler runs; on failure, Portage's `_build_exit` / `_merge_exit` failure arms (`Scheduler.py:1641-1658` / `:1543-1561`), each fired exactly once like the `-jN` scheduler.

## fn run_merge_loop
- Ports Portage's `Scheduler._calc_resume_list` for the `--keep-going` dependent-drop. Without `keep_going` stops at the first failure (`merge_one`'s `Err`), portuale's long-standing default. Portage's `emerge` also exits non-zero when anything failed under `--keep-going`.
- Each successful merge removes its own package (Portage `Scheduler.py:1595-1602`) from the up-front-saved resume list (and commits), so a SIGKILL leaves exactly the tail for `--resume`. A no-op "merge" (already installed) was never saved, so its removal is a silent no-op.
- A postinst failure (#444) drops nothing: the merge landed, so dependents stay satisfiable through the live vdb entry. The pass still re-plans (`resuming` banner, restarted counters, resume-list re-save via `resave_resume_list`, `Scheduler._save_resume_list`), and the failed entry is recorded in the pass report (world/resume exclusion downstream) but never in `gone` (so a later drop notice never cites it as a requirement).
- `merge_one_source_entry` is also the `Source`-entry arm of `emerge_getbinpkg::run_merge_plan` (`emerge --getbinpkg`'s mixed source+binary merge); the mixed dispatcher routes `Binary` entries to `merge_binpkg` before ever calling it.

## fn resave_resume_list (#444)
- Re-saves the resume list for a resumed `--keep-going` pass (favorites and options read back, mergelist replaced by the survivors), Portage's `_save_resume_list` after `_calc_resume_list`. A pass with nothing left keeps the per-merge shrink's tail (a lone or non-keep-going failure never re-saves, so the failed entry stays replayable, matching the pinned `--resume` shape).

## fn merge_one_source_entry
- A blocker-removal task is not a merge (backlog #72 B3). Executing it is a documented non-goal (`LLM/02.072-uninstall_merge_rows.md` section 7); the entry is skipped exactly like an installed no-op.
- Prints Portage's `MergeListItem._start` per-package line (backlog #177).
- The resolved `USE` for the entry lets `bin/ebuild.sh`'s `use()` (and every USE-conditional in the phases) see the flags; `phase_env_vars` would otherwise leave `USE=""`. Narrowing: the IUSE-declared enabled flags only (`GraphEntry::use_flags_display`), not the implicit/arch part of the effective set.
- Per-package `FEATURES` (backlog #98): this entry's gates (Rust-side `feature_enabled` and the vdb-environment regeneration) read the per-entry folded list; `set_resolved_features` re-derives the merge-time tokens.
- Per-package `PORTAGE_TMPDIR` (backlog #99).
- Pre-clean ports Portage's `_emerge/EbuildBuild._start_pre_clean` (`EbuildBuild.py:207-229`) and `Scheduler.py:969-981`/`:1119-1139`: the `clean` phase runs before every build, unconditionally (`noclean` only skips the *post*-merge clean). Without it a stale `.installed` marker or an already-`instprep`ped image from an earlier build in the same `${PORTAGE_BUILDDIR}` silently skips `install` (backlog #42, found in #38 S4).
- `Installing` ports Portage's `PackageMerge._start` per-package line (backlog #177), printed from `run_merge`'s pre-merge hook: Portage wraps the finished `EbuildBuild` in a `PackageMerge` (`Scheduler._build_exit`, `Scheduler.py:1615-1621`), so `Installing` lands after the build phase output (and the `--buildpkg` packaging) and before the `EbuildMerge` vdb merge, the same point the `-jN` scheduler's build/merge split prints it (before the serialized `run_qmerge`). A failed build never reaches the hook, so no `Installing` prints for it, exactly like Portage queuing no merge for a failed build.
- Prints Portage's `PackageMerge._install_exit` `Completed (N of M) cpv::repo` line (backlog #177) where portuale used to print its own `merged.` line.
- A postinst failure (#444) prints `Completed` anyway (Portage's `_install_exit` runs because `EbuildMerge` returns before setting `postinst_failure`) and returns the `postinst failed` marker instead of `merge failed`, skipping the world record below. The post-merge clean was already skipped by the merge itself (never after a postinst failure).
