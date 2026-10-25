//! The up-front `pkg_pretend` pass shared by every merge plan.
//!
//! Each merging entry's `pretend` runs before anything builds or merges
//! (Portage fetches remote binaries here too; portuale still defers that
//! to the merge). With one job each entry's block replays before the next
//! starts; with more, the outputs buffer and replay in merge-list order.
//! A failure aborts before anything builds.

use crate::emerge_build::*;

/// One entry of the pass, with the resolve context its gate needs.
pub(crate) trait PretendEntry: Sync {
    /// The underlying merge-plan entry.
    fn entry(&self) -> &GraphEntry;
    /// The version this entry would merge, or `None` for an outcome the
    /// pass never runs for.
    fn version(&self) -> Option<&str> {
        entry_version(self.entry())
    }
    /// Whether this entry belongs in the pass.
    fn needs_pretend(&self) -> bool;
    /// This entry's per-package `PORTAGE_TMPDIR` (a missing matched
    /// directory fails the entry, like the build would).
    fn entry_tmpdir(&self) -> Result<PathBuf, String>;
    /// Clean, run the captured `pkg_pretend`, compose the `${T}` log. A
    /// failing pretend still hands back its log, so the runner replays
    /// the buffered output before recording the failure.
    fn run_pretend(&self) -> PretendRun;
}

/// One entry's pretend run: the replayable `${T}` log with the status
/// line first (`None` when the entry stays silent, a remote binary with
/// no file) plus the entry's outcome. The log replays whether the
/// pretend succeeded or failed; the failure still aborts the pass
/// before anything builds.
pub(crate) struct PretendRun {
    /// The composed log for the runner to replay and remove.
    pub log: Option<PathBuf>,
    /// The entry's failure (unresolvable entry, `clean` or `pretend`
    /// phase failure, log setup failure).
    pub result: Result<(), String>,
}

/// Arguments of [`run_pretend_pass`].
pub(crate) struct RunPretendPassArgs<'a, E: PretendEntry> {
    /// The plan's entries (or one wrapper per entry).
    pub entries: &'a [E],
    /// `--jobs` after the interactive-package fallback: `usize::MAX` is a
    /// bare `--jobs`, like Portage's `True`.
    pub jobs: usize,
    /// The replay sink is stdout: `pkg_pretend` output bypasses
    /// `--quiet`/`--jobs`, like Portage.
    pub replay: &'a mut dyn std::io::Write,
}

/// Portage's `max(1, min(max_jobs, len(pretend_pkgs))`: a bare `--jobs`
/// (`True`) becomes the entry count, anything else is clamped to `1..=len`.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend
pub(crate) fn pretend_max_jobs(jobs: usize, pending: usize) -> usize {
    jobs.min(pending).max(1)
}

/// The up-front `pkg_pretend` pass: every entry the filter keeps runs, and
/// the outputs replay in merge-list order. Returns the failing entry
/// indices with their messages; a failure aborts before anything builds.
/// At one job each block replays before the next entry starts; above one
/// job the blocks buffer and replay at the end, like Portage's
/// unbuffered/buffered split.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend
pub(crate) fn run_pretend_pass<E: PretendEntry>(
    args: RunPretendPassArgs<'_, E>,
) -> Vec<(usize, String)> {
    let RunPretendPassArgs {
        entries,
        jobs,
        replay,
    } = args;
    let pending: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].needs_pretend())
        .collect();
    if pending.is_empty() {
        return Vec::new();
    }
    let max_jobs = pretend_max_jobs(jobs, pending.len());
    if max_jobs == 1 {
        // Portage's unbuffered path (`buffered = max_jobs > 1`): each
        // entry's block is out before the next entry starts, in
        // merge-list order. The phases still capture to a file (that is
        // what the runner replays); only the timing is live.
        let mut failures = Vec::new();
        for idx in pending {
            let run = entries[idx].run_pretend();
            if let Some(log) = run.log {
                replay_pretend_log(&log, replay);
            }
            if let Err(e) = run.result {
                failures.push((idx, e));
            }
        }
        return failures;
    }
    let mut failures: Vec<(usize, String)> = Vec::new();
    let mut logs: std::collections::HashMap<usize, std::path::PathBuf> =
        std::collections::HashMap::new();
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, PretendRun)>();
        let mut next = 0usize;
        let mut in_flight = 0usize;
        while next < pending.len() || in_flight > 0 {
            while next < pending.len() && in_flight < max_jobs {
                let idx = pending[next];
                next += 1;
                in_flight += 1;
                let entry = &entries[idx];
                let tx = tx.clone();
                scope.spawn(move || {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        entry.run_pretend()
                    }))
                    .unwrap_or_else(|payload| PretendRun {
                        log: None,
                        result: Err(worker_panic_message(entry.entry(), payload)),
                    });
                    // A closed receiver means the pass already gave up; the
                    // entry still ran to completion first.
                    let _sent = tx.send((idx, r));
                });
            }
            let Ok((idx, run)) = rx.recv() else { break };
            in_flight -= 1;
            if let Some(log) = run.log {
                logs.insert(idx, log);
            }
            if let Err(e) = run.result {
                failures.push((idx, e));
            }
        }
    });
    failures.sort_by_key(|(idx, _)| *idx);
    // Replay in merge-list order, then remove the capture, like Portage
    // reading and unlinking each buffered pretend log.
    let mut ordered = pending;
    ordered.sort_unstable();
    for idx in ordered {
        if let Some(path) = logs.remove(&idx) {
            replay_pretend_log(&path, replay);
        }
    }
    failures
}

/// A clean-proof staging capture for one pretend: `mkstemp` under the run
/// `${PORTAGE_TMPDIR}` root, which no entry `clean` or builddir reset
/// touches. The caller composes the `${T}` log from it, then removes it.
pub(crate) fn stage_pretend_capture(portage_tmpdir: &Path) -> Result<PathBuf, String> {
    let dir = portage_tmpdir.to_path_buf();
    mkstemp_in(portage_tmpdir, "pretend-stage-", ".log").and_then(|staged| {
        staged
            .keep()
            .map(|(_file, path)| path)
            .map_err(|e| format!("keeping the pretend staging in {}: {e}", dir.display()))
    })
}

/// `mkstemp` in `dir`: O_EXCL, mode 0600, unguessable name. Portage's
/// `tempfile.mkstemp` equivalent for the pass's logs.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one
fn mkstemp_in(dir: &Path, prefix: &str, suffix: &str) -> Result<tempfile::NamedTempFile, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// Compose one entry's final pretend log in its `${T}`: the status line
/// first, then the staged capture. A planted symlink at the old
/// predictable path is never touched: the name is unguessable and
/// creation fails instead of following a link.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one
pub(crate) fn compose_pretend_log(
    t_base: &Path,
    entry: &GraphEntry,
    cp: &str,
    version: &str,
    staged: &[u8],
) -> Result<PathBuf, String> {
    let t_dir = t_base
        .join("portage")
        .join(&entry.category)
        .join(format!("{}-{version}", entry.package))
        .join("temp");
    let mut log = mkstemp_in(&t_dir, "pretend-", ".pretend.log")?;
    use std::io::Write as _;
    let status = format!(">>> Running pre-merge checks for {cp}-{version}\n");
    log.write_all(status.as_bytes())
        .and_then(|()| log.write_all(staged))
        .map_err(|e| format!("{}: {e}", t_dir.display()))?;
    log.keep()
        .map(|(_file, path)| path)
        .map_err(|e| format!("keeping the pretend log in {}: {e}", t_dir.display()))
}

/// Arguments of [`run_staged_pretend`].
pub(crate) struct RunStagedPretendArgs<'a> {
    /// The merge-plan entry, for the `${T}` layout.
    pub entry: &'a GraphEntry,
    /// `category/package`.
    pub cp: &'a str,
    /// The version being pretended.
    pub version: &'a str,
    /// The run `${PORTAGE_TMPDIR}` root the staging capture lives under.
    pub portage_tmpdir: &'a Path,
    /// The entry's per-package `PORTAGE_TMPDIR` base of its `${T}`.
    pub t_base: &'a Path,
    /// The captured pretend run writing to the staging path.
    pub run: &'a dyn Fn(&Path) -> Result<(), String>,
}

/// The stage/run/read/remove/compose sequence shared by the source and
/// binary pretend impls: the phases capture to a staging file outside
/// the builddir (both resets wipe `${T}`), whose bytes compose the
/// entry's `${T}` log after the run. The log comes back whether the
/// pretend succeeded or failed, so the runner replays a failing entry's
/// buffered output before recording the failure; a log setup failure is
/// reported as the failure, after the phase error when both exist.
// Portage: _emerge/Scheduler.py, _run_pkg_pretend_one
pub(crate) fn run_staged_pretend(args: RunStagedPretendArgs<'_>) -> PretendRun {
    let RunStagedPretendArgs {
        entry,
        cp,
        version,
        portage_tmpdir,
        t_base,
        run,
    } = args;
    let staging = match stage_pretend_capture(portage_tmpdir) {
        Ok(staging) => staging,
        Err(e) => {
            return PretendRun {
                log: None,
                result: Err(e),
            };
        }
    };
    let outcome = run(&staging);
    let staged = std::fs::read(&staging).unwrap_or_default();
    let _staging_removed = std::fs::remove_file(&staging);
    match compose_pretend_log(t_base, entry, cp, version, &staged) {
        Ok(log) => PretendRun {
            log: Some(log),
            result: outcome,
        },
        Err(compose_err) => match outcome {
            Ok(()) => PretendRun {
                log: None,
                result: Err(compose_err),
            },
            Err(phase_err) => PretendRun {
                log: None,
                result: Err(format!("{phase_err}\n{compose_err}")),
            },
        },
    }
}

/// Replay one pretend log to `replay` and remove it, like Portage reading
/// and unlinking each buffered pretend log.
// Portage: _emerge/Scheduler.py, _read_pretend_log
fn replay_pretend_log(path: &Path, replay: &mut dyn std::io::Write) {
    if let Ok(bytes) = std::fs::read(path)
        && !bytes.is_empty()
    {
        // A closed stdout must not fail a pass whose pretends succeeded.
        let _written = replay.write_all(&bytes);
        let _flushed = replay.flush();
    }
    let _removed = std::fs::remove_file(path);
}

/// Arguments of [`finish_failed_pretend`].
pub(crate) struct FinishFailedPretendArgs<'a, 'b> {
    /// What the loops report a failure through.
    pub report: &'a FailReport<'b>,
    /// The plan's entries, for the failure reports.
    pub entries: &'a [GraphEntry],
    /// The pass's failures, in merge-list order.
    pub failures: Vec<(usize, String)>,
    /// `--keep-going`.
    pub keep_going: bool,
}

/// The one `pretend_failed → ledger.finish` block shared by all four
/// pretend call sites: `Ok(false)` when the pass stayed clean (the caller
/// continues), `Ok(true)` to stop with the default report, `Err` with the
/// message the caller aborts with.
pub(crate) fn finish_failed_pretend(args: FinishFailedPretendArgs<'_, '_>) -> Result<bool, String> {
    let FinishFailedPretendArgs {
        report,
        entries,
        failures,
        keep_going,
    } = args;
    if failures.is_empty() {
        return Ok(false);
    }
    let mut ledger = Ledger::default();
    let mut unreported = Vec::new();
    for (idx, e) in failures {
        if !ledger.failed(report, &entries[idx], &e) {
            unreported.push(e);
        }
    }
    let first = (!unreported.is_empty()).then(|| unreported.join("\n"));
    match ledger.finish(report, keep_going, first) {
        Ok(()) => Ok(true),
        Err(message) => Err(message),
    }
}
