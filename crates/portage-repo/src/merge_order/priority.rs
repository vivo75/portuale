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
