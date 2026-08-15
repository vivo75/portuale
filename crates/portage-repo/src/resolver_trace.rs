//! `emerge --pretend --debug` resolver trace: the DEBUG-level per-package
//! narration (`tr!`, stdout) and the digraph/candidate dumps (`tr_err!`,
//! stderr). Emitting Portage's shapes makes the two implementations directly
//! diffable.
//!
//! Deliberate divergences: plain-text node labels (no ANSI colour);
//! portuale's post-prune merge closure as the node set, with top-level atoms
//! as pseudo-arg nodes; and `Child:`/`Parent Dep:` interleaving in portuale's
//! BFS queue order, not Portage's LIFO stack order (the `digraph:` dump
//! matches; the narration order does not).
//!
//! Design note and the full message inventory:
//! `docs/history/emerge-pretend-debug.md`.
// Portage: _emerge/depgraph.py, portage/util/__init__.py (writemsg_level)

use std::fmt;
use std::path::Path;

use crate::merge_order::DepPriority;
use crate::{
    CandidateSource, GraphEntry, PretendOutcome, RepoConfig, installed_candidates,
    installed_pkg_repo, list_candidates, read_vdb_slot,
};

/// The single highest priority classification, for `Priority:` lines and
/// `digraph:` edge labels.
// Portage: _emerge/DepPriority.py, DepPriority.__str__
pub(crate) fn dep_priority_str(p: &DepPriority) -> &'static str {
    // No `ignored` field modelled here; `optional` is the widest portuale
    // expresses.
    if p.optional {
        "optional"
    } else if p.buildtime_slot_op {
        "buildtime_slot_op"
    } else if p.buildtime {
        "buildtime"
    } else if p.runtime_slot_op {
        "runtime_slot_op"
    } else if p.runtime {
        "runtime"
    } else if p.runtime_post {
        "runtime_post"
    } else {
        "soft"
    }
}
