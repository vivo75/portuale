//! Replaying `_create_graph` for fail-fast passes.

use crate::merge_order::*;
use crate::parse_dep_atom;

/// A `(category, package)` key.
pub(crate) type Cp = (String, String);

/// The USE state `replay_create_graph` needs for one entry: declared `IUSE`, the
/// enabled flags before any autounmask change, and the flags no change can
/// touch. `None` for entries whose USE cannot change (installed, binary, missing).
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayUse {
    pub iuse: HashSet<String>,
    pub enabled: HashSet<String>,
    pub fixed: HashSet<String>,
}

/// The first dependency the replayed walk fails on, as `(parent entry, dep index)`.
/// `contradiction` marks the autounmask shape (a flip Portage rejects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayFailure {
    pub parent: usize,
    pub edge: usize,
    pub contradiction: bool,
}

/// What `replay_create_graph` saw: admission order, the edge admitting each entry,
/// the first failing dependency, and the needed USE changes per `cat/pkg`.
#[derive(Debug, Clone, Default)]
pub(crate) struct WalkReplay {
    pub admitted: Vec<usize>,
    pub admitted_by: Vec<Option<(usize, usize)>>,
    /// For an argument entry: the index into `top_level_atoms` of the adding atom.
    pub admitted_arg: Vec<Option<usize>>,
    /// Every resolved `(parent, dep index, target)` edge, in walk order.
    pub edges: Vec<(usize, usize, usize)>,
    /// Per `cat/pkg`, the `(parent, dep index)` first needing each USE change.
    pub flip_origin: HashMap<(String, String), (usize, usize)>,
    pub failure: Option<ReplayFailure>,
    pub needed: HashMap<(String, String), HashMap<String, bool>>,
}

/// Arguments of [`replay_create_graph`].
pub(crate) struct ReplayCreateGraphArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) top_level_atoms: &'a [String],
    pub(crate) root: &'a Path,
    pub(crate) allow_flips: bool,
    pub(crate) needed_init: &'a HashMap<(String, String), HashMap<String, bool>>,
    pub(crate) use_of: &'a dyn Fn(usize) -> Option<ReplayUse>,
}

/// Portage's `_create_graph` walk replayed over a settled graph, stopping at the
/// first failing dependency -- the fail-fast half the BFS pass does not have.
///
/// Walk order is `build_digraph`'s `.order` replay. Per popped node, each dep key
/// is one `_add_pkg_dep_string` call; unresolvable atoms yield first, so a
/// failing atom fails the key before any of its children is admitted.
///
/// USE: checked against the target's configured USE with the needed changes so
/// far applied. With `allow_flips` a takable mismatch becomes a needed change,
/// unless it contradicts one already needed (the failure); without it every
/// mismatch fails. `needed_init` seeds changes from earlier passes.
///
/// Deliberately conservative: an edge counts as failing only when its `cat/pkg`
/// has a `NoVisibleCandidate` entry naming this parent, or when the USE rule
/// fails it; an edge with no entry to land on at all is skipped.
/// Portage: depgraph.py, _create_graph
pub(crate) fn replay_create_graph(args: ReplayCreateGraphArgs<'_>) -> WalkReplay {
    let ReplayCreateGraphArgs {
        entries,
        top_level_atoms,
        root,
        allow_flips,
        needed_init,
        use_of,
    } = args;
    let n = entries.len();
    let pre = digraph_prelude(entries, root);
    let alt_suppressed = suppressed_alt_edges(entries, &pre, root, &HashMap::new());
    let mut out = WalkReplay {
        admitted_by: vec![None; n],
        admitted_arg: vec![None; n],
        needed: needed_init.clone(),
        ..WalkReplay::default()
    };
    let mut discovered = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    for (ai, atom_str) in top_level_atoms.iter().enumerate() {
        let Some(atom) = parse_dep_atom(atom_str, "replay_top_level_atoms") else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        if let Some(idxs) = pre
            .cp_indices
            .get(&(atom.category.as_str(), atom.package.as_str()))
        {
            for &i in idxs {
                if pre.installed[i] || !pre.edge_matches(atom_str, i) || discovered[i] {
                    continue;
                }
                discovered[i] = true;
                out.admitted.push(i);
                out.admitted_arg[i] = Some(ai);
                stack.push(i);
            }
        }
    }
    let mut disjunctive_stack: Vec<(usize, u8)> = Vec::new();
    loop {
        while let Some(i) = stack.pop() {
            let mut keys: Vec<u8> = entries[i]
                .deps
                .iter()
                .filter(|e| e.disjunctive)
                .map(|e| e.key)
                .collect();
            keys.dedup();
            for k in keys {
                disjunctive_stack.push((i, k));
            }
            let mut inline_keys: Vec<u8> = entries[i]
                .deps
                .iter()
                .filter(|e| !e.disjunctive)
                .map(|e| e.key)
                .collect();
            inline_keys.dedup();
            for k in inline_keys {
                if replay_key(ReplayKeyArgs {
                    entries,
                    pre: &pre,
                    alt_suppressed: &alt_suppressed,
                    i,
                    disjunctive: false,
                    key: k,
                    allow_flips,
                    use_of,
                    discovered: &mut discovered,
                    stack: &mut stack,
                    out: &mut out,
                }) {
                    return out;
                }
            }
        }
        let Some((i, k)) = disjunctive_stack.pop() else {
            break;
        };
        if replay_key(ReplayKeyArgs {
            entries,
            pre: &pre,
            alt_suppressed: &alt_suppressed,
            i,
            disjunctive: true,
            key: k,
            allow_flips,
            use_of,
            discovered: &mut discovered,
            stack: &mut stack,
            out: &mut out,
        }) {
            return out;
        }
    }
    out
}

/// Arguments of [`replay_key`].
pub(crate) struct ReplayKeyArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) pre: &'a DigraphPrelude<'a>,
    pub(crate) alt_suppressed: &'a [HashSet<usize>],
    pub(crate) i: usize,
    pub(crate) disjunctive: bool,
    pub(crate) key: u8,
    pub(crate) allow_flips: bool,
    pub(crate) use_of: &'a dyn Fn(usize) -> Option<ReplayUse>,
    pub(crate) discovered: &'a mut [bool],
    pub(crate) stack: &'a mut Vec<usize>,
    pub(crate) out: &'a mut WalkReplay,
}

/// Whether the replayed edge can land on a merge-bound or installed target.
/// Atoms with no package yield first in Portage's order, so a missing dep
/// fails the key before a later atom's USE change is recorded.
// Portage: depgraph.py, _minimize_children
fn replay_edge_has_target(
    entries: &[GraphEntry],
    pre: &DigraphPrelude<'_>,
    i: usize,
    ei: usize,
) -> bool {
    let edge = &entries[i].deps[ei];
    // drop-ok: fallback; dropped only if next line also fails
    let Some(atom) = portage_dep::parse_atom(&edge.evaluated)
        .or_else(|| parse_dep_atom(&edge.atom, "replay_edge_targets"))
    else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return false;
    }
    let idxs = pre
        .cp_indices
        .get(&(edge.category.as_str(), edge.package.as_str()))
        .map(Vec::as_slice)
        .unwrap_or_default();
    idxs.iter().copied().any(|j| {
        !matches!(
            entries[j].outcome,
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. }
        ) && pre.edge_matches(&edge.atom, j)
    })
}

/// One `_add_pkg_dep_string` call: fail the key on the first unresolvable atom,
/// else admit the targets in atom order. Returns `true` once a failure is recorded.
pub(crate) fn replay_key(args: ReplayKeyArgs<'_>) -> bool {
    let ReplayKeyArgs {
        entries,
        pre,
        alt_suppressed,
        i,
        disjunctive,
        key,
        allow_flips,
        use_of,
        discovered,
        stack,
        out,
    } = args;
    let parent_cp = (entries[i].category.as_str(), entries[i].package.as_str());
    // Portage `_minimize_children` yields the atoms with no package first, in
    // textual order, then the satisfiable ones: a missing dep fails the key
    // before a later atom's USE change is recorded. Target-less edges go
    // first, in stored order, then the rest, in stored order.
    // Portage: depgraph.py, _minimize_children
    let mut order: Vec<usize> = Vec::new();
    for (ei, edge) in entries[i].deps.iter().enumerate() {
        if edge.disjunctive != disjunctive || edge.key != key || alt_suppressed[i].contains(&ei) {
            continue;
        }
        order.push(ei);
    }
    order.sort_by_key(|&ei| replay_edge_has_target(entries, pre, i, ei));
    // Pass 1: pick every atom's target and settle its USE before admitting anything.
    let mut targets: Vec<(usize, usize)> = Vec::new();
    let mut flips: Vec<(Cp, Vec<(String, bool)>)> = Vec::new();
    for ei in order {
        let edge = &entries[i].deps[ei];
        // drop-ok: fallback; dropped only if next line also fails
        let Some(atom) = portage_dep::parse_atom(&edge.evaluated)
            .or_else(|| parse_dep_atom(&edge.atom, "replay_edge_atoms"))
        else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        let idxs = pre
            .cp_indices
            .get(&(edge.category.as_str(), edge.package.as_str()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let nvc_names_parent = idxs.iter().any(|&j| {
            matches!(entries[j].outcome, PretendOutcome::NoVisibleCandidate)
                && entries[j]
                    .required_by
                    .iter()
                    .any(|(c, p)| (c.as_str(), p.as_str()) == parent_cp)
        });
        let cands: Vec<usize> = idxs
            .iter()
            .copied()
            .filter(|&j| {
                !matches!(
                    entries[j].outcome,
                    PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. }
                ) && pre.edge_matches(&edge.atom, j)
            })
            .collect();
        // Only unconditional `flag` / `-flag` use-deps are checked: conditional ones
        // depend on the parent's own USE, which the walk may itself change.
        let written: HashSet<String> = parse_dep_atom(&edge.atom, "replay_written_use_deps")
            .and_then(|a| a.use_deps)
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                matches!(
                    d.op,
                    portage_dep::UseDepOp::Enabled | portage_dep::UseDepOp::Disabled
                )
            })
            .map(|d| d.flag)
            .collect();
        let use_deps: Vec<portage_dep::UseDep> = atom
            .use_deps
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| written.contains(&d.flag))
            .collect();
        // Highest-available over the graph: IUSE-carrying instances first, then the
        // usual merge-bound-then-highest ranking.
        let wanted: Vec<(String, bool)> = use_deps
            .iter()
            .filter_map(|d| match d.op {
                portage_dep::UseDepOp::Enabled => Some((d.flag.clone(), true)),
                portage_dep::UseDepOp::Disabled => Some((d.flag.clone(), false)),
                _ => None,
            })
            .collect();
        let flag_names: Vec<String> = use_deps
            .iter()
            .filter(|d| d.default.is_none())
            .map(|d| d.flag.clone())
            .collect();
        let carries =
            |j: usize| use_of(j).is_none_or(|u| flag_names.iter().all(|f| u.iuse.contains(f)));
        let preferred: Vec<usize> = cands.iter().copied().filter(|&j| carries(j)).collect();
        let pick = if preferred.is_empty() {
            &cands
        } else {
            &preferred
        };
        let Some(t) = DigraphPrelude::rank_best(entries, &pre.installed, pick.iter().copied())
        else {
            if nvc_names_parent {
                out.failure = Some(ReplayFailure {
                    parent: i,
                    edge: ei,
                    contradiction: false,
                });
                return true;
            }
            continue;
        };
        if !use_deps.is_empty()
            && let Some(u) = use_of(t)
        {
            let tcp = (entries[t].category.clone(), entries[t].package.clone());
            let mut current = u.enabled.clone();
            for (f, on) in out.needed.get(&tcp).into_iter().flatten() {
                if u.iuse.contains(f) {
                    if *on {
                        current.insert(f.clone());
                    } else {
                        current.remove(f);
                    }
                }
            }
            for (cp, fl) in &flips {
                if *cp == tcp {
                    for (f, on) in fl {
                        if *on {
                            current.insert(f.clone());
                        } else {
                            current.remove(f);
                        }
                    }
                }
            }
            if !portage_dep::use_deps_satisfied(&use_deps, &u.iuse, &current) {
                let mut change: Vec<(String, bool)> = Vec::new();
                let mut contradiction = false;
                let mut impossible = false;
                for (f, on) in wanted {
                    if !u.iuse.contains(&f) {
                        // Undeclared: the default alone decides; no change can help.
                        if flag_names.contains(&f) {
                            impossible = true;
                        }
                        continue;
                    }
                    if current.contains(&f) == on {
                        continue;
                    }
                    if u.fixed.contains(&f) {
                        impossible = true;
                        continue;
                    }
                    let prior = out
                        .needed
                        .get(&tcp)
                        .and_then(|m| m.get(&f))
                        .copied()
                        .or_else(|| {
                            flips
                                .iter()
                                .filter(|(cp, _)| *cp == tcp)
                                .flat_map(|(_, fl)| fl.iter())
                                .find(|(g, _)| *g == f)
                                .map(|(_, v)| *v)
                        });
                    if prior.is_some_and(|p| p != on) {
                        contradiction = true;
                    }
                    change.push((f, on));
                }
                if contradiction || impossible || !allow_flips || change.is_empty() {
                    out.failure = Some(ReplayFailure {
                        parent: i,
                        edge: ei,
                        contradiction,
                    });
                    return true;
                }
                out.flip_origin.entry(tcp.clone()).or_insert((i, ei));
                flips.push((tcp, change));
            }
        }
        targets.push((t, ei));
    }
    for (cp, fl) in flips {
        let m = out.needed.entry(cp).or_default();
        for (f, on) in fl {
            m.insert(f, on);
        }
    }
    for (t, ei) in targets {
        out.edges.push((i, ei, t));
        if !discovered[t] {
            discovered[t] = true;
            out.admitted.push(t);
            out.admitted_by[t] = Some((i, ei));
            stack.push(t);
        }
    }
    false
}
