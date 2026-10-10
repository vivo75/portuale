//! Progress and status lines of the build scheduler.

use crate::emerge_build::*;

/// `(cat/pkg, version)` for an entry the scheduler will build -- the
/// same outcome→version mapping `merge_one_source_entry` does inline.
pub(crate) fn scheduler_cp_version(entry: &GraphEntry) -> Result<(String, String), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        // #72 B3: unreachable via `scheduler_needs_build` (a removal is
        // never scheduled); kept explicit so no wildcard hides it.
        PretendOutcome::AlreadyInstalled { .. }
        | PretendOutcome::NoVisibleCandidate
        | PretendOutcome::Uninstall { .. } => {
            return Err(format!("{cp}: not a buildable entry"));
        }
    };
    Ok((cp, version))
}

/// Whether the scheduler must actually run a build for this entry (a
/// `New`/`Upgrade`/`Downgrade`/`Reinstall` from source). `AlreadyInstalled`
/// and `Binary` entries are treated as already satisfied.
pub(crate) fn scheduler_needs_build(entry: &GraphEntry) -> bool {
    entry.source != CandidateSource::Binary
        && matches!(
            entry.outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
                | PretendOutcome::Reinstall { .. }
        )
}

/// Whether this entry counts toward real's merge progress counters --
/// real `Scheduler.py:296-304` counts only `operation == "merge"`
/// mergelist entries (uninstalls and nomerge nodes are not counted and
/// print no progress line). Portuale's equivalents are the outcomes that
/// actually merge: `New`/`Reinstall`/`Upgrade`/`Downgrade`.
/// `AlreadyInstalled`/`Uninstall`/`NoVisibleCandidate` neither count nor
/// print, exactly like the mergelist entries real skips in
/// `MergeListItem._start` (`pkg.installed` returns early).
pub(crate) fn entry_counts_toward_progress(entry: &GraphEntry) -> bool {
    matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Reinstall { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
    )
}

/// Every merging entry's own real `(curval, maxval)` snapshot (real
/// `Scheduler._pkg_count`, copied per `MergeListItem` at task creation
/// after `curval += 1`, `Scheduler.py:2273`/`2355`): `max` is the count
/// of merging entries, `cur` the 1-based ordinal in merge order. One
/// snapshot **per entry position** (`out[i]` belongs to `entries[i]`; a
/// non-merging entry carries `MergeProgress::single()`, which it never
/// prints) -- keyed positionally, not by `(category, package)`, so two
/// merging entries for the same cat/pkg in one run (e.g. two slots of
/// one package) keep their own `cur` instead of collapsing onto one map
/// slot (review of backlog #177). Callers pass `progress[i]` into the
/// merge functions, so the run-level loops own the only scan.
///
/// Narrowing: real assigns `curval` in task-start order, which under
/// `--jobs` can differ from merge-list order when independent packages
/// start out of order. Portuale's scheduler dispatches in index order
/// (the `find` scans `0..n`), so list ordinal and start ordinal agree
/// whenever dependencies force the order -- and in the common serial
/// case they are identical by construction. Real also recomputes
/// `maxval` per `--keep-going` pass (`Scheduler.py:1307-1312`); portuale
/// keeps the run's original total (a failure-path-only difference).
pub(crate) fn merge_progress_map(entries: &[GraphEntry]) -> Vec<mrg_director::MergeProgress> {
    let max = entries
        .iter()
        .filter(|e| entry_counts_toward_progress(e))
        .count();
    let mut cur = 0usize;
    entries
        .iter()
        .map(|entry| {
            if entry_counts_toward_progress(entry) {
                cur += 1;
                mrg_director::MergeProgress { cur, max }
            } else {
                mrg_director::MergeProgress::single()
            }
        })
        .collect()
}

/// The colouriser for the merge progress lines: real `actions.py:2816-
/// 2828` resolves module-global `havecolor` from `--color y|n` over
/// `NO_COLOR`/`NOCOLOR`/`TERM`/isatty (`color::resolve_havecolor` ports
/// exactly that gate). The merge path threads no `--color` value down
/// (unlike the pretend renderer, which takes `color_opt`), so this
/// resolves the ambient default -- colour on a tty, off when piped.
/// Either way the line *shape* is real's; only the escapes differ.
pub(crate) fn progress_color() -> crate::color::Colorizer {
    crate::color::Colorizer::new(crate::color::resolve_havecolor(None))
}

/// Backlog #197: real `Scheduler._background_mode`
/// (`3rdparty/portage/lib/_emerge/Scheduler.py:470-531`) as a value.
/// `background` is real `Scheduler._background`: a parallel `--jobs`
/// run, `--quiet`, or `--quiet-build=y` -- except that a one-package
/// mergelist without `--quiet`/`--quiet-build=y` resets to serial and
/// non-background (real also resets `max_jobs` there; portuale keeps
/// its dispatch and only takes the display half -- a scheduled build
/// under `-jN` still captures its phase output). `show_jobs` is real
/// `JobStatusDisplay.quiet == false`, i.e. `background && (!quiet ||
/// verbose)` (`Scheduler.py:524-527`): `--quiet` without `--verbose`
/// suppresses the `>>> Jobs:` lines but not the status lines
/// themselves. Two deliberate cuts, both stated: interactive packages
/// (real forces non-background with a `>>> Sending package output to
/// stdio` notice; portuale models no `interactive` PROPERTIES) and the
/// `--pretend`/`--fetchonly`/`--fetch-all-uri` exemption (those modes
/// never reach the merge code that reads this value).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StatusMode {
    /// Real `Scheduler._background`: status lines print with no leading
    /// blank, and build output is captured, not streamed.
    pub background: bool,
    /// Real `JobStatusDisplay` is live: `>>> Jobs:` lines print.
    pub show_jobs: bool,
}

impl StatusMode {
    /// A mode for unit tests: serial, non-background, no Jobs lines.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        StatusMode {
            background: false,
            show_jobs: false,
        }
    }
}

/// Backlog #197: real's scheduler-display predicate. `jobs` is the CLI
/// `--jobs` value (`usize::MAX` is a bare `-j`, i.e. real's `max_jobs
/// is True`); `quiet` is `--quiet`; `quiet_build` is the `--quiet-build`
/// tri-state (`Some(true)` is bare/`=y` -- real `main.py` normalizes
/// both to `"y"` before `_background_mode` reads it, so bare counts);
/// `mergelist_len` is the resolved entry count; `verbose` is
/// `--verbose`.
pub(crate) fn scheduler_status_mode(
    jobs: usize,
    quiet: bool,
    quiet_build: Option<bool>,
    mergelist_len: usize,
    verbose: bool,
) -> StatusMode {
    let background_quiet = quiet || quiet_build == Some(true);
    let mut background = jobs > 1 || background_quiet;
    if mergelist_len <= 1 && !background_quiet {
        background = false;
    }
    StatusMode {
        background,
        show_jobs: background && (!quiet || verbose),
    }
}

/// Backlog #197: real `_emerge/JobStatusDisplay.py` (the `>>> Jobs:`
/// line) plus the `Scheduler._status_msg` half (`Scheduler.py:2383-
/// 2397`) that prefixes every status line with a blank line unless the
/// scheduler is in background mode. One value per merge run, shared by
/// reference (the `-jN` scheduler prints from worker threads, so the
/// counters live behind a mutex; every `println!` still happens under
/// its own call, exactly like the existing merge-path prints).
///
/// Real's `maxval` counts every `operation == "merge"` mergelist entry
/// (`Scheduler.py:296-304`); portuale's is the build-bound count (see
/// `scheduler_needs_build`) -- the two agree whenever every entry
/// builds, and the installed-noop counting quirk is out of scope.
/// Real's `merge_wait` arm is implemented but always reads 0 here:
/// portuale serializes every vdb merge immediately behind its build and
/// models neither the merge-wait queue nor `FEATURES=merge-wait`.
pub(crate) struct StatusDisplay {
    pub(crate) mode: StatusMode,
    pub(crate) color: crate::color::Colorizer,
    pub(crate) state: std::sync::Mutex<StatusState>,
}

pub(crate) struct StatusState {
    pub(crate) maxval: usize,
    pub(crate) curval: usize,
    pub(crate) running: usize,
    pub(crate) failed: usize,
    /// A `>>> Jobs:` line has been printed: real's `displayMessage`
    /// re-displays the current status after every status line once one
    /// is on screen (`JobStatusDisplay.displayMessage`, verified in the
    /// n197 probe: each `Emerging`/`Installing`/`Completed` is followed
    /// by a `>>> Jobs:` repeat).
    pub(crate) displayed: bool,
}

impl StatusDisplay {
    pub(crate) fn new(mode: StatusMode, maxval: usize, color: crate::color::Colorizer) -> Self {
        StatusDisplay {
            mode,
            color,
            state: std::sync::Mutex::new(StatusState {
                maxval,
                curval: 0,
                running: 0,
                failed: 0,
                displayed: false,
            }),
        }
    }

    /// A display for unit tests: non-background, no Jobs lines, so
    /// `status` prints exactly `"\n{line}\n"` under test capture.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        StatusDisplay::new(
            StatusMode {
                background: false,
                show_jobs: false,
            },
            1,
            crate::color::Colorizer::new(false),
        )
    }

    /// Real `Scheduler._status_msg` + `JobStatusDisplay.displayMessage`
    /// (non-tty half): the leading blank unless in background mode, the
    /// `>>> ` line itself on stdout, then the current `>>> Jobs:` status
    /// again when the display is live and showing (the n197 probe shows
    /// the repeat after every status line; real's tty `\r`-redraw half
    /// is not ported -- portuale prints plain lines on a tty too).
    pub(crate) fn status(&self, line: &str) {
        if !self.mode.background {
            println!();
        }
        println!("{line}");
        let redisplay = self.mode.show_jobs && self.state.lock().unwrap().displayed;
        if redisplay {
            self.print_jobs();
        }
    }

    /// Real `_schedule_tasks_imp`'s `running = self._jobs` on task
    /// start: print the `>>> Jobs: 0 of N complete, 1 running` line
    /// *before* the worker thread prints its `>>> Emerging` line, so
    /// the order matches real's event order.
    pub(crate) fn job_started(&self) {
        self.state.lock().unwrap().running += 1;
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// Real `_build_exit`'s tail (`running = self._jobs` after the
    /// token release): the build slot is free, the merge is pending.
    pub(crate) fn build_finished(&self) {
        let mut state = self.state.lock().unwrap();
        state.running = state.running.saturating_sub(1);
        drop(state);
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// Real `_merge_exit`'s `curval += 1`: the merge landed.
    pub(crate) fn merge_finished(&self) {
        self.state.lock().unwrap().curval += 1;
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// Real `_build_exit`/`_do_merge_exit`'s `failed =
    /// len(self._failed_pkgs)`: record a failure on the display.
    pub(crate) fn job_failed(&self) {
        self.state.lock().unwrap().failed += 1;
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    pub(crate) fn print_jobs(&self) {
        println!("{}", self.jobs_line());
        self.state.lock().unwrap().displayed = true;
    }

    /// Real `JobStatusDisplay._display_status` on a non-tty
    /// (`JobStatusDisplay.py`): `>>> Jobs: {cur} of {max} complete`
    /// plus the `, {r} running` / `, {f} failed` / `, {w} merge wait`
    /// arms only when nonzero, padded with spaces to the 68-column jobs
    /// field (real's `max_display_width - 32` at the non-tty width of
    /// 100; the tty term-width half is cut -- portuale always uses the
    /// non-tty rule, stated here). Real appends `Load avg: {triple}`
    /// after the padding; portuale cuts it exactly like #185 cut the
    /// resolution seconds (repeated-run determinism is a jointly-owned
    /// gate; nondeterministic values ride `--json` only -- coordinator
    /// ruling B14, batch-2026-09-27). The counters wear `INFORM`, like
    /// real's `number_style`; the padding is measured on the plain text,
    /// like real's `plain_output` gauge.
    pub(crate) fn jobs_line(&self) -> String {
        let state = self.state.lock().unwrap();
        jobs_line(JobsLineArgs {
            color: &self.color,
            curval: state.curval,
            maxval: state.maxval,
            running: state.running,
            failed: state.failed,
            merge_wait: 0,
        })
    }
}

/// Arguments of [`jobs_line`].
pub(crate) struct JobsLineArgs<'a> {
    pub(crate) color: &'a crate::color::Colorizer,
    pub(crate) curval: usize,
    pub(crate) maxval: usize,
    pub(crate) running: usize,
    pub(crate) failed: usize,
    pub(crate) merge_wait: usize,
}

/// Backlog #197: the pure `>>> Jobs:` shape (real
/// `JobStatusDisplay._display_status`), split out for unit tests.
/// `merge_wait` is always 0 from portuale (see [`StatusDisplay`]); the
/// arm stays because it is part of the shape. Real's `Load avg:`
/// trailer is cut (see [`StatusDisplay::jobs_line`]); the padding that
/// precedes it is kept, so the line keeps real's shape and column.
pub(crate) fn jobs_line(args: JobsLineArgs<'_>) -> String {
    let JobsLineArgs {
        color,
        curval,
        maxval,
        running,
        failed,
        merge_wait,
    } = args;
    let mut plain = format!("Jobs: {curval} of {maxval} complete");
    let mut styled = format!(
        "Jobs: {} of {} complete",
        color.c("INFORM", &curval.to_string()),
        color.c("INFORM", &maxval.to_string()),
    );
    for (value, label) in [
        (running, "running"),
        (failed, "failed"),
        (merge_wait, "merge wait"),
    ] {
        if value > 0 {
            plain.push_str(&format!(", {value} {label}"));
            styled.push_str(&format!(
                ", {} {label}",
                color.c("INFORM", &value.to_string()),
            ));
        }
    }
    // Real `self._jobs_column_width = width - 32` at the non-tty
    // `width = max_display_width = 100`. The padding stays even though
    // real's `Load avg:` trailer is cut (see `jobs_line`'s doc
    // comment), so the line keeps real's shape and column.
    let padding = 68usize.saturating_sub(plain.len());
    styled.push_str(&" ".repeat(padding));
    format!(">>> {styled}")
}

/// Real `_emerge/MergeListItem.py::_start` (`MergeListItem.py:60-85`):
/// `{Emerging|Emerging binary|Fetching} ({cur} of {max}) {cpv}::{repo}`,
/// plus ` for {root}` when `ROOT != "/"`. The counters wear
/// `MERGE_LIST_PROGRESS`, the `cpv::repo` wears `PKG_MERGE` (source) or
/// `PKG_BINARY_MERGE` (binary); everything else is uncoloured. Real
/// prints it via the scheduler's `statusMessage` path, which prefixes
/// `>>> ` (`JobStatusDisplay._format_msg`) on stdout -- hence the
/// literal prefix here. `version` is the entry's merge version.
pub(crate) fn emerging_line(
    entry: &GraphEntry,
    version: &str,
    progress: mrg_director::MergeProgress,
    root: &Path,
    color: &crate::color::Colorizer,
) -> String {
    let binary = entry.source == CandidateSource::Binary;
    let action = if binary {
        "Emerging binary"
    } else {
        "Emerging"
    };
    let pkg_color = if binary {
        "PKG_BINARY_MERGE"
    } else {
        "PKG_MERGE"
    };
    let mut line = format!(
        ">>> {action} ({} of {}) {}",
        color.c("MERGE_LIST_PROGRESS", &progress.cur.to_string()),
        color.c("MERGE_LIST_PROGRESS", &progress.max.to_string()),
        color.c(
            pkg_color,
            &format!(
                "{}/{}-{version}::{}",
                entry.category,
                entry.package,
                entry_repo(entry)
            )
        ),
    );
    if root.as_os_str() != "/" {
        line.push_str(&format!(" for {}", root.display()));
    }
    line
}

/// Real `_emerge/PackageMerge.py::_start` (`PackageMerge.py:32-54`):
/// `Installing ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}` when
/// `ROOT != "/"` (note: `to`, not the `for` the Emerging line uses).
/// Same colours as [`emerging_line`]. Real suppresses it under
/// `--fetchonly`/`--pretend`/`--buildpkgonly` (`_should_show_status`);
/// portuale's merge functions never run in those modes, so no gate is
/// needed at the call sites.
pub(crate) fn installing_line(
    entry: &GraphEntry,
    version: &str,
    progress: mrg_director::MergeProgress,
    root: &Path,
    color: &crate::color::Colorizer,
) -> String {
    let binary = entry.source == CandidateSource::Binary;
    let pkg_color = if binary {
        "PKG_BINARY_MERGE"
    } else {
        "PKG_MERGE"
    };
    let mut line = format!(
        ">>> Installing ({} of {}) {}",
        color.c("MERGE_LIST_PROGRESS", &progress.cur.to_string()),
        color.c("MERGE_LIST_PROGRESS", &progress.max.to_string()),
        color.c(
            pkg_color,
            &format!(
                "{}/{}-{version}::{}",
                entry.category,
                entry.package,
                entry_repo(entry)
            )
        ),
    );
    if root.as_os_str() != "/" {
        line.push_str(&format!(" to {}", root.display()));
    }
    line
}

/// Real `_emerge/PackageMerge.py::_install_exit` (`PackageMerge.py:56-
/// 79`): `Completed ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}`
/// when `ROOT != "/"`. Same colours and suppression as
/// [`installing_line`].
pub(crate) fn completed_line(
    entry: &GraphEntry,
    version: &str,
    progress: mrg_director::MergeProgress,
    root: &Path,
    color: &crate::color::Colorizer,
) -> String {
    let binary = entry.source == CandidateSource::Binary;
    let pkg_color = if binary {
        "PKG_BINARY_MERGE"
    } else {
        "PKG_MERGE"
    };
    let mut line = format!(
        ">>> Completed ({} of {}) {}",
        color.c("MERGE_LIST_PROGRESS", &progress.cur.to_string()),
        color.c("MERGE_LIST_PROGRESS", &progress.max.to_string()),
        color.c(
            pkg_color,
            &format!(
                "{}/{}-{version}::{}",
                entry.category,
                entry.package,
                entry_repo(entry)
            )
        ),
    );
    if root.as_os_str() != "/" {
        line.push_str(&format!(" to {}", root.display()));
    }
    line
}
