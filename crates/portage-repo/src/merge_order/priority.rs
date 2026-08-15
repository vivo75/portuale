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

impl std::fmt::Display for DepPriority {
    /// The single highest classification; `Priority:` lines and edge labels use it.
    /// Portage: _emerge/DepPriority.py, __str__
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::resolver_trace::dep_priority_str(self))
    }
}
