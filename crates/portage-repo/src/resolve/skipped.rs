//! Skipped-update records and their grouping.

use crate::*;

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
