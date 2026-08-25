//! Progress and status lines of the build scheduler.

use crate::emerge_build::*;

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
