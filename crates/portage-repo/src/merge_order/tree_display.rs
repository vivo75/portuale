//! `--tree` display order and stuck-node handling.

use crate::merge_order::*;

/// Returns the alive nodes in scheduling order (installed "nomerge"
/// nodes included -- the caller drops them).
/// Backlog #81: tree-mode stuck-branch state for [`select_nodes`].
///
/// Real's tree serializer keeps `scheduled_uninstalls` as live scheduler
/// state: when a round selects nothing and blockers are pending, it
/// schedules an uninstall for the blocked package, reverses the edges so
/// the blocked package merges on top of it, and appends the solved
/// blocker when the blocked package (or the uninstall itself) is later
/// selected (`depgraph.py:9998`, `:10190`, `:10315-10358`). The flat
/// greedy pop drains past the same shape, so the row stays hidden there.
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
    /// the loop resets state and retries selection like real's
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
    /// Real applies overlap guards to strong (`!!`) blockers
    /// (`forbid_overlap`, portage-essential); v1 schedules soft (`!`)
    /// rows only and leaves strong rows hidden (current behavior).
    pub(crate) strong: bool,
    /// Synthetic uninstall node once scheduled.
    pub(crate) node: Option<usize>,
}

/// The stuck branch: schedule one pending uninstall.
///
/// Mirrors real's choice loop in narrowed form: among pending rows whose
/// uninstall is not yet scheduled, whose installed instance is not
/// itself a graph node (real `digraph.contains(inst_pkg)` skips those),
/// and whose blocker is soft, take the first in entry order (real picks
/// by fewest parent-deps with an early break at one; single-pending
/// shapes -- every oracle here -- decide identically). Creates the
/// synthetic uninstall node as the replacement's parent with a hard edge:
/// real's edge reversal makes the merge node the uninstall's child
/// (`depgraph.py:10190` -- `mygraph.add(slot_node, uninst_task)`), so the
/// replacement is picked single the next round (it is the only node with
/// parents) while the uninstall itself stays unselectable (it has a
/// child) and dies unscheduled at the replacement's selection.
/// #131: the direction is load-bearing for the display order -- as the
/// replacement's child (the pre-#131 shape) the replacement rejoins a
/// later batch; as its parent the next round picks the replacement
/// alone, exactly like real's round-2 pick.
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
        // Synthetic uninstall node: the replacement's parent (a leaf
        // itself only until the replacement is picked -- it carries the
        // replacement as its one child). Appended to `order` like real's
        // `mygraph.add` appends new nodes. Never directly selectable: the
        // frontier never learns it (like the pre-#131 child shape, which
        // the frontier likewise never synced), and it dies unscheduled
        // at the replacement's selection (`tree_note_selection`).
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
/// Real appends a solved blocker when the blocked package merges while
/// its uninstall is scheduled, and discards it when the uninstall node
/// itself is selected (`:10315-10358`). Either selection orders the same
/// verdict here.
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
                // The replacement merged on top: the uninstall task is
                // gone (real `mygraph.remove`), like the flat loop's
                // removal site.
                g.alive[u] = false;
            }
            if !ts.solved.contains(&(p.owner, p.blocker)) {
                ts.solved.push((p.owner, p.blocker));
            }
        }
    }
}

/// The pending-`Replacement`-row scan [`tree_solved_replacements`] and
/// [`tree_display_order`] share: unsatisfied `Replacement` arms (the
/// uninstall-solving `Uninstall` arms and the unsolvable rows already
/// have display homes everywhere; strong blockers stay hidden per the v1
/// narrowing), each paired with the merge-bound replacement entry real's
/// solved-blocker append follows (`depgraph.py:10351-10358`).
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

/// Backlog #81 S1: which satisfied Replacement rows tree-mode
/// serialization solves.
///
/// Runs the shared scheduler loop in tree mode (greedy pop disabled,
/// one non-root leaf per round) with the stuck branch enabled, and
/// returns the solved `(owner entry, blocker index)` rows. Callers pass
/// resolver-indexed `entries`; verdicts address the same indexing.
///
/// Shares `schedule_graph` (closure, prune, bias) and every selection
/// helper with the flat path -- the only differences from
/// [`serialize_merge_order`] are the `not tree_mode` gate and the stuck
/// branch, exactly like real. Returns empty without running the loop
/// when no Replacement row is pending (the common case pays nothing).
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
    // NOTE: `ext` may append synthetic closure entries; `pending`
    // addresses resolver indexing, which is a prefix of `ext` (the
    // closure only appends), so indices stay valid.
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

/// Backlog #131 S1: the tree-mode serialization order for `--tree`
/// display. Real's `_ordered_tree_display` walks the reversed tree-mode
/// retlist (`depgraph.py:10618-10619`, tree serializer with the greedy
/// leaf pop disabled, `:9764-9777`), not the flat merge order — the two
/// differ wherever one-at-a-time selection meets leaves in a different
/// sequence than the flat batch pop (p2b's `newpkg` interleaving,
/// slotusegroup's sibling order). Returns the merge-direction entry
/// order (resolver indexing, like [`serialize_merge_order`]); the
/// display layer reverses it before walking, exactly like real.
///
/// Unlike [`tree_solved_replacements`], the loop always runs (the order
/// is needed with or without pending rows) but only the order is kept —
/// verdicts stay that function's sole source, so this never changes
/// which rows solve. Unseen-by-the-scheduler entries rejoin through the
/// same leftover weave-back as the flat path.
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
    // NOTE: `ext` may append synthetic closure entries; `pending`
    // addresses resolver indexing, which is a prefix of `ext` (the
    // closure only appends), so indices stay valid.
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
