//! Dependency priorities, the edges they label and the ignore rungs.

use crate::merge_order::*;

/// The per-edge dependency classification driving every `ignore_priority` decision.
/// `cross` is always `false` (one root at a time); `ignored` is expressed by
/// dropping keys from the walk. `satisfied` is filled in by `build_digraph`.
/// Portage: _emerge/DepPriority.py, DepPriority
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DepPriority {
    pub buildtime: bool,
    pub runtime: bool,
    pub runtime_post: bool,
    pub buildtime_slot_op: bool,
    pub runtime_slot_op: bool,
    pub optional: bool,
    /// An already-installed package matches this atom, so the edge can be relaxed
    /// to break a cycle.
    pub satisfied: bool,
}

/// One resolved dependency of a `GraphEntry`, in `deps`-tuple key order
/// (`RDEPEND`, `IDEPEND`, `PDEPEND`, `DEPEND`, `BDEPEND`).
/// Portage: depgraph.py, _add_pkg_dep_string
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepEdge {
    pub atom: String,
    /// The token with its use-deps evaluated against the parent's USE, for the
    /// `--debug` `Candidates:` line. Falls back to the token when evaluation
    /// returns `None`.
    pub evaluated: String,
    pub category: String,
    pub package: String,
    pub priority: DepPriority,
    /// Inside a `|| ( … )` group, or itself a `virtual/*` atom: deferred until the
    /// ordinary stack drains, one bundle per dep key.
    /// Portage: depgraph.py, _queue_disjunctive_deps
    pub disjunctive: bool,
    /// For a `disjunctive` atom from a `||` group: `(group, branch)`.
    /// `None` for an inline atom or a bare deferred `virtual/*`.
    pub alt: Option<(u32, u32)>,
    /// Which `deps`-tuple key this came from (0=`RDEPEND` .. 4=`BDEPEND`).
    pub key: u8,
}

/// Run over one USE-reduced dep key: returns `(inline, disjunctive)` atom lists.
/// A `||` group goes wholesale into the deferred bundle; a bare `virtual/*`
/// atom is deferred too, with `u32::MAX` markers so it never groups with a
/// genuine alternative.
/// Portage: depgraph.py, _queue_disjunctive_deps
pub(crate) fn split_disjunctive(tokens: &[String]) -> (Vec<String>, Vec<(String, u32, u32)>) {
    let mut inline: Vec<String> = Vec::new();
    let mut disjunctive: Vec<(String, u32, u32)> = Vec::new();
    // Everything below an enclosing `||` is disjunctive, however deeply nested.
    let mut any_of_depth: Option<usize> = None;
    let mut depth: usize = 0;
    let mut pending_any_of = false;
    let mut group: u32 = 0;
    let mut next_group: u32 = 0;
    let mut branch: u32 = 0;
    // The branch stays fixed for every atom until that group's `)`.
    let mut branch_group_depth: Option<usize> = None;
    for tok in tokens {
        match tok.as_str() {
            "||" => pending_any_of = true,
            "(" => {
                depth += 1;
                if pending_any_of && any_of_depth.is_none() {
                    any_of_depth = Some(depth);
                    group = next_group;
                    next_group += 1;
                    branch = 0;
                    branch_group_depth = None;
                } else if any_of_depth.is_some()
                    && Some(depth) == any_of_depth.map(|d| d + 1)
                    && branch_group_depth.is_none()
                {
                    // A nested alternative directly inside the `||`.
                    branch_group_depth = Some(depth);
                }
                pending_any_of = false;
            }
            ")" => {
                if branch_group_depth == Some(depth) {
                    branch_group_depth = None;
                    branch += 1;
                }
                if any_of_depth == Some(depth) {
                    any_of_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {
                pending_any_of = false;
                let is_virtual =
                    portage_dep::parse_atom(tok).is_some_and(|a| a.category == "virtual");
                if let Some(aod) = any_of_depth {
                    disjunctive.push((tok.clone(), group, branch));
                    if depth == aod {
                        // A bare atom directly in the `||` is its own branch.
                        branch += 1;
                    }
                } else if is_virtual {
                    disjunctive.push((tok.clone(), u32::MAX, u32::MAX));
                } else {
                    inline.push(tok.clone());
                }
            }
        }
    }
    (inline, disjunctive)
}

/// The dependency keys walked, paired with the `DepPriority` each contributes.
/// `optional` is `pkg.built` for the build-time keys.
/// Portage: depgraph.py, _add_pkg_dep_string (deps tuple)
pub(crate) fn key_priority(key: &str, built: bool) -> DepPriority {
    match key {
        "RDEPEND" | "IDEPEND" => DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        "PDEPEND" => DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        },
        // "DEPEND" | "BDEPEND"
        _ => DepPriority {
            buildtime: true,
            optional: built,
            ..DepPriority::default()
        },
    }
}

/// Flattens `metadata`'s dep-key strings, one key at a time in `keys` order.
/// One `DepEdge` per distinct `(atom, category, package, priority)`: an atom
/// named by two keys contributes two edges. `built` marks build-time keys
/// `optional`.
pub(crate) fn dep_edges_from_metadata(
    metadata: &HashMap<String, String>,
    use_flags: &HashSet<String>,
    keys: &[&str],
    built: bool,
) -> Vec<DepEdge> {
    let mut edges: Vec<DepEdge> = Vec::new();
    let mut seen: HashSet<(String, String, String, DepPriority)> = HashSet::new();
    for dep_key in keys {
        let Some(d) = metadata.get(*dep_key) else {
            continue;
        };
        let base = key_priority(dep_key, built);
        let key_index = match *dep_key {
            "RDEPEND" => 0,
            "IDEPEND" => 1,
            "PDEPEND" => 2,
            "DEPEND" => 3,
            _ => 4,
        };
        let toks: Vec<String> = d.split_whitespace().map(String::from).collect();
        // The structured reduction keeps the `||` boundaries `split_disjunctive` needs.
        let Ok(structured) = portage_use_reduce::use_reduce_structured(
            &toks,
            use_flags,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };
        let (inline, deferred) = split_disjunctive(&structured);
        let flat: Vec<(String, Option<(u32, u32)>)> = inline
            .into_iter()
            .map(|a| (a, None))
            .chain(deferred.into_iter().map(|(a, g, b)| (a, Some((g, b)))))
            .collect();
        for (t, alt) in flat {
            if t == "||" {
                continue;
            }
            let disjunctive = alt.is_some();
            let Some(dep_atom) = portage_dep::parse_atom(&t) else {
                crate::note_unparsed_dep_token(&t, "merge-order digraph");
                continue;
            };
            if dep_atom.blocker != portage_dep::Blocker::None {
                continue;
            }
            // A `:=`/`:slot=` atom promotes its key's priority to the slot-operator
            // variant, which the ladder relaxes less readily.
            let mut priority = base;
            if dep_atom.slot_operator == Some(portage_dep::SlotOperator::Equals) {
                if priority.buildtime {
                    priority.buildtime_slot_op = true;
                }
                if priority.runtime {
                    priority.runtime_slot_op = true;
                }
            }
            let key = (
                t.clone(),
                dep_atom.category.clone(),
                dep_atom.package.clone(),
                priority,
            );
            if seen.insert(key) {
                edges.push(DepEdge {
                    atom: t.clone(),
                    evaluated: portage_dep::evaluate_atom_conditionals(&t, use_flags)
                        .unwrap_or_else(|| t.clone()),
                    category: dep_atom.category,
                    package: dep_atom.package,
                    priority,
                    disjunctive,
                    alt,
                    key: key_index,
                });
            }
        }
    }
    edges
}

pub(crate) fn s_ignore_satisfied_runtime_post(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if !p.satisfied {
        return false;
    }
    if p.buildtime || p.runtime {
        return false;
    }
    p.runtime_post
}

pub(crate) fn s_ignore_runtime_post(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime || p.runtime {
        return false;
    }
    p.runtime_post
}

pub(crate) fn s_ignore_satisfied_runtime(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime {
        return false;
    }
    if !p.runtime {
        return true;
    }
    p.satisfied
}

pub(crate) fn s_ignore_satisfied_buildtime(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.buildtime_slot_op {
        return false;
    }
    p.satisfied
}

pub(crate) fn s_ignore_satisfied_buildtime_slot_op(p: &DepPriority) -> bool {
    if p.optional {
        return true;
    }
    if p.satisfied {
        return true;
    }
    !p.buildtime && !p.runtime
}

impl std::fmt::Display for DepPriority {
    /// The single highest classification; `Priority:` lines and edge labels use it.
    /// Portage: _emerge/DepPriority.py, __str__
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::resolver_trace::dep_priority_str(self))
    }
}
