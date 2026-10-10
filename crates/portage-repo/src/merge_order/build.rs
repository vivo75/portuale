//! Building the digraph and biasing it toward Portage's order.

use crate::merge_order::*;

/// Builds the merge-order digraph out of the resolved `entries`.
///
/// Nodes are the entries; edges come from `GraphEntry::deps`. `required_by`
/// supplies fallback edges for owner relationships the forward walk missed.
/// Portage: depgraph.py, _create_graph
pub(crate) fn build_digraph(
    entries: &[GraphEntry],
    top_level_atoms: &[String],
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> Digraph {
    let n = entries.len();
    let pre = digraph_prelude(entries, root);
    let cp_indices = &pre.cp_indices;
    let installed = &pre.installed;

    let mut g = Digraph {
        n,
        children: vec![Vec::new(); n],
        parents: vec![Vec::new(); n],
        order: Vec::new(),
        installed: installed.clone(),
        alive: vec![true; n],
    };

    // Installed match for the atom, via the shared `edge_satisfied_with` rule.
    let installed_by_cp = installed_candidates_by_cp(root);
    let satisfied = |edge: &DepEdge, child: Option<usize>| -> bool {
        edge_satisfied_with(
            &installed_by_cp,
            edge,
            child.and_then(|ci| {
                entries[ci]
                    .slot
                    .as_deref()
                    .zip(entries[ci].sub_slot.as_deref())
            }),
        )
    };

    let edge_matches = |atom: &str, j: usize| pre.edge_matches(atom, j);

    // One `||` branch per group; shared with `kept_alt_branches`.
    let alt_suppressed = suppressed_alt_edges(entries, &pre, root, circular);

    // LIFO `dep_stack` seeded from the top-level atoms: a node is recorded
    // into `.order` when its parent's dep string first names it.
    let mut discovered = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let discover = |i: usize, discovered: &mut Vec<bool>, order: &mut Vec<usize>| -> bool {
        if discovered[i] {
            return false;
        }
        discovered[i] = true;
        order.push(i);
        true
    };
    for atom_str in top_level_atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        if let Some(idxs) = cp_indices.get(&(atom.category.as_str(), atom.package.as_str())) {
            for &i in idxs {
                if !edge_matches(atom_str, i) {
                    continue;
                }
                if discover(i, &mut discovered, &mut g.order) {
                    stack.push(i);
                }
            }
        }
    }
    // Drain the ordinary stack, then pop one disjunctive bundle; repeat.
    // Each bundle is one dep key's `||` / `virtual/*` atoms.
    let mut disjunctive_stack: Vec<(usize, u8)> = Vec::new();
    let expand = |i: usize,
                  disjunctive: bool,
                  key_filter: Option<u8>,
                  discovered: &mut Vec<bool>,
                  order: &mut Vec<usize>,
                  stack: &mut Vec<usize>| {
        for (ei, edge) in entries[i].deps.iter().enumerate() {
            if edge.disjunctive != disjunctive {
                continue;
            }
            if key_filter.is_some_and(|k| k != edge.key) {
                continue;
            }
            if alt_suppressed[i].contains(&ei) {
                continue;
            }
            let Some(idxs) = cp_indices.get(&(edge.category.as_str(), edge.package.as_str()))
            else {
                continue;
            };
            for &j in idxs {
                if !edge_matches(&edge.atom, j) {
                    continue;
                }
                if discover(j, discovered, order) {
                    stack.push(j);
                }
            }
        }
    };
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
            expand(i, false, None, &mut discovered, &mut g.order, &mut stack);
        }
        let Some((i, k)) = disjunctive_stack.pop() else {
            break;
        };
        expand(i, true, Some(k), &mut discovered, &mut g.order, &mut stack);
    }
    // Unreached entries keep array position, appended after discovered nodes.
    for (i, seen) in discovered.iter().enumerate() {
        if !seen {
            g.order.push(i);
        }
    }

    // Edges, forward first so `children` keeps dep-key/atom order.
    for (i, entry) in entries.iter().enumerate() {
        for (ei, edge) in entry.deps.iter().enumerate() {
            if alt_suppressed[i].contains(&ei) {
                continue;
            }
            // The single package the atom resolves to (`merge_bound_only = false`:
            // the scheduler digraph edges to nomerge nodes too).
            if let Some(j) = pre.select_dep_target(SelectDepTargetArgs {
                entries,
                from: i,
                from_deps: &entry.deps,
                ei,
                suppressed: &alt_suppressed[i],
                merge_bound_only: false,
            }) {
                // Drop direct self-edges unless an unsatisfied build-time dep.
                // Portage: depgraph.py, _add_pkg
                let sat = satisfied(edge, Some(j));
                if i == j && !(edge.priority.buildtime && !sat) {
                    continue;
                }
                let mut priority = edge.priority;
                priority.satisfied = sat;
                g.add_edge(i, j, priority);
            }
        }
    }
    // Fallback edges from `required_by` for owner relationships with no `deps` entry.
    for (j, e) in entries.iter().enumerate() {
        // A removal's `required_by` names the blocker owner it removes for, which
        // is ordered first: the dedicated pass adds the reversed edge instead.
        if matches!(e.outcome, PretendOutcome::Uninstall { .. }) {
            for owner in &e.required_by {
                let Some(owner_indices) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str()))
                else {
                    continue;
                };
                for &i in owner_indices {
                    if i == j || g.children[i].iter().any(|(c, _)| *c == j) {
                        continue;
                    }
                    // The owner is the removal's child, so the owner is selected first.
                    g.add_edge(
                        j,
                        i,
                        DepPriority {
                            buildtime: true,
                            runtime: true,
                            satisfied: false,
                            ..DepPriority::default()
                        },
                    );
                }
            }
            continue;
        }
        for owner in &e.required_by {
            let Some(owner_indices) = cp_indices.get(&(owner.0.as_str(), owner.1.as_str())) else {
                continue;
            };
            for &i in owner_indices {
                if i == j || g.children[i].iter().any(|(c, _)| *c == j) {
                    continue;
                }
                // Same owner set for every slot of a multi-slot cp: skip when the
                // owner already edges to another slot (one package per atom).
                if g.children[i].iter().any(|(c, _)| {
                    entries[*c].category == e.category && entries[*c].package == e.package
                }) {
                    continue;
                }
                // Never synthesise a two-cycle: the forward direction is authoritative.
                if g.children[j].iter().any(|(c, _)| *c == i) {
                    continue;
                }
                g.add_edge(
                    i,
                    j,
                    DepPriority {
                        runtime: true,
                        satisfied: g.installed[j],
                        ..DepPriority::default()
                    },
                );
            }
        }
    }
    g
}

/// The nomerge-root prune is deliberately not ported: this graph holds only the
/// merge closure, which is exactly Portage's post-prune node set.
/// Portage: depgraph.py, _serialize_tasks
/// Every `@system`-set member plus everything reachable by runtime edges only.
/// Portage: _find_deep_system_runtime_deps.py
pub(crate) fn deep_system_deps(
    g: &Digraph,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
) -> Vec<bool> {
    let system_cps: HashSet<(String, String)> = config
        .system_packages
        .iter()
        .filter_map(|a| portage_dep::parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();
    let mut deep = vec![false; g.n];
    let mut stack: Vec<usize> = Vec::new();
    for &i in &g.order {
        if system_cps.contains(&(entries[i].category.clone(), entries[i].package.clone())) {
            stack.push(i);
        }
    }
    while let Some(i) = stack.pop() {
        if deep[i] {
            continue;
        }
        deep[i] = true;
        for (c, prios) in &g.children[i] {
            if g.alive[*c] && prios.iter().any(|p| p.runtime || p.runtime_post) {
                stack.push(*c);
            }
        }
    }
    deep
}

/// Re-sorts `mygraph.order` so `@system`-deep runtime deps come first, then
/// highest to lowest reference count. `false` leaves discovery order alone.
/// Portage: depgraph.py, _merge_order_bias
pub(crate) fn merge_order_bias(
    g: &mut Digraph,
    entries: &[GraphEntry],
    config: &portage_profile::Config,
    implicit_system_deps: bool,
) {
    if !implicit_system_deps {
        return;
    }
    let deep = deep_system_deps(g, entries, config);
    let parent_count: Vec<usize> = (0..g.n)
        .map(|i| g.parents[i].iter().filter(|&&p| g.alive[p]).count())
        .collect();
    // Stable, so a tie keeps discovery order.
    g.order
        .sort_by_key(|&i| (!deep[i], std::cmp::Reverse(parent_count[i])));
}

// ---------------------------------------------------------------------
// the scheduler
// ---------------------------------------------------------------------

/// Seed libc / os-headers merges early: almost every ebuild has an implicit
/// build-time dependency on libc that Portage strips and re-expresses here.
/// Returns the merge-bound `virtual/libc` / `virtual/os-headers` entries and
/// their `RDEPEND` providers, os-headers first.
/// Portage: depgraph.py, _serialize_tasks (asap_nodes)
pub(crate) fn seed_toolchain_asap(entries: &[GraphEntry]) -> Vec<usize> {
    // Only genuine new-version merges seed `asap`, not bare `[ebuild R]` reinstalls.
    let new_cpv_merge = |i: usize| {
        matches!(
            entries[i].outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        )
    };
    let providers = |virt_pkg: &str| -> Vec<usize> {
        let Some(vi) = entries
            .iter()
            .position(|e| e.category == "virtual" && e.package == virt_pkg)
        else {
            return Vec::new();
        };
        let mut out: Vec<usize> = Vec::new();
        if new_cpv_merge(vi) {
            out.push(vi);
        }
        for edge in entries[vi].deps.iter().filter(|d| d.key == 0) {
            if let Some(pi) = entries
                .iter()
                .position(|e| e.category == edge.category && e.package == edge.package)
                && new_cpv_merge(pi)
                && !out.contains(&pi)
            {
                out.push(pi);
            }
        }
        out
    };
    let mut asap: Vec<usize> = Vec::new();
    for i in providers("os-headers").into_iter().chain(providers("libc")) {
        if !asap.contains(&i) {
            asap.push(i);
        }
    }
    asap
}

/// The closure of `node` under `ig`, or `None` when it escapes `mergeable`.
/// Portage: depgraph.py, _serialize_tasks (gather_deps)
pub(crate) fn gather_deps(
    g: &Digraph,
    node: usize,
    ig: Option<Ignore>,
    mergeable: &HashSet<usize>,
) -> Option<HashSet<usize>> {
    let mut sel: HashSet<usize> = HashSet::new();
    let mut stack = vec![node];
    while let Some(x) = stack.pop() {
        if sel.contains(&x) {
            continue;
        }
        if !mergeable.contains(&x) {
            return None;
        }
        sel.insert(x);
        stack.extend(g.child_nodes(x, ig));
    }
    Some(sel)
}
