//! Real `_emerge/depgraph.py::_serialize_tasks` -- portage's own merge-order
//! scheduler -- ported over a typed dependency digraph.
//!
//! The pre-2026-09-06 implementation approximated this with a batched
//! Kahn's walk over cp-level `required_by` edges plus a discovery-rank
//! tie-break. That reproduced the *set* of merge tasks but not their
//! order, because real's scheduler is not a topological sort at all: it
//! is a repeated "pop every currently-eligible leaf" loop over a digraph
//! whose edges carry a real `DepPriority` (build-time / run-time /
//! run-time-post / slot-operator / optional / already-satisfied), with a
//! priority-relaxation ladder for cycles, a `_merge_order_bias`
//! pre-sort, and an `asap_nodes` fast path for freshly-unblocked
//! `PDEPEND` children.
//!
//! Every piece of that is needed for the order to come out right -- the
//! ladder decides *which* nodes are eligible in a round, the bias
//! decides their sequence within it, and `asap_nodes` overrides both for
//! `PDEPEND` children. Validated end to end against real portage's own
//! `emerge -p --debug` digraph dump on a live ~1850-node Gentoo system:
//! feeding this algorithm real's dumped graph reproduces real's merge
//! list exactly.
//!
//! The one deliberate scope reduction, also verified against that dump:
//! the graph only needs the *forward transitive dependency closure of
//! the merge-bound packages* (461 nodes for the validation case), not
//! real's full `@world`/`@system` universe (1854 nodes). Real prunes its
//! own graph down before scheduling ("Prune 'nomerge' root nodes if
//! nothing depends on them", `depgraph.py:9509-9518`), and re-running
//! the algorithm on just the closure yields the identical merge list.
//!
//! The selection loop runs over an incrementally-maintained leaf
//! frontier (`SerializeFrontier` below, a port of real
//! `_emerge/_serialize_frontier.py`): per-node, per-filter
//! surviving-child counts plus per-filter ready heaps, so each ladder
//! rung enumerates its leaves without an O(V+E) scan and each removal
//! only decrements its parents. `PORTAGE_SERIALIZE_FRONTIER_DISABLE`
//! falls back to the plain scans (real's own escape hatch). Pure
//! plumbing: leaf sets, order, and every downstream decision are
//! identical with the frontier on or off (pinned by unit tests).

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

/// The version a `GraphEntry` resolves to, whatever its outcome -- real
/// `Package.version`, which `find_smallest_cycle`'s `sorted(nodes)`
/// compares after `cp`. A `Uninstall` removal (#72 B3) carries the
/// installed version it removes, like real's uninstall `Package`.
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

/// The whole pipeline: build the digraph out of `entries`, prune it the
/// way real does, bias it, schedule it, and weave portuale's own
/// non-merge-bound entries back in.
///
/// `implicit_system_deps` (real `--implicit-system-deps`, default on)
/// gates the bias re-sort only -- the digraph build, prune, schedule,
/// and weave-back are identical either way.
///
/// Returns a permutation of `0..entries.len()` in merge order.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn serialize_merge_order(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
    repos: &[RepoConfig],
    dynamic_deps: bool,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> (Vec<usize>, Vec<usize>) {
    let (ext, mut g, real_n, discovery_rank) = schedule_graph(
        entries,
        top_level_atoms,
        config,
        root,
        implicit_system_deps,
        repos,
        dynamic_deps,
        circular,
    );
    let entries: &[GraphEntry] = &ext;

    debug_dump_graph(&g, entries, top_level_atoms, root);

    // B3: post-prune, pre-bias insertion order -- the tie-break real's
    // stable `_merge_order_bias` preserves when parent counts are equal.
    if mo_sel_enabled() {
        let nodes: Vec<String> = g
            .order
            .iter()
            .map(|&i| mo_sel_cpv(&entries[i], g.installed[i]))
            .collect();
        eprintln!("MO_ORDER count={} {}", nodes.len(), nodes.join(" "));
    }

    merge_order_bias(&mut g, entries, config, implicit_system_deps);
    let scheduled: Vec<usize> = select_nodes(&mut g, entries, root, None)
        .into_iter()
        .filter(|&i| i < real_n)
        .collect();

    // Real's own retlist skips every "nomerge" node (`if node.operation
    // == "nomerge": continue`), because real never displays one. Portuale
    // keeps its `AlreadyInstalled`/`NoVisibleCandidate` entries -- its
    // `--tree`/`--emptytree`/"already installed" rendering needs them --
    // so a nomerge node that survived the prune keeps the position the
    // scheduler gave it: it is a genuine graph node there, ordered ahead
    // of everything that depends on it, exactly as real orders it before
    // dropping it from the display.
    //
    // Only the entries the scheduler never saw are woven back in
    // afterwards: the pruned "nomerge" *roots* (real drops those from
    // scheduling outright -- a top-level already-installed target) and
    // any entry with no node at all. Each goes on its own unbiased
    // discovery rank, so such an entry is never bias-promoted past a
    // merge task it was behind in plain discovery order.
    let scheduled = weave_scheduled_leftovers(&scheduled, real_n, &discovery_rank);
    // Backlog #278: the pre-bias insertion order over the caller's
    // (pre-sort, pre-extension) indices -- real `digraph`'s node
    // insertion order, which `get_cycles` iterates. Only indices below
    // `real_n` can appear in `scheduled` (synthetics are filtered
    // above), so truncating the extended rank is exact.
    (scheduled, discovery_rank[..real_n].to_vec())
}

/// The shared scheduler-graph setup for [`serialize_merge_order`]
/// and the #81 tree-mode simulation: the installed-dependency closure,
/// `build_digraph`, the nomerge-root prune and `_merge_order_bias`.
/// Returns the extended entries, the biased graph, the real-entry count
/// and the unbiased discovery rank. Diagnostics (`debug_dump_graph`,
/// the `MO_ORDER` print) stay with the flat caller, not here.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
fn schedule_graph(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    config: &portage_profile::Config,
    root: &Path,
    implicit_system_deps: bool,
    repos: &[RepoConfig],
    dynamic_deps: bool,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> (Vec<GraphEntry>, Digraph, usize, Vec<usize>) {
    let real_n = entries.len();
    // Real `_complete_graph` auto-enables (a merge changes an
    // already-installed package -> real re-walks the whole `@world` /
    // `@system` universe as nomerge nodes carrying their full recorded
    // dependency trees). In that mode leaf selection clears a shallow
    // installed subtree (`xtrans` -> `elt-patches` -> `gentoo-functions`)
    // before a deep one (`xorg-proto` -> `meson` -> the Python stack).
    // Portuale gave every installed node `(no children)`, so both freed
    // together and sorted by bias alone. `add_installed_dependency_
    // closure` supplies those trees -- but only when a real
    // `_complete_graph` would run: a plain all-`[ebuild N]` resolve keeps
    // installed nodes as leaves. The trigger is the same set real's
    // `complete_graph_auto_enable` checks (in-slot version/USE change, or
    // a new-slot install of an already-installed `cp`); computed here
    // straight off `entries` rather than threaded from the CLI, because
    // the CLI's own `want_complete` misses a reason-less `[ebuild R]`
    // whose displayed USE still differs from the vdb. Synthetic nodes get
    // indices `>= real_n` and are filtered out of `scheduled` below.
    let complete = entries.iter().any(|e| {
        matches!(
            e.outcome,
            PretendOutcome::Reinstall { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        ) || (matches!(e.outcome, PretendOutcome::New { .. }) && e.new_slot)
    });
    let mut ext = entries.to_vec();
    add_installed_dependency_closure(
        &mut ext,
        root,
        repos,
        &config.system_packages,
        !complete,
        dynamic_deps,
    );
    let entries: &[GraphEntry] = &ext;
    let n = entries.len();

    let mut g = build_digraph(entries, top_level_atoms, root, circular);
    // Unbiased discovery rank, kept before the bias re-sorts `g.order` --
    // it is what the trivial (non-merge-bound) entries are woven back in
    // on, so that a "package is already installed" notice never gets
    // bias-promoted past a merge task it was behind in plain discovery
    // order.
    let mut discovery_rank = vec![0usize; n];
    for (pos, &i) in g.order.iter().enumerate() {
        discovery_rank[i] = pos;
    }

    debug_dump_graph(&g, entries, top_level_atoms, root);

    // Real `_serialize_tasks`: after stripping its `DependencyArg` nodes
    // (portuale has none), "Prune 'nomerge' root nodes if nothing depends
    // on them, since otherwise they slow down merge order calculation"
    // (`depgraph.py:9505-9518`) -- iterated to a fixpoint. This is what
    // turns the `@system` seed above into real's actual post-prune
    // selection graph: the deep-only members survive (a parent inside the
    // closure), the top-level `@system` leaves (`net-misc/wget` and its
    // ilk, whose only parent was the pruned `SetArg`) do not.
    //
    // Restricted to the synthetic `i >= real_n` nodes. A real
    // `AlreadyInstalled` *entry* -- a top-level `--noreplace`/`--deep`
    // target -- is also a nomerge root real would drop, but portuale
    // *displays* it (real never does) and its `--tree` / "already
    // installed" rendering wants it after the deps it pulls in. Leaving
    // it in the scheduler produces exactly that (it only leafs once its
    // subtree has); pruning it instead drops it into `leftover`, which
    // re-weaves it on its *parent-first* discovery rank -- wrong order.
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
