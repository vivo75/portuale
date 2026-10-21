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
/// One deliberate cut: the `--pretend`/`--fetchonly`/`--fetch-all-uri`
/// exemption (those modes never reach this code). Interactive packages are
/// handled by [`apply_interactive_tasks`].
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

/// The scheduler-display predicate over the CLI flags. `jobs` is the
/// `--jobs` value (`usize::MAX` is a bare `-j`); `quiet_build` is the
/// `--quiet-build` tri-state (`Some(true)` is bare/`=y`).
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

/// The interactive-package half of `Scheduler._background_mode`: a
/// background run holding a merge entry with `PROPERTIES=interactive`
/// falls back to the foreground, lists the packages, and drops a parallel
/// `--jobs` to 1. Returns the effective `--jobs` and mode; prints the notice
/// on stdout like Portage's `writemsg_level` at INFO.
// Portage: Scheduler.py, _background_mode / _get_interactive_tasks
pub(crate) fn apply_interactive_tasks(
    mode: StatusMode,
    jobs: usize,
    entries: &[GraphEntry],
    root: &std::path::Path,
    color: &crate::color::Colorizer,
) -> (StatusMode, usize) {
    let interactive: Vec<&GraphEntry> = entries
        .iter()
        .filter(|e| e.interactive && entry_counts_toward_progress(e))
        .collect();
    if !mode.background || interactive.is_empty() {
        return (mode, jobs);
    }
    print!(
        "{}",
        interactive_notice(&interactive, jobs > 1, root, color)
    );
    (
        StatusMode {
            background: false,
            show_jobs: false,
        },
        if jobs > 1 { 1 } else { jobs },
    )
}

/// The notice text of [`apply_interactive_tasks`].
pub(crate) fn interactive_notice(
    interactive: &[&GraphEntry],
    parallel: bool,
    root: &std::path::Path,
    color: &crate::color::Colorizer,
) -> String {
    let mut out =
        String::from(">>> Sending package output to stdio due to interactive package(s):\n\n");
    for entry in interactive {
        let version = scheduler_cp_version(entry)
            .map(|(_, v)| v)
            .unwrap_or_default();
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        out.push_str(&format!("  {}", color.c("INFORM", &cpv)));
        if root != std::path::Path::new("/") {
            out.push_str(&format!(" for {}", root.display()));
        }
        out.push('\n');
    }
    out.push('\n');
    if parallel {
        out.push_str(
            ">>> Setting --jobs=1 due to the above interactive package(s)\n\
             >>> In order to temporarily mask interactive updates, you may\n\
             >>> specify --accept-properties=-interactive\n",
        );
    }
    out
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

    /// Task start: print the `>>> Jobs:` line *before* the worker prints its
    /// `>>> Emerging` line, matching Portage's event order.
    pub(crate) fn job_started(&self) {
        self.state.lock().unwrap().running += 1;
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// Build done: the build slot is free, the merge is pending.
    pub(crate) fn build_finished(&self) {
        let mut state = self.state.lock().unwrap();
        state.running = state.running.saturating_sub(1);
        drop(state);
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// Merge landed: `curval += 1`.
    pub(crate) fn merge_finished(&self) {
        self.state.lock().unwrap().curval += 1;
        if self.mode.show_jobs {
            self.print_jobs();
        }
    }

    /// A resumed `--keep-going` pass restarts the counters over what is
    /// left, like Portage resetting `_pkg_count`/`_status_display` in its
    /// resume loop. The live display (once a `>>> Jobs:` line printed)
    /// stays live.
    // Portage: Scheduler.py, _merge (resume loop)
    pub(crate) fn reset_for_resume(&self, maxval: usize) {
        let mut state = self.state.lock().unwrap();
        state.maxval = maxval;
        state.curval = 0;
        state.running = 0;
        state.failed = 0;
    }

    /// Failure: record it for the display.
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
    let action = if entry.source == CandidateSource::Binary {
        "Emerging binary"
    } else {
        "Emerging"
    };
    progress_line(ProgressLine {
        action,
        entry,
        version,
        progress,
        root,
        color,
    })
}

/// `Fetching ({cur} of {max}) {cpv}::{repo}[ for {root}]`, the `--fetchonly` job's status line.
// Portage: _emerge/EbuildBuild.py, _start (the fetchonly arm)
pub(crate) fn fetching_line(
    entry: &GraphEntry,
    version: &str,
    progress: mrg_director::MergeProgress,
    root: &Path,
    color: &crate::color::Colorizer,
) -> String {
    progress_line(ProgressLine {
        action: "Fetching",
        entry,
        version,
        progress,
        root,
        color,
    })
}

/// The values [`progress_line`] reads from its caller.
struct ProgressLine<'a> {
    action: &'a str,
    entry: &'a GraphEntry,
    version: &'a str,
    progress: mrg_director::MergeProgress,
    root: &'a Path,
    color: &'a crate::color::Colorizer,
}

fn progress_line(args: ProgressLine<'_>) -> String {
    let ProgressLine {
        action,
        entry,
        version,
        progress,
        root,
        color,
    } = args;
    let binary = entry.source == CandidateSource::Binary;
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
