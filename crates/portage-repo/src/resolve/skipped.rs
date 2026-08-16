//! Skipped-update records and their grouping.

use crate::*;

/// One withheld upgrade for Portage's skipped-update block: same shape for the
/// direct solve's removal rows and the reverse-pin withholds. Rendered by
/// `pretend.rs`; the rc stays 0 either way.
// Portage: depgraph.py, _conflict_missed_update
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedUpdate {
    pub category: String,
    pub package: String,
    pub slot: String,
    pub skipped_version: String,
    pub skipped_sub_slot: String,
    pub skipped_repo: String,
    pub skipped_use: Vec<(String, String)>,
    pub atom: String,
    pub consumer_cpv: String,
    pub consumer_installed: bool,
    pub consumer_use: Vec<(String, String)>,
    /// Whether the missed instance resolves in the running root.
    pub missed_targets_running_root: bool,
    /// Whether a merge consumer resolves in the running root.
    pub consumer_targets_running_root: bool,
}

/// One slot for the abbreviated skipped-update tail: a masked version higher
/// than the chosen one whose missing-dependency mask still bites at settle
/// time. Only the slot renders.
// Portage: depgraph.py, _show_missed_update (check_backtrack probe)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDep {
    pub category: String,
    pub package: String,
    pub slot: String,
}

/// One update for the full per-update form: a masked higher version whose
/// missing-dependency mask no longer bites at settle time. Renders the
/// `selected:` / `skipped:` rows plus the missing atom's disclosure, with the
/// chain pinned to the single skipped parent (no `(argument)` line).
// Portage: depgraph.py, _show_missed_update_unsatisfied_dep
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedMissingDepFull {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The selected entry's display row (merge-bound) or installed instance.
    pub selected_display: String,
    /// The masked higher version's display row (always merge-scheduled).
    pub skipped_display: String,
    /// The missing atom's disclosure: masked candidates, or the plain miss.
    pub dep: SkippedDepDisclosure,
}

/// The missing atom's disclosure inside [`SkippedMissingDepFull`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkippedDepDisclosure {
    Masked(MaskedDepReport),
    Plain(PlainMissDepReport),
}
