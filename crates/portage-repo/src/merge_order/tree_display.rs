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

/// The stuck branch: schedule one pending uninstall.
///
/// Among pending rows whose uninstall is not yet scheduled, whose
/// installed instance is not itself a graph node, and whose blocker is
/// soft, takes the first in entry order. Creates the synthetic uninstall
/// node as the replacement's parent with a hard edge, so the replacement
/// is picked single the next round while the uninstall itself stays
/// unselectable and dies unscheduled at the replacement's selection.
/// Portage: depgraph.py, tree serializer's choice loop.
pub(crate) fn tree_schedule_stuck(g: &mut Digraph, entries: &[GraphEntry], ts: &mut TreeStuck) {
    for p in ts.pending.iter_mut() {
        if p.node.is_some() {
            continue;
        }
        if p.strong {
            continue;
        }
        let walked_installed = entries.iter().any(|e| {
            (e.category.as_str(), e.package.as_str()) == (p.inst_cp.0.as_str(), p.inst_cp.1.as_str())
                && e.slot.as_deref() == Some(p.inst_slot.as_str())
                && matches!(&e.outcome, crate::PretendOutcome::AlreadyInstalled { version } if version == &p.inst_version)
        });
        if walked_installed {
            continue;
        }
        // Synthetic uninstall node: the replacement's parent with a hard
        // edge. The frontier never learns it; it dies unscheduled at the
        // replacement's selection.
        let u = g.n;
        g.n += 1;
        g.children
            .push(vec![(p.repl, vec![DepPriority::default()])]);
        g.parents.push(Vec::new());
        g.order.push(u);
        g.installed.push(true);
        g.alive.push(true);
        g.parents[p.repl].push(u);
        p.node = Some(u);
        ts.scheduled.insert(u);
        ts.progressed = true;
        return;
    }
}

/// Record solved verdicts at selection time.
///
/// Either the replacement merging or the uninstall node itself being
/// selected orders the same verdict.
/// Portage: depgraph.py, tree serializer's solved-blocker append.
pub(crate) fn tree_note_selection(
    g: &mut Digraph,
    entries: &[GraphEntry],
    ts: &mut TreeStuck,
    selected: usize,
) {
    let _ = entries;
    for p in &ts.pending {
        let Some(u) = p.node else {
            continue;
        };
        if !ts.scheduled.contains(&u) {
            continue;
        }
        if selected == p.repl || selected == u {
            if selected == p.repl {
                // The replacement merged on top: the uninstall task is gone.
                g.alive[u] = false;
            }
            if !ts.solved.contains(&(p.owner, p.blocker)) {
                ts.solved.push((p.owner, p.blocker));
            }
        }
    }
}
