//! Building the digraph and biasing it toward Portage's order.

use crate::merge_order::*;

/// Builds the merge-order digraph out of the resolved `entries`.
///
/// Nodes are the entries themselves (portuale's own graph is already
/// one node per resolved `cat/pkg` slot); edges come from
/// `GraphEntry::deps`, which carries real's own per-key `DepPriority`.
/// `required_by` supplies a fallback edge for any owner relationship the
/// forward `deps` walk didn't record (a diamond dependency's second
/// owner, a synthetic rebuild entry, an entry whose metadata was
/// unreadable) so the new scheduler is never *less* constrained than the
/// `required_by`-only one it replaces.
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

    // Real `mypriority.satisfied`: an installed package matching the
    // atom, via the shared `edge_satisfied_with` rule (see its doc).
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

    // The per-entry candidate strings `edge_matches` narrows atoms
    // against -- see `digraph_prelude` for the full real grounding.
    let edge_matches = |atom: &str, j: usize| pre.edge_matches(atom, j);

    // Real `dep_zapdeps` (`dep_check.py`): a `|| ( … )` group resolves to
    // one alternative, not all. The derivation lives in
    // `suppressed_alt_edges` so the public `kept_alt_branches` used by the
    // #76 wait predicate cannot drift from this internal use.
    let alt_suppressed = suppressed_alt_edges(entries, &pre, root, circular);

    // Real `_create_graph`: an explicit LIFO `dep_stack` seeded from the
    // top-level atoms. A node is recorded into `.order` the moment its
    // parent's dep string first names it (forward, before any recursion),
    // then the *last*-pushed node is expanded first -- so each node's own
    // direct children land as one contiguous forward-order run, but the
    // deepest-declared sibling's subtree is numbered before its earlier
    // siblings'.
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
    // Real `_create_graph`'s own two-stack outer loop
    // (`depgraph.py:3254-3269`): the ordinary `_dep_stack` is drained
    // completely, and only then is the *last*-queued disjunctive bundle
    // popped off `_dep_disjunctive_stack` and expanded -- which pushes
    // fresh nodes back onto `_dep_stack`, so the whole thing repeats.
    // Each bundle is one dep key's worth of `|| ( … )` / `virtual/*`
    // atoms (`_queue_disjunctive_deps` queues at most one per key,
    // before that key's inline atoms are added).
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
    // An entry this walk never reaches -- a synthetic slot-operator /
    // `--rebuild-if-*` entry (no `deps` of its own), or an `--nodeps`
    // run -- keeps its original array position, appended after every
    // genuinely-discovered node.
    for (i, seen) in discovered.iter().enumerate() {
        if !seen {
            g.order.push(i);
        }
    }

    // Edges. Forward first, so `children` keeps real's own
    // dep-key/atom order (which `asap_nodes` and the cycle harvester
    // both read).
    for (i, entry) in entries.iter().enumerate() {
        for (ei, edge) in entry.deps.iter().enumerate() {
            if alt_suppressed[i].contains(&ei) {
                continue;
            }
            // The single package the atom resolves to -- shared with
            // `pretend.rs::print_tree` through `resolved_dep_targets`
            // (#82), so the ranking cannot drift between the scheduling
            // graph and the tree display. `merge_bound_only = false`:
            // real's scheduler digraph edges to nomerge nodes too.
            if let Some(j) = pre.select_dep_target(SelectDepTargetArgs {
                entries,
                from: i,
                from_deps: &entry.deps,
                ei,
                suppressed: &alt_suppressed[i],
                merge_bound_only: false,
            }) {
                // Real `_add_pkg`: a direct self-edge is dropped unless
                // it is an unsatisfied build-time dependency, "since
                // otherwise it can skew the merge order calculation in
                // an unwanted way" (`depgraph.py:3765-3770`).
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
    // Fallback edges from `required_by` for owner relationships the
    // forward walk has no `deps` entry for.
    for (j, e) in entries.iter().enumerate() {
        // #72 B3: a `Uninstall` removal's `required_by` names the blocker
        // owner it removes *for*, which real orders the removal **after**
        // (u1/u3: the owner's row, then `[uninstall]`). The generic
        // fallback would add owner -> removal (removal first), so the
        // dedicated pass below adds the reversed edge instead and this
        // loop skips the removal.
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
                    // `add_edge(j, i)`: the owner is the removal's child,
                    // so the owner is selected first (real's
                    // `_serialize_tasks` defers every uninstall node until
                    // it is scheduled, `get_nodes` `depgraph.py:9539-9549`).
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
                // `required_by` is keyed by `cat/pkg` only, so a
                // multi-slot dependency hands every scheduled slot the
                // same owner set. If the forward dep walk already gave
                // this owner a real edge to *another* slot of `j`'s
                // `cat/pkg`, that atom was slot-qualified and resolved
                // elsewhere -- don't synthesize a fallback edge to this
                // slot too (real resolves each atom to one package).
                if g.children[i].iter().any(|(c, _)| {
                    entries[*c].category == e.category && entries[*c].package == e.package
                }) {
                    continue;
                }
                // Track X Slice C (#242): `required_by` is cp-keyed and
                // root-blind, so one puller's cp fans out to every
                // scheduled instance -- including a same-cp sibling in
                // the other root that never pulled `j`. When the forward
                // walk already edged `j` back to this owner, synthesising
                // `i -> j` closes a two-node ring the walk never recorded
                // (the g216 target `g216comp` edges to the running one;
                // the fallback would edge the running one back, stranding
                // the whole chain behind a phantom cycle). The forward
                // direction is authoritative: a genuine relationship the
                // forward walk missed has no edge either way. Never
                // synthesise a two-cycle.
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

/// Real `_serialize_tasks`' own "Prune 'nomerge' root nodes if nothing
/// depends on them, since otherwise they slow down merge order
/// calculation" loop (`depgraph.py:9509-9518`) is deliberately **not**
/// ported, and this note records why.
///
/// Real's graph is seeded from `DependencyArg` nodes (`@world`,
/// `@selected`, `@system`, each explicit target), so before scheduling it
/// carries the entire installed universe those sets reach -- 1854 nodes
/// for the live case this module was validated against, of which the
/// prune removes 977. Portuale's graph is built from resolved
/// `GraphEntry`s instead: it has no arg nodes and never contains that
/// universe in the first place (461 nodes for the same case, which is
/// exactly real's own post-prune reachable-from-merge-tasks closure --
/// re-running the algorithm on just that closure reproduces real's merge
/// list identically).
///
/// What the prune *would* still remove here is a top-level
/// `AlreadyInstalled` entry -- a nomerge node with no parents. Real drops
/// those because it never displays one at all (confirmed live: `emerge -p
/// --noreplace <already-installed pkg>` prints nothing whatsoever).
/// Portuale does display them, and its own `--tree` / "package is already
/// installed" rendering wants them ordered after the dependencies they
/// pull in -- which is what leaving them in the scheduler produces, since
/// such a node only becomes a leaf once its whole subtree has been
/// emitted. Keeping them costs nothing in fidelity for the merge tasks
/// themselves: a parentless node is skipped by real's own "removing a
/// root node will not produce a leaf node, so avoid it" preference at
/// every relaxed `ignore_priority` rung anyway.
/// Real `_emerge/_find_deep_system_runtime_deps.py`: every `@system`-set
/// member in the graph, plus everything reachable from one by following
/// only *runtime*-priority edges (`RDEPEND`/`IDEPEND`/`PDEPEND`;
/// `DEPEND`/`BDEPEND` are dropped). Feeds `merge_order_bias`'s own
/// "system deps first" tier -- real: "promote deep system runtime
/// deps... for optimal leaf node selection".
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

/// Real `depgraph._merge_order_bias` (`depgraph.py:9274-9307`): re-sorts
/// `mygraph.order` in place so that, among simultaneously-eligible
/// leaves, `@system`-deep runtime deps come first and the rest go from
/// highest to lowest reference count. Real's own uninstalls-last rule
/// has nothing to apply to here (a `--pretend` merge graph has no
/// uninstall nodes).
///
/// `implicit_system_deps` is real `myparams["implicit_system_deps"]`
/// (`create_depgraph_params.py:120`, default on): `false` (real
/// `--implicit-system-deps=n`) takes real's own early return
/// (`depgraph.py:9279`) and leaves discovery order alone.
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
    // Stable, so a tie keeps discovery order -- real's own
    // `cmp_sort_key` comparator returns 0 for a tie and `list.sort` is
    // stable too.
    g.order
        .sort_by_key(|&i| (!deep[i], std::cmp::Reverse(parent_count[i])));
}

// ---------------------------------------------------------------------
// the scheduler
// ---------------------------------------------------------------------

/// Real `_serialize_tasks`' own libc / os-headers `asap_nodes` seeding
/// (`depgraph.py:9608-9638`, bug #303567 / #328317): merge libc as early
/// as possible -- almost every ebuild has an implicit build-time
/// dependency on it that real strips (`strip_libc_deps`) and re-expresses
/// as this ordering preference -- and pull an `os-headers` upgrade in
/// first when there is one.
///
/// Real walks `_expand_virt_from_graph(root, virtual/libc)` -> the
/// graphed `virtual/libc`'s own `RDEPEND` provider atoms ->
/// `_package_tracker.match` -> the provider package, keeping only
/// `pkg.operation == "merge"` (not already installed at that cpv).
/// Portuale's equivalent: find the graphed `virtual/libc` /
/// `virtual/os-headers` entry, take its `RDEPEND` (`key == 0`) provider
/// `cat/pkg`s, and return the merge-bound entries with those `cp`s.
/// `os-headers` providers first (bug #328317), then libc.
pub(crate) fn seed_toolchain_asap(entries: &[GraphEntry]) -> Vec<usize> {
    // Real: `pkg.operation == "merge" and not vardb.cpv_exists(pkg.cpv)`
    // -- a genuine new-version/upgrade merge, NOT a bare `[ebuild R]`
    // reinstall at a cpv already in the vdb. A `@world` re-emerge where
    // `virtual/libc` / `virtual/os-headers` just reinstall must not seed
    // `asap`, or they front-load past dep-free `@system` leaves like
    // `sys-devel/gnuconfig`.
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
        // The virtual itself, if it is a genuine merge (real
        // `_package_tracker.match` returns it too).
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

/// Real `_serialize_tasks`' `gather_deps`: the closure of `node` under
/// `ig`, or `None` when it escapes `mergeable`. "Recursively gather a
/// group of nodes that RDEPEND on eachother. This ensures that they are
/// merged as a group and get their RDEPENDs satisfied as soon as
/// possible."
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
