//! Slot conflicts: records, merging and rendering.

use crate::*;

/// A genuine slot conflict: two different atoms landing on the same
/// category/package/slot whose independently-resolved best versions are
/// incompatible. Purely informational ("report, don't enforce"): unlike
/// Portage's fatal depgraph error, portuale reports it and keeps the
/// first-resolved version. Different slots coexisting is not a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflict {
    pub category: String,
    pub package: String,
    pub slot: String,
    /// The version already resolved (and recursed into), by whichever atom
    /// reached this slot first.
    pub resolved_version: String,
    /// The atom text that does NOT accept `resolved_version`.
    pub conflicting_atom: String,
    /// Every distinct version pulled into the graph (>= 2) with its parents;
    /// `instances[0]` is always `resolved_version`.
    pub instances: Vec<SlotConflictInstance>,
}

/// One version of a slot-conflicted `category/package:slot`, plus its pullers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictInstance {
    pub version: String,
    pub sub_slot: String,
    pub repo_name: String,
    /// The `USE="…"` pairs for this instance's header line (empty renders
    /// `USE=""`); vdb-based for an installed instance.
    pub use_display: Vec<(String, String)>,
    pub parents: Vec<SlotConflictParent>,
    /// Portage's installed nomerge node: kept by a residual consumer pin,
    /// rendered `(… installed in '<root>')`. Always false for merge-vs-merge.
    pub installed: bool,
}

/// One parent that pulled a `SlotConflictInstance` into the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotConflictParent {
    /// `cat/pkg-ver:slot/sub_slot::repo`, or empty for a top-level atom.
    pub parent_cpv: String,
    pub atom: String,
    /// The parent's own resolved `USE="…"` for its `required by` line; empty
    /// for a top-level `(Argument)` parent, vdb-based for an installed one.
    pub use_display: Vec<(String, String)>,
    /// An installed parent whose recorded atom the merge breaks.
    pub installed: bool,
}

/// Which conflict party is installed rather than merge-bound: `Existing` is
/// instance A (already in the graph), `Current` instance B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstalledSide {
    Existing,
    Current,
}
