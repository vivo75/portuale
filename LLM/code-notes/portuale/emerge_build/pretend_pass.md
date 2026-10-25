# crates/portuale/src/emerge_build/pretend_pass.rs
The up-front `pkg_pretend` pass shared by every merge plan (scheduler
`-jN`, serial `-j1`, `--buildpkgonly`, mixed source+binary): one generic
runner over a per-entry trait, with the resolve context carried by two
thin wrappers (`SourcePretendEntry` in `scheduler.rs`,
`MixedPretendEntry` in `emerge_getbinpkg.rs`).

## trait PretendEntry
- One entry of the pass with the resolve context its gate needs.
  `needs_pretend` is the whole pending filter (source: build-bound plus
  the md5-cache `DEFINED_PHASES` gate; mixed: `pretend_needs_run` plus
  the source gate or the local-binpkg gate). `entry_tmpdir` is the
  entry's per-package `PORTAGE_TMPDIR` root and stays fallible: a
  missing matched directory fails the entry like the build would
  (Portage `_check_temp_dir`). `version` defaults to the one shared
  `entry_version` extraction. `Sync` for the scoped spawn.
- `run_pretend` does the whole per-entry sequence and hands the runner
  a `PretendRun`: the final `${T}` log to replay and remove, plus the
  entry's outcome. The log comes back whether the pretend succeeded or
  failed, so the runner replays a failing entry's buffered output
  before recording the failure (Portage's `_emit_pkg_pretend_result`,
  `Scheduler.py:1008-1034`, prints `result.output` and then records the
  failure; `_run_pkg_pretend_one`'s finally block,
  `Scheduler.py:1316-1325`, reads the capture whatever the return
  code). `log` is `None` only when the entry stays silent (a remote
  binary with no local file, filtered from pending anyway) or when the
  log itself could not be set up. `result` is the entry's failure
  (unresolvable entry, `clean`/`pretend` phase failure, log setup
  failure); when composing the log fails after a phase failure, both
  messages report with the phase error first.
- The per-kind impls stay next to their runners and share
  `run_staged_pretend`: source reuses `pretend_one_entry` unchanged,
  mixed reuses `pretend_one_mixed_entry` unchanged (source arm
  delegates to the source impl).

## struct RunPretendPassArgs, fn run_pretend_pass
- Ports Portage's `_run_pkg_pretend` (`Scheduler.py:946`): the pending
  filter, the `1..=len` clamp, one job sequential with per-entry replay,
  more jobs scoped-threaded with end replay in merge-list order plus
  unlinking (`Scheduler.py:1008-1044`, `_emit_pkg_pretend_result` /
  `_read_pretend_log`). Returns the failing indices with their messages;
  a failure aborts before anything builds. `catch_unwind` per worker
  reports a panicking pretend thread instead of hanging the pass.
- At one job each entry's block replays before the next entry starts.
  The phases still capture to a file (that is what the runner replays);
  only the timing is live, which is also what makes the order
  unit-testable through the replay sink. The final bytes match the
  buffered run exactly, so the contract suite pins no new output shape.
  Portage's true unbuffered streaming (`buffered = max_jobs > 1`,
  `Scheduler.py:974`, phases writing straight to stdout) is the one
  deliberate mechanism cut: identical text, no stdio interleave.

## fn pretend_max_jobs
- Portage's `max(1, min(max_jobs, len(pretend_pkgs)))`
  (`Scheduler.py:961-964`): a bare `--jobs` (`True`) becomes the entry
  count, anything else is clamped to `1..=len`.

## struct PretendRun, struct RunStagedPretendArgs, fn run_staged_pretend
- `PretendRun{log,result}` is what each entry hands the runner: the
  replayable log plus the outcome, kept separate so a failure still
  replays. `run_staged_pretend` is the one stage/run/read/remove/compose
  sequence both impls call (entry, `cp`, version, run
  `${PORTAGE_TMPDIR}`, entry `${T}` base, and the captured runner as
  `&dyn Fn(&Path) -> Result<(), String>`): staging under the run root
  stays clean-proof for both arms, compose failures report after the
  phase error when both exist.
- `mkstemp_in` is Portage's `tempfile.mkstemp`
  (`Scheduler.py:1290`): `O_EXCL`, mode 0600, unguessable name.
- `compose_pretend_log` derives the entry's `${T}`
  (`<tmp>/portage/<cat>/<pf>/temp`, like the phases do), creates the
  log there with Portage's `.pretend.log` suffix, and writes the
  `>>> Running pre-merge checks for <cpv>` status line first, then the
  staged capture -- so the line heads every block in merge-list order
  (Portage's `_status_msg("Running pre-merge checks for ...")`,
  `Scheduler.py:1085`, which portuale's established contract text
  renders as this line heading each replayed block).
- `stage_pretend_capture` exists because both resets wipe the entry's
  builddir: the source `clean` phase removes `${T}` (Portage
  `bin/phase-functions.sh`, `__dyn_clean`), and the binary hook path
  removes the whole builddir first (`pretend_binary_entry`). A log
  created in `${T}` before the run would be deleted with the status
  line in it and recreated without `O_EXCL`, so the phases capture to a
  staging file under the run `${PORTAGE_TMPDIR}` root (clean-proof for
  both arms) and the compose step moves those bytes into the fresh
  `${T}` log afterwards. Same bytes as a direct capture, in the same
  order (status, `clean`, `pretend`).

## fn replay_pretend_log
- Reads one log into the sink, flushes, removes it -- Portage's
  `_read_pretend_log` (`Scheduler.py:1036`). Best-effort throughout: a
  closed stdout must not fail a pass whose pretends all succeeded.

## struct FinishFailedPretendArgs, fn finish_failed_pretend
- The one `pretend_failed -> ledger.finish` block shared by all four
  pretend call sites: feeds every failure through the run's `FailReport`
  (`ledger.failed`), then `ledger.finish`. `Ok(false)` when the pass
  stayed clean (the caller continues its pass), `Ok(true)` to stop with
  the default report, `Err` with the message the caller aborts with.
