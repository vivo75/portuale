//! Building the digraph and biasing it toward Portage's order.

use crate::merge_order::*;

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
