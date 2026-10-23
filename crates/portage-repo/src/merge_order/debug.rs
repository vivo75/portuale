//! `--debug` merge-order dumps and node-selection traces.

use crate::merge_order::*;
use crate::parse_dep_atom;

/// `PORTUALE_MO_SEL=1`: one `MO_SEL` line per `select_nodes` iteration to stderr.
/// Never on by default, so a normal run is byte-identical.
pub(crate) fn mo_sel_enabled() -> bool {
    std::env::var_os("PORTUALE_MO_SEL").is_some_and(|v| v != "0")
}

/// The version an entry is resolved at. `None` for `NoVisibleCandidate` and
/// `Uninstall` removals, which are never merge targets.
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

/// The `cat/pkg-ver` label for the trace `pick=` field, prefixed `m:` for a
/// merge-bound node or `n:` for a nomerge one.
pub(crate) fn mo_sel_cpv(e: &GraphEntry, installed: bool) -> String {
    let kind = if installed { 'n' } else { 'm' };
    match outcome_version(e) {
        Some(v) => format!("{kind}:{}/{}-{}", e.category, e.package, v),
        None => format!("{kind}:{}/{}", e.category, e.package),
    }
}

/// Arguments of [`mo_sel_trace_line`].
pub(crate) struct MoSelTraceLineArgs<'a> {
    pub(crate) iter: usize,
    pub(crate) retlist: usize,
    pub(crate) alive: usize,
    pub(crate) asap: &'a [String],
    pub(crate) prefer_asap: bool,
    pub(crate) drop_satisfied: bool,
    pub(crate) ig: Option<Ignore>,
    pub(crate) pick: &'a [String],
}

/// One `MO_SEL ` line (`PORTUALE_MO_SEL`) -- pure so the format is unit-pinned.
pub(crate) fn mo_sel_trace_line(args: MoSelTraceLineArgs<'_>) -> String {
    let MoSelTraceLineArgs {
        iter,
        retlist,
        alive,
        asap,
        prefer_asap,
        drop_satisfied,
        ig,
        pick,
    } = args;
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

/// Dumps the freshly-built digraph in Portage's `digraph.debug_print()` format,
/// to stderr under `--pretend --debug` (or `PORTUALE_DEBUG_MERGE_GRAPH`).
/// Portage: portage/util/digraph.py, debug_print
///
/// Portage's format, per node in insertion order (`g.order` before
/// `merge_order_bias` re-sorts it -- Portage's `debug_print` iterates
/// `self.nodes`, not `.order`):
///
/// ```text
/// (cat/pkg-ver:slot/sub_slot::repo, <state>) depends on
///   (child..., <state>) (<max-priority>)
/// (leaf..., <state>) (no children)
/// ```
/// The cycle's induced subgraph plus the reported leaf, to stderr under
/// `--pretend --debug`, once per relaxed runtime cycle.
/// Portage: depgraph.py, _serialize_tasks
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
    // The last node in `.order` within the subgraph is the reported leaf.
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
    // The env var predates the `--debug` wiring; keep it working without `--debug`.
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

    // Pseudo-`DependencyArg` nodes for the top-level atoms, appended last.
    let mut cp_idx: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (j, e) in entries.iter().enumerate() {
        cp_idx
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(j);
    }
    for atom_str in top_level_atoms {
        let Some(atom) = parse_dep_atom(atom_str, "debug_top_level_atoms") else {
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
