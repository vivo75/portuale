//! Replaying `_create_graph` for fail-fast passes.

use crate::merge_order::*;

/// A `(category, package)` key.
pub(crate) type Cp = (String, String);

/// The USE state `replay_create_graph` needs for one entry: its declared
/// (profile-valid) `IUSE`, the flags its configuration enables *before*
/// any autounmask change, and the flags no `package.use` change can touch
/// (profile-forced or -masked). `None` for an entry whose USE cannot
/// change (installed, binary, missing) -- its use-deps are not checked.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayUse {
    pub iuse: HashSet<String>,
    pub enabled: HashSet<String>,
    pub fixed: HashSet<String>,
}

/// The first dependency real's `_create_graph` fails on, as
/// `(parent entry, index into its deps)`. `contradiction` marks the
/// autounmask shape: the atom needs a USE flag the other way from a
/// change already needed on the same package (real rejects the flip,
/// `depgraph.py:7714`, and the dependency is unsatisfied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayFailure {
    pub parent: usize,
    pub edge: usize,
    pub contradiction: bool,
}

/// What `replay_create_graph` saw: real's admission order, the edge that
/// first admitted each entry (`None` for an argument or an unreached
/// entry), the first failing dependency, and the USE changes the walk
/// needed, per `cat/pkg` (real `_needed_use_config_changes`).
#[derive(Debug, Clone, Default)]
pub(crate) struct WalkReplay {
    pub admitted: Vec<usize>,
    pub admitted_by: Vec<Option<(usize, usize)>>,
    /// For an argument entry: the index into `top_level_atoms` of the
    /// atom that added it.
    pub admitted_arg: Vec<Option<usize>>,
    /// Every resolved `(parent, dep index, target)` edge, in walk order
    /// (real's digraph parent-insertion order).
    pub edges: Vec<(usize, usize, usize)>,
    /// Per `cat/pkg`, the `(parent, dep index)` whose atom first needed
    /// each USE change the walk made (real's dep chain for the change).
    pub flip_origin: HashMap<(String, String), (usize, usize)>,
    pub failure: Option<ReplayFailure>,
    pub needed: HashMap<(String, String), HashMap<String, bool>>,
}

/// Backlog #244: real `_create_graph`'s walk replayed over a settled
/// graph, stopping at the first dependency that fails -- the fail-fast
/// half portuale's BFS pass (which walks everything and classifies
/// afterwards, see `abort_outcome`) does not have.
///
/// Walk order is `build_digraph`'s own `.order` replay (arguments added
/// in argv order and pushed, `_dep_stack` popped LIFO, disjunctive
/// bundles deferred until the stack drains). Per popped node, each dep
/// key is one `_add_pkg_dep_string` call: `_minimize_children`
/// (`depgraph.py:4751-4775`) yields unresolvable atoms first, so a
/// failing atom anywhere in the key fails the node before any of the
/// key's children is admitted; earlier keys' children stay admitted.
///
/// USE: an atom's use-deps are checked against the target's configured
/// USE with the needed changes so far applied (real `_pkg_use_enabled`).
/// With `allow_flips` a mismatch the target can take becomes a needed
/// change -- unless the change contradicts one already needed, which is
/// the failure (`depgraph.py:7714`); without it (real's
/// `myparams["autounmask"] = False` clean pass) every mismatch fails.
/// `needed_init` seeds the changes from earlier passes (real
/// `_feedback_config`), keyed by `cat/pkg`.
///
/// Deliberately conservative: an edge counts as failing only when its
/// `cat/pkg` has a `NoVisibleCandidate` entry naming this parent, or
/// when the USE rule above fails it; an edge with no entry to land on
/// at all (deps portuale never walks) is skipped.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn replay_create_graph(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
    allow_flips: bool,
    needed_init: &HashMap<(String, String), HashMap<String, bool>>,
    use_of: &dyn Fn(usize) -> Option<ReplayUse>,
) -> WalkReplay {
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
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
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
                if replay_key(
                    entries,
                    &pre,
                    &alt_suppressed,
                    i,
                    false,
                    k,
                    allow_flips,
                    use_of,
                    &mut discovered,
                    &mut stack,
                    &mut out,
                ) {
                    return out;
                }
            }
        }
        let Some((i, k)) = disjunctive_stack.pop() else {
            break;
        };
        if replay_key(
            entries,
            &pre,
            &alt_suppressed,
            i,
            true,
            k,
            allow_flips,
            use_of,
            &mut discovered,
            &mut stack,
            &mut out,
        ) {
            return out;
        }
    }
    out
}

/// One `_add_pkg_dep_string` call of `replay_create_graph`: resolve every
/// atom of `entries[i]`'s key `key`, fail the key on the first
/// unresolvable one (unresolvable atoms are yielded first, so nothing of
/// the key is admitted then), else admit the targets in atom order.
/// Returns `true` once a failure is recorded.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn replay_key(
    entries: &[GraphEntry],
    pre: &DigraphPrelude<'_>,
    alt_suppressed: &[HashSet<usize>],
    i: usize,
    disjunctive: bool,
    key: u8,
    allow_flips: bool,
    use_of: &dyn Fn(usize) -> Option<ReplayUse>,
    discovered: &mut [bool],
    stack: &mut Vec<usize>,
    out: &mut WalkReplay,
) -> bool {
    let parent_cp = (entries[i].category.as_str(), entries[i].package.as_str());
    // Pass 1: pick every atom's target and settle its USE; a failure
    // stops the key before anything is admitted.
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
        // Only the use-deps written unconditionally (`flag` / `-flag`) are
        // checked: a `flag?` / `flag=` one depends on the parent's own USE,
        // which real's walk may itself change (`_apply_parent_use_changes`)
        // -- evaluating it against the settled parent would invent
        // contradictions real never meets.
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
        // Real `_select_pkg_highest_available` over the graph: an
        // instance whose IUSE can carry every use-dep flag, then the
        // usual merge-bound-then-highest ranking.
        // Only the unconditional `flag` / `-flag` forms constrain the
        // target (`use_deps_satisfied`'s own rule); a flag with a
        // `(+)`/`(-)` default need not be declared.
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
                        // Undeclared: satisfied or not by its `(+)`/`(-)`
                        // default alone -- no change can help.
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
