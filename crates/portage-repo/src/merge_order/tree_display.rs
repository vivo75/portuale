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

/// The pending-`Replacement`-row scan [`tree_solved_replacements`] and
/// [`tree_display_order`] share: unsatisfied `Replacement` arms paired
/// with the merge-bound replacement entry that solves them.
pub(crate) fn tree_stuck_pending(entries: &[GraphEntry]) -> Vec<PendingUninstall> {
    let mut pending = Vec::new();
    for (owner, entry) in entries.iter().enumerate() {
        for (blocker, b) in entry.blockers.iter().enumerate() {
            if b.unsolvable {
                continue;
            }
            let Some(crate::BlockerSatisfiedBy::Replacement { cp, slot }) = &b.satisfied_by else {
                continue;
            };
            let Some(repl) = entries.iter().position(|e| {
                (e.category.as_str(), e.package.as_str()) == (cp.0.as_str(), cp.1.as_str())
                    && e.slot.as_deref() == Some(slot.as_str())
                    && crate::merge_bound_version(&e.outcome)
                        .is_some_and(|v| v != b.matched_version.as_str())
            }) else {
                continue;
            };
            pending.push(PendingUninstall {
                repl,
                inst_cp: (b.matched_category.clone(), b.matched_package.clone()),
                inst_slot: slot.clone(),
                inst_version: b.matched_version.clone(),
                owner,
                blocker,
                strong: b.strong,
                node: None,
            });
        }
    }
    pending
}

/// The leftover weave-back [`serialize_merge_order`] and
/// [`tree_display_order`] share: entries the scheduler never saw (pruned
/// "nomerge" roots, entries with no node at all) rejoin on their unbiased
/// discovery rank, so such an entry is never bias-promoted past a merge
/// task it was behind in plain discovery order.
pub(crate) fn weave_scheduled_leftovers(
    scheduled: &[usize],
    real_n: usize,
    discovery_rank: &[usize],
) -> Vec<usize> {
    let placed: HashSet<usize> = scheduled.iter().copied().collect();
    let mut leftover: Vec<usize> = (0..real_n).filter(|i| !placed.contains(i)).collect();
    leftover.sort_by_key(|&i| discovery_rank[i]);

    let mut out: Vec<usize> = Vec::with_capacity(real_n);
    let mut ti = 0;
    for &m in scheduled {
        while ti < leftover.len() && discovery_rank[leftover[ti]] < discovery_rank[m] {
            out.push(leftover[ti]);
            ti += 1;
        }
        out.push(m);
    }
    out.extend(&leftover[ti..]);
    debug_assert_eq!(out.len(), real_n);
    out
}

/// Arguments of [`tree_solved_replacements`].
pub(crate) struct TreeSolvedReplacementsArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) top_level_atoms: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) implicit_system_deps: bool,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) dynamic_deps: bool,
    pub(crate) circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

/// Which satisfied Replacement rows tree-mode serialization solves.
///
/// Runs the shared scheduler loop in tree mode (greedy pop disabled, one
/// non-root leaf per round) with the stuck branch enabled, and returns
/// the solved `(owner entry, blocker index)` rows. Returns empty without
/// running the loop when no Replacement row is pending.
pub(crate) fn tree_solved_replacements(
    args: TreeSolvedReplacementsArgs<'_>,
) -> Vec<(usize, usize)> {
    let TreeSolvedReplacementsArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    } = args;
    let pending = tree_stuck_pending(entries);
    if pending.is_empty() {
        return Vec::new();
    }
    let (ext, mut g, _real_n, _discovery_rank) = schedule_graph(ScheduleGraphArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    });
    // `pending` addresses resolver indexing, which is a prefix of `ext`
    // (the closure only appends), so indices stay valid.
    let _ = &ext;
    let mut ts = TreeStuck {
        pending,
        scheduled: HashSet::new(),
        solved: Vec::new(),
        progressed: false,
    };
    select_nodes(&mut g, &ext, root, Some(&mut ts));
    ts.solved
}

/// Arguments of [`tree_display_order`].
pub struct TreeDisplayOrderArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub top_level_atoms: &'a [String],
    pub config: &'a portage_profile::Config,
    pub root: &'a Path,
    pub implicit_system_deps: bool,
    pub repos: &'a [RepoConfig],
    pub dynamic_deps: bool,
    pub circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

/// The tree-mode serialization order for `--tree` display.
///
/// Returns the merge-direction entry order (resolver indexing, like
/// [`serialize_merge_order`]); the display layer reverses it before
/// walking, exactly like Portage.
/// Portage: depgraph.py, _ordered_tree_display.
///
/// Unlike [`tree_solved_replacements`], the loop always runs but only the
/// order is kept -- verdicts stay that function's sole source.
pub fn tree_display_order(args: TreeDisplayOrderArgs<'_>) -> Vec<usize> {
    let TreeDisplayOrderArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    } = args;
    let pending = tree_stuck_pending(entries);
    let (ext, mut g, real_n, discovery_rank) = schedule_graph(ScheduleGraphArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    });
    // `pending` addresses resolver indexing, which is a prefix of `ext`
    // (the closure only appends), so indices stay valid.
    let mut ts = TreeStuck {
        pending,
        scheduled: HashSet::new(),
        solved: Vec::new(),
        progressed: false,
    };
    let scheduled: Vec<usize> = select_nodes(&mut g, &ext, root, Some(&mut ts))
        .into_iter()
        .filter(|&i| i < real_n)
        .collect();
    weave_scheduled_leftovers(&scheduled, real_n, &discovery_rank)
}
