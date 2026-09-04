//! `--debug` merge-order dumps and node-selection traces.

use crate::merge_order::*;

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
