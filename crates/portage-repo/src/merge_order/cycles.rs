//! Cycle search and the circular-dependency report.

use crate::merge_order::*;

/// Real `find_smallest_cycle`: the smallest `gather_deps` closure among
/// the currently-mergeable nodes, searched from the lowest
/// `ignore_priority` rung upward so as few dependencies as possible are
/// relaxed. "In the case of multiple runtime cycles, where some cycles
/// may depend on smaller independent cycles, it's optimal to merge
/// smaller independent cycles before other cycles that depend on them."
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn find_smallest_cycle(
    g: &Digraph,
    frontier: Option<&mut SerializeFrontier>,
    entries: &[GraphEntry],
    range: &PriorityRange,
    asap: &[usize],
    prefer_asap: bool,
    // #142 S2 F4b: nodes temporarily ineligible for harvesting —
    // owners waiting on their unadmitted removal (real's validation
    // edge keeps them out of the leaf set, so real's cycle search
    // never sees them either). Empty everywhere else, including all
    // unit tests.
    ineligible: &HashSet<usize>,
) -> Option<(HashSet<usize>, Option<Ignore>)> {
    let mergeable: HashSet<usize> = leaves_via(frontier, g, range.ig_medium())
        .into_iter()
        .filter(|i| !ineligible.contains(i))
        .collect();
    if mergeable.is_empty() {
        return None;
    }
    let mut cand: Vec<usize> = if prefer_asap && !asap.is_empty() {
        asap.iter()
            .copied()
            .filter(|i| mergeable.contains(i))
            .collect()
    } else {
        mergeable.iter().copied().collect()
    };
    // Real "Sort nodes for deterministic results" -- `sorted(nodes)`,
    // i.e. `Package.__lt__`: `cp` then version.
    cand.sort_by(|&a, &b| {
        (entries[a].category.as_str(), entries[a].package.as_str())
            .cmp(&(entries[b].category.as_str(), entries[b].package.as_str()))
            .then_with(|| {
                let ver = |i: usize| entry_version(&entries[i]).unwrap_or_default().to_string();
                portage_versions::vercmp(&ver(a), &ver(b))
                    .unwrap_or(0)
                    .cmp(&0)
            })
    });
    let mut best: Option<(HashSet<usize>, Option<Ignore>)> = None;
    for i in range.medium_post..=range.medium_soft {
        let ig = range.ig(i);
        for &node in &cand {
            if !g.has_parents(node) {
                continue;
            }
            let Some(cl) = gather_deps(g, node, ig, &mergeable) else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| cl.len() < b.len()) {
                best = Some((cl, ig));
            }
        }
        // "Exit this loop with the lowest possible priority, which
        // minimizes the use of installed packages to break cycles."
        if best.is_some() {
            break;
        }
    }
    best
}

/// Real `_serialize_tasks`' own final block for a multi-node cycle:
/// harvest one node at a time from the induced subgraph, always taking a
/// leaf at the lowest `ignore_priority` that produces one, and
/// preferring an installed leaf "in order to avoid merging something too
/// early as in bug 917259".
pub(crate) fn harvest_cycle(g: &Digraph, sub: &HashSet<usize>) -> Vec<usize> {
    let ladder: Vec<Option<Ignore>> = NORMAL
        .ignore
        .iter()
        .chain(SATISFIED.ignore.iter())
        .filter(|f| f.is_some())
        .copied()
        .collect();
    let mut remaining = sub.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = Vec::new();
        for ig in &ladder {
            leaves = g
                .order
                .iter()
                .copied()
                .filter(|i| {
                    remaining.contains(i)
                        && !g.children[*i].iter().any(|(c, prios)| {
                            remaining.contains(c) && ig.is_none_or(|f| prios.iter().any(|p| !f(p)))
                        })
                })
                .collect();
            if !leaves.is_empty() {
                break;
            }
        }
        if leaves.is_empty() {
            // Real `leaves = [cycle_digraph.order[-1]]`.
            leaves = vec![
                *g.order
                    .iter()
                    .rev()
                    .find(|i| remaining.contains(i))
                    .expect("remaining is non-empty"),
            ];
        }
        let installed_leaves: Vec<usize> =
            leaves.iter().copied().filter(|&i| g.installed[i]).collect();
        let pick = *installed_leaves.first().unwrap_or(&leaves[0]);
        remaining.remove(&pick);
        out.push(pick);
    }
    out
}

/// Real `depgraph._serialize_tasks`' own selection loop, restricted to
/// what a `--pretend` merge graph contains: no `Uninstall` tasks, no
/// blocker nodes (portuale renders blockers separately), and therefore
/// none of real's `myblocker_uninstalls` scheduling. What remains is the
/// whole of real's leaf-selection machinery:
///
/// * the `NONE .. MEDIUM_SOFT` `ignore_priority` ladder, with real's
///   "greedily pop all of these nodes since no relationship has been
///   ignored" batch at rung `NONE` and one-node-at-a-time selection
///   (preferring a node that actually has a parent) at every relaxed rung;
/// * `asap_nodes` -- real's `PDEPEND`-promotion path (bug #180045): every
///   unsatisfied `runtime_post` child freed by a relaxed-rung selection
///   is queued to be merged as soon as it becomes a leaf, ahead of the
///   ordinary bias order, and is scanned under `DepPrioritySatisfiedRange`;
/// * `find_smallest_cycle` for a genuine runtime cycle, with the
///   `drop_satisfied` escalation to `DepPrioritySatisfiedRange` that lets
///   an already-installed dependency break one.
///
/// Port of real `portage.util.digraph.get_cycles` (+ `shortest_path` +
/// `bfs`): every node's shortest child→node paths (all ties), each a
/// recorded cycle. Real feeds this the stuck remainder with the
/// `medium_soft` rung and counts the records (`large_cycle_count =
/// len(cycles) > 3`); rotations of one ring count separately (a square
/// records four), which is exactly what makes the count a "lot of
/// cycles" signal rather than a ring census.
///
/// `ig` is the survival filter (real's `ignore_priority`); pass `None`
/// for the unfiltered graph. Nodes with no path back to themselves
/// contribute nothing. Only the count and the member union are consumed
/// downstream, so traversal order is fixed (sorted indices) for
/// determinism -- real keeps every tie, which makes its result set
/// order-independent too (same reasoning as its PYTHONHASHSEED comment).
pub(crate) fn elementary_cycles(g: &Digraph, ig: Option<Ignore>) -> Vec<Vec<usize>> {
    use std::collections::VecDeque;
    /// BFS shortest path, real `digraph.shortest_path`: first visit wins
    /// per node (BFS order), returned on first reaching `end`. `None`
    /// when `end` is unreachable (real raises `KeyError` for unknown
    /// nodes instead -- every node here is known by construction).
    fn shortest_path(
        g: &Digraph,
        start: usize,
        end: usize,
        ig: Option<Ignore>,
    ) -> Option<Vec<usize>> {
        let mut paths: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut queue: VecDeque<(Option<usize>, usize)> = VecDeque::from([(None, start)]);
        let mut enqueued: HashSet<usize> = HashSet::from([start]);
        while let Some((parent, n)) = queue.pop_front() {
            let mut path = match parent {
                None => Vec::new(),
                Some(p) => paths.get(&p).cloned().unwrap_or_default(),
            };
            path.push(n);
            paths.insert(n, path);
            if n == end {
                return paths.remove(&end);
            }
            let mut fresh: Vec<usize> = g.children[n]
                .iter()
                .filter(|(c, prios)| {
                    !enqueued.contains(c) && ig.is_none_or(|f| prios.iter().any(|p| !f(p)))
                })
                .map(|(c, _)| *c)
                .collect();
            fresh.sort_unstable();
            for c in fresh {
                enqueued.insert(c);
                queue.push_back((Some(n), c));
            }
        }
        None
    }

    let mut all: Vec<Vec<usize>> = Vec::new();
    for &node in &g.order {
        // Shortest child→node path length seen for this node; every
        // path tied at the minimum is recorded (real appends paths not
        // longer than the running minimum, then filters by the final
        // minimum -- same set, same child order).
        let mut min_len: Option<usize> = None;
        let mut cands: Vec<Vec<usize>> = Vec::new();
        for (child, prios) in &g.children[node] {
            if !ig.is_none_or(|f| prios.iter().any(|p| !f(p))) {
                continue;
            }
            let Some(path) = shortest_path(g, *child, node, ig) else {
                continue;
            };
            if min_len.is_none_or(|m| path.len() <= m) {
                if min_len.is_none_or(|m| path.len() < m) {
                    min_len = Some(path.len());
                }
                cands.push(path);
            }
        }
        if let Some(m) = min_len {
            all.extend(cands.into_iter().filter(|p| p.len() == m));
        }
    }
    all
}

/// Port of real `circular_dependency_handler._prepare_reduced_merge_list`
/// over an explicit drain set: leaf-drain (no filter, like real's plain
/// `leaf_nodes()`), falling back to the lowest-order remaining node when
/// nothing is a leaf. Real drains its whole stuck remainder -- cycle
/// members plus everything left unscheduled downstream of them; the
/// caller (`cycle_report`) passes exactly that set: members plus
/// transitive requirers. A drained child frees its parents (checked
/// against the shrinking remainder, not the static set), matching
/// real's shrinking copy.
pub(crate) fn reduced_merge_order(
    g: &Digraph,
    drain: &HashSet<usize>,
    late: &[bool],
) -> Vec<usize> {
    // A node is a leaf when it has no still-remaining child in the
    // drain set -- self-edges count, exactly like real's plain
    // `leaf_nodes()` on its shrinking copy (an unsatisfied buildtime
    // self-loop is the only self-edge `build_digraph` keeps). Checking
    // a static set instead would pin every ring member forever, since
    // a drained child must free its parents.
    let mut remaining: HashSet<usize> = drain.clone();
    let mut out: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let mut leaves: Vec<usize> = g
            .order
            .iter()
            .copied()
            .filter(|i| {
                remaining.contains(i) && !g.children[*i].iter().any(|(c, _)| remaining.contains(c))
            })
            .collect();
        if leaves.is_empty() {
            // Real `node = tempgraph.order[0]` -- lowest-order remaining
            // node in insertion order (the fallback only fires inside
            // a ring, where every remaining node has a remaining child).
            // Backlog #245: a cp that lives in both roots has its
            // running-root (BDEPEND build-tool) instance inserted after
            // the target-root one in real's graph, so among the remaining
            // nodes the target-root ones go first; within a root the
            // scheduler's own insertion order stands (`g.order`). `late`
            // is `GraphEntry::targets_running_root` per node (empty = all
            // target-rooted, the single-root case, which changes nothing).
            let pos: HashMap<usize, usize> =
                g.order.iter().enumerate().map(|(p, &i)| (i, p)).collect();
            leaves = remaining
                .iter()
                .copied()
                .min_by_key(|&i| {
                    (
                        late.get(i).copied().unwrap_or(false),
                        pos.get(&i).copied().unwrap_or(usize::MAX),
                    )
                })
                .into_iter()
                .collect();
        }
        for i in leaves {
            remaining.remove(&i);
            out.push(i);
        }
    }
    out
}

/// The `medium_soft` rung of the satisfied range -- the filter real's
/// cycle handler passes to `get_cycles`. (Kept as an accessor because
/// `PriorityRange`/`SATISFIED` stay private to this module.)
pub(crate) fn satisfied_medium_soft_rung() -> Option<Ignore> {
    SATISFIED.ig_medium_soft()
}

/// Cycle report over a freshly built scheduling graph: all elementary
/// cycles (entry indices) plus the reduced display order. The drain set
/// is the cycle members plus everything transitively requiring them --
/// real drains its whole stuck remainder (members plus everything left
/// unscheduled downstream); portuale schedules everything, so the
/// downstream cone is re-derived here from `required_by` (cp-level, like
/// everywhere else). Built on demand -- callers only pay for it when a
/// hard cycle was already reported (the only path that consumes either
/// number). `top_level_atoms`/`root` feed `build_digraph` exactly as
/// `serialize_merge_order` passes them.
pub(crate) fn cycle_report(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let g = build_digraph(entries, top_level_atoms, root, circular);
    let cycles = elementary_cycles(&g, satisfied_medium_soft_rung());
    let members: HashSet<usize> = cycles.iter().flatten().copied().collect();
    let mut cp_indices: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        cp_indices
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(i);
    }
    let mut drain: HashSet<usize> = members.clone();
    let mut stack: Vec<usize> = members.iter().copied().collect();
    while let Some(i) = stack.pop() {
        for owner in &entries[i].required_by {
            if let Some(idxs) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str())) {
                for &j in idxs {
                    if drain.insert(j) {
                        stack.push(j);
                    }
                }
            }
        }
    }
    let late: Vec<bool> = entries.iter().map(|e| e.targets_running_root).collect();
    let display = reduced_merge_order(&g, &drain, &late);
    (cycles, display)
}
