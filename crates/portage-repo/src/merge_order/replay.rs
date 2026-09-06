//! Replaying `_create_graph` for fail-fast passes.

use crate::merge_order::*;

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
    // Pass 1: pick every atom's target and settle its USE before admitting anything.
    let mut targets: Vec<(usize, usize)> = Vec::new();
    let mut flips: Vec<(Cp, Vec<(String, bool)>)> = Vec::new();
    for (ei, edge) in entries[i].deps.iter().enumerate() {
        if edge.disjunctive != disjunctive || edge.key != key || alt_suppressed[i].contains(&ei) {
            continue;
        }
        let Some(atom) = portage_dep::parse_atom(&edge.evaluated)
            .or_else(|| portage_dep::parse_atom(&edge.atom))
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
        let written: HashSet<String> = portage_dep::parse_atom(&edge.atom)
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
