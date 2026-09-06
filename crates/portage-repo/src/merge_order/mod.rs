//! Portage's `_serialize_tasks` merge-order scheduler over a typed digraph.
//!
//! Portage's scheduler is not a topological sort: it is a repeated "pop every
//! currently-eligible leaf" loop over edges carrying a `DepPriority`, with a
//! priority-relaxation ladder, a `_merge_order_bias` pre-sort, and an
//! `asap_nodes` fast path for freshly-unblocked `PDEPEND` children.
//! Portage: _emerge/depgraph.py, _serialize_tasks
//!
//! The graph holds only the forward transitive closure of the merge-bound
//! packages, which reproduces Portage's merge list exactly (see notes).
//!
//! The selection loop runs over an incrementally-maintained leaf frontier
//! (`SerializeFrontier`, a port of `_serialize_frontier.py`); the frontier
//! is pure plumbing, identical with it on or off (pinned by unit tests).

use std::cmp::Reverse;

mod alt_edges;
mod build;
mod cycles;
mod debug;
mod digraph;
mod installed_closure;
mod prelude;
mod priority;
mod replay;
mod select_nodes;
mod tree_display;

#[cfg(test)]
mod tests;

pub use alt_edges::*;
pub(crate) use build::*;
pub(crate) use cycles::*;
pub(crate) use debug::*;
pub(crate) use digraph::*;
pub use installed_closure::*;
pub(crate) use prelude::*;
pub use priority::*;
pub(crate) use replay::*;
pub(crate) use select_nodes::*;
pub use tree_display::*;

use std::collections::{BinaryHeap, HashMap, HashSet};

use std::path::Path;

use crate::{
    BlockerSatisfiedBy, CandidateSource, CircularDepChild, GraphEntry, PretendOutcome, RepoConfig,
    RootCp, VisibilityProvenance, all_installed_packages, read_vdb_flag_set, read_vdb_slot,
};

/// The version a `GraphEntry` resolves to, whatever its outcome.
pub(crate) fn entry_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version }
        | PretendOutcome::Reinstall { version, .. }
        | PretendOutcome::AlreadyInstalled { version }
        | PretendOutcome::Uninstall { version } => Some(version),
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::NoVisibleCandidate => None,
    }
}

/// Arguments of [`schedule_graph`].
struct ScheduleGraphArgs<'a> {
    entries: &'a [GraphEntry],
    top_level_atoms: &'a [String],
    config: &'a portage_profile::Config,
    root: &'a Path,
    implicit_system_deps: bool,
    repos: &'a [RepoConfig],
    dynamic_deps: bool,
    circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
}

/// Installed-dependency closure, `build_digraph`, the nomerge-root prune and
/// `_merge_order_bias`. Diagnostics stay with the flat caller, not here.
fn schedule_graph(args: ScheduleGraphArgs<'_>) -> (Vec<GraphEntry>, Digraph, usize, Vec<usize>) {
    let ScheduleGraphArgs {
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    } = args;
    let real_n = entries.len();
    // Complete mode (a merge changes an installed package): supply installed trees.
    // Synthetic nodes get indices `>= real_n` and are filtered out below.
    let complete = entries.iter().any(|e| {
        matches!(
            e.outcome,
            PretendOutcome::Reinstall { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        ) || (matches!(e.outcome, PretendOutcome::New { .. }) && e.new_slot)
    });
    let mut ext = entries.to_vec();
    add_installed_dependency_closure(AddInstalledDependencyClosureArgs {
        entries: &mut ext,
        root,
        repos,
        system_atoms: &config.system_packages,
        virtuals_only: !complete,
        dynamic_deps,
    });
    let entries: &[GraphEntry] = &ext;
    let n = entries.len();

    let mut g = build_digraph(entries, top_level_atoms, root, circular);
    // Unbiased discovery rank, kept before the bias re-sort.
    let mut discovery_rank = vec![0usize; n];
    for (pos, &i) in g.order.iter().enumerate() {
        discovery_rank[i] = pos;
    }

    debug_dump_graph(&g, entries, top_level_atoms, root);

    // Prune parentless synthetic installed roots to a fixpoint; restricted to
    // `i >= real_n` so displayed top-level installed targets stay scheduled.
    loop {
        let mut removed = false;
        for i in real_n..n {
            if !g.alive[i] || !g.installed[i] {
                continue;
            }
            if g.parents[i].iter().any(|&p| g.alive[p]) {
                continue;
            }
            g.alive[i] = false;
            removed = true;
        }
        if !removed {
            break;
        }
    }
    g.order.retain(|&i| g.alive[i]);
    merge_order_bias(&mut g, entries, config, implicit_system_deps);
    (ext, g, real_n, discovery_rank)
}
