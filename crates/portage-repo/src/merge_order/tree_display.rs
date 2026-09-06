//! `--tree` display order and stuck-node handling.

use crate::merge_order::*;

/// Returns the alive nodes in scheduling order (installed "nomerge"
/// nodes included -- the caller drops them).
///
/// Tree-mode stuck-branch state for [`select_nodes`]: when a round
/// selects nothing and blockers are pending, schedule an uninstall for
/// the blocked package and solve the row at a later selection.
/// Portage: depgraph.py, tree serializer's `scheduled_uninstalls`.
///
/// Portuale's scheduler graph has no uninstall nodes, so a scheduled
/// uninstall is a synthetic node: childless, with the blocked
/// (replacement) entry as its one parent and a hard (never-ignored)
/// edge. Selection and removal flow through the normal loop; only the
/// verdicts (`solved`) escape.
pub(crate) struct TreeStuck {
    /// One pending scheduling per unsatisfied Replacement row:
    /// the replacement entry, the installed instance it replaces,
    /// and the row's address for the verdict.
    pub(crate) pending: Vec<PendingUninstall>,
    /// Synthetic uninstall node indices already scheduled.
    pub(crate) scheduled: HashSet<usize>,
    /// Verdicts: `(owner entry, blocker index)` rows tree mode solves.
    pub(crate) solved: Vec<(usize, usize)>,
    /// Set by [`tree_schedule_stuck`] when it schedules something, so
    /// the loop resets state and retries selection like Portage's
    /// `continue`.
    pub(crate) progressed: bool,
}

/// One install-replacing satisfied row awaiting tree-mode scheduling.
#[derive(Debug)]
pub(crate) struct PendingUninstall {
    /// The merge-bound replacement entry.
    pub(crate) repl: usize,
    /// The installed instance it replaces (cp, slot, version).
    pub(crate) inst_cp: (String, String),
    pub(crate) inst_slot: String,
    pub(crate) inst_version: String,
    /// Verdict address.
    pub(crate) owner: usize,
    pub(crate) blocker: usize,
    /// v1 schedules soft (`!`) rows only; strong (`!!`) rows stay hidden.
    pub(crate) strong: bool,
    /// Synthetic uninstall node once scheduled.
    pub(crate) node: Option<usize>,
}
