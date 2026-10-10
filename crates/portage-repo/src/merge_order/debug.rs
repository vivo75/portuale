//! `--debug` merge-order dumps and node-selection traces.

use crate::merge_order::*;

/// `PORTUALE_MO_SEL=1`: emit one `MO_SEL ` line per `select_nodes`
/// iteration to stderr -- the portuale half of the real-vs-portuale
/// merge-order trace harness (`TEST/scripts/mo-trace/`). The real half is
/// `real-trace.py`, which injects an `RT_SEL` line with the same fields
/// into real's `_serialize_tasks`; `align-traces.py` walks the two streams
/// in step and reports the first iteration whose state diverges. Never
/// on by default, so a normal run is byte-identical (there is no other
/// output change).
pub(crate) fn mo_sel_enabled() -> bool {
    std::env::var_os("PORTUALE_MO_SEL").is_some_and(|v| v != "0")
}

/// The version an entry is resolved at (the merge target for an
/// upgrade/downgrade, the current version otherwise). `None` for
/// `NoVisibleCandidate` and for a `Uninstall` removal (#72 B3), which is
/// never a merge target.
pub(crate) fn outcome_version(e: &GraphEntry) -> Option<&str> {
    match &e.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            Some(version)
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
        PretendOutcome::AlreadyInstalled { version } => Some(version),
        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
    }
}

/// Real `Package`'s `cat/pkg-ver` label for the trace `pick=` field.
/// Prefixed `m:` for a merge-bound node (real `operation == "merge"`) or
/// `n:` for a nomerge/installed one, so the aligner can see *which kind*
/// of node each side removed when the frontiers differ -- the iter-1
/// gnuconfig/elt-patches batch vs real's installed-nomerge batch is
/// exactly the difference this harness exists to localize.
pub(crate) fn mo_sel_cpv(e: &GraphEntry, installed: bool) -> String {
    let kind = if installed { 'n' } else { 'm' };
    match outcome_version(e) {
        Some(v) => format!("{kind}:{}/{}-{}", e.category, e.package, v),
        None => format!("{kind}:{}/{}", e.category, e.package),
    }
}

/// One `MO_SEL ` line (`PORTUALE_MO_SEL`) -- pure so the format is
/// unit-pinned. Field order is the shared harness contract:
/// `iter retlist alive asap prefer_asap drop_satisfied ig pick`; `asap`
/// is the bracketed list of `asap_nodes` cpvs (a count was not enough:
/// firefox's clang-runtime divergence is exactly one extra lingering
/// asap node on portuale's side).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn mo_sel_trace_line(
    iter: usize,
    retlist: usize,
    alive: usize,
    asap: &[String],
    prefer_asap: bool,
    drop_satisfied: bool,
    ig: Option<Ignore>,
    pick: &[String],
) -> String {
    format!(
        "MO_SEL iter={iter} retlist={retlist} alive={alive} asap=[{}] \
         prefer_asap={} drop_satisfied={} ig={} pick={}",
        asap.join(" "),
        prefer_asap as u8,
        drop_satisfied as u8,
        ignore_name(ig),
        pick.join(" ")
    )
}

/// Dumps the freshly-built digraph in real portage's own
/// `digraph.debug_print()` format (`portage/util/digraph.py:349`),
/// preceded by the `\ndigraph:\n\n` header -- to **stderr**, under
/// `emerge --pretend --debug` (or the legacy `PORTUALE_DEBUG_MERGE_GRAPH`
/// env var). This is how the module was validated and the fastest way to
/// localise a merge-order divergence: dump both, diff the node sets, then
/// the edge sets, then the priorities, then `.order` (per this module's
/// header).
///
/// Real's format, per node in insertion order (`g.order` before
/// `merge_order_bias` re-sorts it -- real `debug_print` iterates
/// `self.nodes`, not `.order`):
///
/// ```text
/// (cat/pkg-ver:slot/sub_slot::repo, <state>) depends on
///   (child..., <state>) (<max-priority>)
/// (leaf..., <state>) (no children)
/// ```
///
/// **Divergences from real** (see `resolver_trace`'s header):
/// plain-text node labels; portuale's post-prune merge closure as the
/// node set, with the top-level atoms appended as pseudo-`DependencyArg`
/// nodes (`<atom> depends on` / `  <resolved-node> (soft)`) so a diff
/// against real still lines up, rather than real's full `@world`/
/// `@system` universe (proven unnecessary for ordering by the
/// `_serialize_tasks` port).
/// Real `depgraph.py:9917-9930`'s `\nruntime cycle digraph (<n> nodes):
/// \n\n` + `debug_print()` of the cycle's induced subgraph + `runtime
/// cycle leaf: <pkg>\n\n` -- to stderr, under `--pretend --debug`, once
/// per relaxed runtime cycle. Same node/edge formatting as
/// `debug_dump_graph`.
pub(crate) fn debug_dump_cycle(
    g: &Digraph,
    entries: &[GraphEntry],
    sub: &HashSet<usize>,
    root: &Path,
) {
    let label = |i: usize| crate::resolver_trace::node_label(&entries[i], root, g.installed[i]);
    crate::resolver_trace::err(format_args!(
        "\nruntime cycle digraph ({} nodes):\n\n",
        sub.len()
    ));
    for &i in g.order.iter().filter(|i| sub.contains(i)) {
        let kids: Vec<&(usize, Vec<DepPriority>)> = g.children[i]
            .iter()
            .filter(|(c, _)| sub.contains(c))
            .collect();
        if kids.is_empty() {
            crate::resolver_trace::err(format_args!("{} (no children)\n", label(i)));
        } else {
            crate::resolver_trace::err(format_args!("{} depends on\n", label(i)));
            for (c, prios) in kids {
                let max = crate::resolver_trace::max_priority(prios);
                crate::resolver_trace::err(format_args!("  {} ({})\n", label(*c), max));
            }
        }
    }
    // Real `cycle_digraph.order[-1]` -- the last node in `.order` within
    // the subgraph is the leaf real reports.
    if let Some(&leaf) = g.order.iter().rev().find(|i| sub.contains(i)) {
        crate::resolver_trace::err(format_args!("runtime cycle leaf: {}\n\n", label(leaf)));
    }
}

/// Build the digraph and dump it -- for `topological_merge_order`'s
/// single-package short-circuit, where `serialize_merge_order` (and thus
/// the normal `debug_dump_graph` call) never runs.
pub(crate) fn debug_dump_graph_only(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) {
    let g = build_digraph(entries, top_level_atoms, root, circular);
    debug_dump_graph(&g, entries, top_level_atoms, root);
}

pub(crate) fn debug_dump_graph(
    g: &Digraph,
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
) {
    let env = std::env::var_os("PORTUALE_DEBUG_MERGE_GRAPH").is_some();
    if !env && !crate::resolver_debug() {
        return;
    }
    // The env var predates the `--debug` wiring and went to stderr
    // directly; keep it working even without `--debug`.
    let emit = |s: String| {
        if crate::resolver_debug() {
            crate::resolver_trace::err(format_args!("{s}"));
        } else {
            eprint!("{s}");
        }
    };
    emit("\ndigraph:\n\n".to_string());

    let label = |i: usize| crate::resolver_trace::node_label(&entries[i], root, g.installed[i]);

    for &i in &g.order {
        if g.children[i].is_empty() {
            emit(format!("{} (no children)\n", label(i)));
        } else {
            emit(format!("{} depends on\n", label(i)));
            for (c, prios) in &g.children[i] {
                let max = crate::resolver_trace::max_priority(prios);
                emit(format!("  {} ({})\n", label(*c), max));
            }
        }
    }

    // Pseudo-`DependencyArg` nodes for the top-level atoms, appended
    // after the real graph (real interleaves them; portuale flattens set
    // expansion before the resolver, so it only has the concrete atoms).
    let mut cp_idx: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (j, e) in entries.iter().enumerate() {
        cp_idx
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(j);
    }
    for atom_str in top_level_atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        match cp_idx.get(&(atom.category.as_str(), atom.package.as_str())) {
            Some(idxs) if !idxs.is_empty() => {
                emit(format!("{atom_str} depends on\n"));
                for &j in idxs {
                    emit(format!("  {} (soft)\n", label(j)));
                }
            }
            _ => emit(format!("{atom_str} (no children)\n")),
        }
    }
}
