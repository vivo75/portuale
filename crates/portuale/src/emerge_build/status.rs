//! Progress and status lines of the build scheduler.

use crate::emerge_build::*;

/// `(cat/pkg, version)` for an entry the scheduler will build.
pub(crate) fn scheduler_cp_version(entry: &GraphEntry) -> Result<(String, String), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        // A removal is never scheduled; kept explicit so no wildcard hides
        // it.
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

/// Whether this entry counts toward the merge progress counters: only the
/// outcomes that actually merge (`New`/`Reinstall`/`Upgrade`/`Downgrade`).
// Portage: Scheduler.py, _pkg_count
pub(crate) fn entry_counts_toward_progress(entry: &GraphEntry) -> bool {
    matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Reinstall { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
    )
}

/// Every merging entry's own `(curval, maxval)` snapshot: `max` is the count
/// of merging entries, `cur` the 1-based ordinal in merge order. One snapshot
/// per entry position, so two merging entries for the same cat/pkg keep their
/// own `cur`.
///
/// Narrowing: Portage assigns `curval` in task-start order, which under
/// `--jobs` can differ from merge-list order; portuale dispatches in index
/// order, so the two agree whenever dependencies force the order. Portage
/// also recomputes `maxval` per `--keep-going` pass; portuale keeps the
/// run's original total.
// Portage: Scheduler.py, _pkg_count
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

/// The colouriser for the merge progress lines. The merge path resolves the
/// ambient default (colour on a tty, off when piped); the line *shape* is
/// Portage's either way, only the escapes differ.
pub(crate) fn progress_color() -> crate::color::Colorizer {
    crate::color::Colorizer::new(crate::color::resolve_havecolor(None))
}

/// Portage's `Scheduler._background_mode` as a value: `background` is a
/// parallel `--jobs` run, `--quiet`, or `--quiet-build=y` (except that a
/// one-package mergelist without `--quiet`/`--quiet-build=y` resets to
/// serial and non-background); `show_jobs` is `background &&
/// (!quiet || verbose)`.
///
/// Two deliberate cuts: interactive packages (portuale models no
/// `interactive` PROPERTIES) and the `--pretend`/`--fetchonly`/
/// `--fetch-all-uri` exemption (those modes never reach this code).
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

/// The `>>> Jobs:` line plus the `Scheduler._status_msg` half that prefixes
/// every status line with a blank line unless in background mode. One value
/// per merge run, shared by reference (the `-jN` scheduler prints from worker
/// threads, so the counters live behind a mutex).
///
/// `maxval` is the build-bound count here (see `scheduler_needs_build`);
/// the `merge-wait` arm is implemented but always reads 0 (portuale
/// serializes every vdb merge and models no merge-wait queue).
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
    /// A `>>> Jobs:` line has been printed: the current status re-displays
    /// after every status line once one is on screen.
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

    /// The leading blank unless in background mode, the `>>> ` line itself,
    /// then the current `>>> Jobs:` status again when the display is live.
    /// (The tty `\r`-redraw half is not ported.)
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

    pub(crate) fn print_jobs(&self) {
        println!("{}", self.jobs_line());
        self.state.lock().unwrap().displayed = true;
    }

    /// The `>>> Jobs: {cur} of {max} complete` line plus the `running` /
    /// `failed` / `merge wait` arms only when nonzero, padded to the
    /// 68-column jobs field. Portage's `Load avg:` trailer is cut (kept out
    /// of stdout for run determinism); the padding that precedes it is kept,
    /// so the line keeps Portage's shape and column.
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

/// The pure `>>> Jobs:` shape, split out for unit tests. `merge_wait` is
/// always 0 from portuale (see [`StatusDisplay`]); the arm stays because it
/// is part of the shape.
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
    // The padding stays even though the `Load avg:` trailer is cut, so the
    // line keeps Portage's shape and column.
    let padding = 68usize.saturating_sub(plain.len());
    styled.push_str(&" ".repeat(padding));
    format!(">>> {styled}")
}

/// `{Emerging|Emerging binary} ({cur} of {max}) {cpv}::{repo}`, plus
/// ` for {root}` when `ROOT != "/"`. The counters wear
/// `MERGE_LIST_PROGRESS`, the `cpv::repo` wears `PKG_MERGE` (source) or
/// `PKG_BINARY_MERGE` (binary). `version` is the entry's merge version.
// Portage: MergeListItem.py, _start
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

/// `Installing ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}` when
/// `ROOT != "/"` (note: `to`, not the `for` the Emerging line uses).
/// Same colours as [`emerging_line`].
// Portage: PackageMerge.py, _start
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

/// `Completed ({cur} of {max}) {cpv}::{repo}`, plus ` to {root}` when
/// `ROOT != "/"`. Same colours and suppression as [`installing_line`].
// Portage: PackageMerge.py, _install_exit
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
