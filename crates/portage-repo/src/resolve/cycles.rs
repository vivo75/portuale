//! Circular dependencies.

use crate::*;

/// Finds the shortest **unbreakable** dependency cycle among the
/// merge-bound `entries` -- real `circular_dependency_handler`'s
/// `_find_cycles` + `shortest_cycle`, restricted to *hard* edges. Those
/// are the only edges real `_serialize_tasks`' `_ignore_runtime` scan
/// can't drop, so a cycle made entirely of them is exactly the case
/// real portage reports with `* Error: circular dependencies:`.
///
/// A hard edge is an unsatisfied build-time dep with no run-time
/// alternative, `edge_kind_map[(dep cp, owner cp)] == (true, false)`
/// -- or, since backlog #228, an unsatisfied non-optional
/// slot-operator edge (`DEPEND`/`RDEPEND` `:=`, real `DepPriority`
/// `buildtime_slot_op`/`runtime_slot_op`, which no
/// `ignore_priority` rung of either `DepPriority*Range` relaxes --
/// real `_emerge/DepPriorityNormalRange.py` / `_emerge/
/// DepPrioritySatisfiedRange.py`, grounded by the n228 probe: real
/// 3.0.81.3 aborts `dev-libs/slopcyca -RDEPEND:=-> dev-libs/slopcycb
/// -DEPEND-> dev-libs/slopcyca` with `(runtime_slot_op)` /
/// `(buildtime)`). A plain unsatisfied run-time edge stays soft on
/// purpose: both serializers merge a run-time cycle as a group
/// (`find_smallest_cycle`/`gather_deps`), so it never forces an
/// abort. The slot-op arm reads the owner's own `deps` edges (the
/// merge-order graph input, which carries real's per-key `DepPriority`
/// including the `:=` promotion) with real's own installed gate
/// (`dep_edge_satisfied_by_installed`, so a satisfied slot-op edge
/// stays breakable exactly like real's `satisfied` rung -- and, for a
/// slot-operator edge, only an installed instance in the child's own
/// slot/sub-slot satisfies, real `_add_pkg_dep_string`'s `inst_pkgs`
/// same-slot filter in `depgraph.py`); `DepEdge`
/// `priority.satisfied` itself is never consulted -- it is always
/// false on an entry (`build_digraph` only sets it on a local copy).
///
/// The hard-edge digraph has an edge `owner -> dep` for every such
/// `edge_kind_map` entry whose both endpoints are merge-bound `entries`
/// (plus every slot-op deps edge between two merge-bound entries, even
/// one the walk never recorded -- provided it sits on the `||` branch
/// the walk selected: real's digraph holds the resolved edge only, so
/// a suppressed alternative's `:=` edge never hardens, real
/// `dep_check.py::dep_zapdeps`). Branch selection reuses
/// `merge_order::kept_alt_branches`, the same set `build_digraph`
/// collapses to, so the two cannot disagree. Returns the shortest
/// directed cycle as an ordered CPV
/// list where each element depends on the next (the last wrapping to
/// the first), starting where real's `shortest_cycle[0]` starts
/// (backlog #278 -- `discovery` order stands in for real's digraph
/// insertion order; see the selection below); empty when the hard-edge
/// graph is acyclic (every ordinary resolve).
///
/// Full elementary-cycle enumeration lives separately
/// (`merge_order::elementary_cycles`, real `digraph.get_cycles` over the
/// `medium_soft` rung) and feeds only `large_cycle_count` and the
/// cycle-only re-display -- this stays the short hard ring the message
/// and the suggestions render.
///
/// Backlog #216: a same-cp edge is a cycle only when it targets the
/// owner's own instance. `owner_self_loop` is that test: one of the
/// owner's kept `deps` edges (inline, or on the `||` branch the walk
/// selected -- a suppressed alternative's edge is not in real's
/// digraph) names the owner's own `cat/pkg` and its atom matches the
/// owner's merge-bound cpv. `>=dev-lang/go-X` against in-graph `go-X`
/// loops; `=foo-2` against `foo-1` (a same-cp cross-slot edge, ordinary
/// in real's package-keyed digraph) does not.
///
/// Scope gate for the serialize-dead-end retry in `collect_feedback`
/// (`cycle_restartable`, below): only a fresh single-node self-loop
/// closed by a kept `||` self-branch retries. That is exactly the `||`
/// pick this slice ports (real's pass-1 `preferred_in_graph` self
/// choice, `dep_check.py`); everything else keeps today's
/// report-no-retry path:
/// - multi-node rings are backlog #221's general branch-switching scope
///   (G18, EAPI-gated), whose display-side (`kept_alt_branches`)
///   re-derivation and merge-vs-installed bookkeeping this slice does
///   not re-model;
/// - an inline (branch-less) self-dep has no alternative branch a retry
///   could switch to;
/// - an installed instance of the owner cp means the demoted branch
///   lands on installed subgraphs (slot_want unification, display
///   re-derivation), same scope cut.
///
/// `kept` is `kept_alt_branches` over the same entries.
pub(crate) fn cycle_restartable(
    entries: &[GraphEntry],
    kept: &[HashSet<usize>],
    cycle: &[String],
    root: &Path,
) -> bool {
    let [node_cpv] = cycle else {
        return false;
    };
    let Some((cat, pkg, _)) = split_cpv(node_cpv) else {
        return false;
    };
    // Fresh: no installed instance of the owner cp, so the demoted
    // retry only ever pulls fresh merges, never installed subgraphs.
    if !installed_candidates(root, &cat, &pkg).is_empty() {
        return false;
    }
    let Some(owner_idx) = entries.iter().position(|e| {
        e.category == cat && e.package == pkg && merge_bound_cpv(e).as_deref() == Some(node_cpv)
    }) else {
        return false;
    };
    let Some(owner_cpv) = merge_bound_cpv(&entries[owner_idx]) else {
        return false;
    };
    entries[owner_idx].deps.iter().enumerate().any(|(ei, d)| {
        d.alt.is_some()
            && kept[owner_idx].contains(&ei)
            && d.category == cat
            && d.package == pkg
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

/// Scope: self-loops touching an installed instance stay invisible
/// (like before this slice). Real deadlocks on them too, but only ever
/// as a transient: its `circular_dependency` retry re-resolves past
/// them, which needs the merge-vs-installed bookkeeping (slot_want
/// unification, display re-derivation) backlog #221's general
/// branch-switching scope (G18, EAPI-gated) owns. Reporting one here
/// would flip an outcome-correct resolve (the invisible self-edge never
/// constrained order or display) into a spurious circular abort, so the
/// retry gate (`cycle_restartable`, below) never sees them either --
/// fresh self-loops only, both sides consistent.
pub(crate) fn owner_self_loop(
    entries: &[GraphEntry],
    kept: &[std::collections::HashSet<usize>],
    oi: usize,
    root: &Path,
) -> bool {
    let Some(owner_cpv) = merge_bound_cpv(&entries[oi]) else {
        return false;
    };
    if !installed_candidates(root, &entries[oi].category, &entries[oi].package).is_empty() {
        return false;
    }
    entries[oi].deps.iter().enumerate().any(|(ei, d)| {
        (d.alt.is_none() || kept[oi].contains(&ei))
            && d.category == entries[oi].category
            && d.package == entries[oi].package
            && !d.priority.optional
            && portage_dep::match_from_list(&d.atom, &[owner_cpv.as_str()])
                .is_some_and(|m| !m.is_empty())
    })
}

pub(crate) fn find_hard_cycles(
    entries: &[GraphEntry],
    edge_kind_map: &EdgeKindMap,
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
    insertion_rank: Option<&[usize]>,
) -> Vec<Vec<String>> {
    // Merge-bound entries only, lowest index per cp (the merge list is
    // already in dependency order, so the first is the one to start a
    // rendered cycle at).
    let cp_index = merge_bound_index(entries);
    // Track X Slice C (#242): the per-(root, cp) resolution view (real's
    // package-keyed digraph). Single-root graphs resolve every endpoint
    // through the cp fallback below, exactly as before.
    let root_index = merge_bound_root_index(entries);
    // Backlog #228 slot-op arm: owner index -> dep indices with an
    // unsatisfied non-optional slot-operator `deps` edge between two
    // merge-bound entries. Only the `||` branch the walk selected
    // counts (`kept_alt_branches`, real `dep_zapdeps`' choice -- the
    // same set `build_digraph` collapses to): real's digraph holds the
    // resolved edge only, never a suppressed alternative's. The
    // installed gate is `dep_edge_satisfied_by_installed`, so a `:=`
    // edge is satisfied only by an installed instance in the dep
    // entry's own slot/sub-slot (real `_add_pkg_dep_string`'s
    // `inst_pkgs` same-slot filter in `depgraph.py`).
    let kept = kept_alt_branches(entries, root, circular);
    let slot_op_hard = |owner_idx: usize, dep_idx: usize| -> bool {
        entries[owner_idx].deps.iter().enumerate().any(|(ei, d)| {
            (d.alt.is_none() || kept[owner_idx].contains(&ei))
                && d.category == entries[dep_idx].category
                && d.package == entries[dep_idx].package
                && (d.priority.buildtime_slot_op || d.priority.runtime_slot_op)
                && !d.priority.optional
                && !dep_edge_satisfied_by_installed(root, d, Some(&entries[dep_idx]))
        })
    };
    // adjacency: owner index -> sorted, deduped dep indices (hard edges).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    for ((dep_cp, owner_cp), (has_hard, has_soft)) in edge_kind_map {
        let (Some(oi), Some(di)) = (
            root_cp_lookup(&root_index, &cp_index, owner_cp),
            root_cp_lookup(&root_index, &cp_index, dep_cp),
        ) else {
            continue;
        };
        // Backlog #228: the walk's `(true, false)` arm, or the
        // slot-op deps arm (an `RDEPEND` `:=` edge is never
        // `buildtime_hard`, so the map alone can never see it).
        let hard = (*has_hard && !*has_soft) || slot_op_hard(oi, di);
        if oi == di {
            // Backlog #216: a same-cp edge is a cycle only when it
            // targets the owner's own instance (real's digraph keys by
            // package, so `foo:1 → foo:2` is an ordinary edge while
            // `go → go` deadlocks `_serialize_tasks`). The kept
            // branch's atom decides, matched against the owner's
            // merge-bound cpv.
            if hard && owner_self_loop(entries, &kept, oi, root) {
                adj[oi].push(oi);
            }
            continue;
        }
        if hard {
            adj[oi].push(di);
        }
    }
    // A slot-op deps edge the walk never recorded has no map entry at
    // all -- it still counts when it sits on the branch the walk
    // selected (real's digraph holds that resolved edge); a suppressed
    // `||` alternative's edge is skipped above.
    for &oi in cp_index.values() {
        for (ei, dep) in entries[oi].deps.iter().enumerate() {
            if dep.alt.is_some() && !kept[oi].contains(&ei) {
                continue;
            }
            if !(dep.priority.buildtime_slot_op || dep.priority.runtime_slot_op)
                || dep.priority.optional
            {
                continue;
            }
            // Track X Slice C (#242): the child's resolving-root
            // instance (real resolves each atom against its group's
            // root); the cp fallback is the pre-slice behaviour.
            let Some(di) = dep_child_lookup(
                &root_index,
                &cp_index,
                entries[oi].targets_running_root,
                dep,
            ) else {
                continue;
            };
            // Backlog #216: same true-self-loop gate as the map arm
            // above -- a same-cp slot-op edge on a suppressed branch,
            // or one whose atom does not match the owner's own
            // instance, is no cycle.
            if di == oi && !owner_self_loop(entries, &kept, oi, root) {
                continue;
            }
            if !dep_edge_satisfied_by_installed(root, dep, Some(&entries[di]))
                && !adj[oi].contains(&di)
            {
                adj[oi].push(di);
            }
        }
    }
    // The iteration order analogue of real `digraph`'s node insertion
    // order: the scheduler's pre-bias rank when the caller threads it
    // (post-sort positions), else the resolver's `discovery` stamp, else
    // plain array position -- all stable sorts.
    let order_key = |i: usize| -> (usize, usize) {
        match insertion_rank {
            Some(ranks) => (ranks[i], i),
            None => (entries[i].discovery, i),
        }
    };
    for v in &mut adj {
        // Insertion order, not array position: the merge sort above
        // reorders cycle members (a dependent discovered later sorts
        // earlier), while real's child lists follow edge-insertion
        // order, i.e. resolution order.
        v.sort_by_key(|&i| order_key(i));
        v.dedup();
    }
    // Shortest directed cycle, chosen the way real
    // `circular_dependency_handler` chooses what to print (backlog
    // #278): real `digraph.get_cycles` (`portage/util/digraph.py:387`)
    // walks nodes in insertion order and, per node, closes a cycle
    // through each child with the shortest child-`>`node path; real
    // `_find_cycles` (`resolver/circular_dependency.py:48`) keeps the
    // first strictly-shortest one, and
    // `_prepare_circular_dep_message` (`:76`) prints from its `[0]`.
    // The scheduler's pre-bias insertion rank is the insertion-order
    // analogue (it re-discovers the graph from the top-level atoms the
    // way real `_create_graph` does, unlike the resolver's walk order,
    // which can pull a later argument's subtree first); without one,
    // `discovery` order stands in. The discovery-sorted adjacency
    // stands in for real's edge-insertion
    // child order (deterministic; real's BFS set order is hash-seeded,
    // but only same-length ties can differ, and per node only the first
    // of those can win the strict-shortest race).
    //
    // Only the display's start node changes: the winning length is the
    // same global minimum the per-start BFS found before, so the cycle
    // set (and, for two-node rings, the members) is untouched, and
    // empty stays empty (a start reaches itself iff one of its children
    // reaches it).
    let shortest_to = |from: usize, to: usize| -> Option<Vec<usize>> {
        if from == to {
            // Real `shortest_path(node, node)`: `bfs` yields the start
            // first, so the one-member path wins immediately.
            return Some(vec![from]);
        }
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut seen: HashSet<usize> = HashSet::from([from]);
        let mut frontier = vec![from];
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for &u in &frontier {
                for &w in &adj[u] {
                    if w == to {
                        // Reconstruct from -> ... -> u, then wrap to `to`.
                        let mut path = vec![u];
                        let mut cur = u;
                        while cur != from {
                            cur = prev[&cur];
                            path.push(cur);
                        }
                        path.reverse();
                        path.push(to);
                        return Some(path);
                    }
                    if seen.insert(w) {
                        prev.insert(w, u);
                        next.push(w);
                    }
                }
            }
            frontier = next;
        }
        None
    };
    let mut best: Option<Vec<usize>> = None;
    // Insertion rank order (the scheduler's pre-bias insertion order),
    // not array position: the merge sort above can place a
    // later-discovered dependent ahead of its puller.
    let mut by_insertion: Vec<usize> = (0..entries.len()).collect();
    by_insertion.sort_by_key(|&i| order_key(i));
    for &start in &by_insertion {
        if adj[start].is_empty() {
            continue;
        }
        let mut node_best: Option<Vec<usize>> = None;
        for &child in &adj[start] {
            if let Some(path) = shortest_to(child, start)
                && node_best.as_ref().is_none_or(|b| path.len() < b.len())
            {
                node_best = Some(path);
            }
        }
        if let Some(candidate) = node_best
            && best.as_ref().is_none_or(|b| candidate.len() < b.len())
        {
            best = Some(candidate);
        }
    }
    match best {
        None => Vec::new(),
        Some(cycle) => {
            vec![
                cycle
                    .into_iter()
                    .map(|i| merge_bound_cpv(&entries[i]).expect("merge-bound"))
                    .collect(),
            ]
        }
    }
}

/// Backlog #221: the restart-trigger half of real `_serialize_tasks`' no-leaf
/// handling (`_emerge/depgraph.py:10264-10296`).
///
/// Real records `backtrack_infos["config"]["circular_dependency"]` and
/// restarts whenever its leaf selection strands -- i.e. whenever the digraph
/// filtered by `DepPrioritySatisfiedRange.ignore_medium_soft` still holds a
/// cycle. That rung (`_ignore_satisfied_buildtime_slot_op`,
/// `DepPrioritySatisfiedRange.py:79-86`) keeps exactly the non-optional
/// edges real cannot satisfy without merging: unsatisfied `buildtime` AND
/// unsatisfied `runtime` alike (`return not priority.buildtime and not
/// priority.runtime` -- only an edge with neither flag, or a satisfied or
/// optional one, is ignored). A pure-runtime cycle never reaches the
/// recorder: `find_smallest_cycle` group-merges it first (no restart, no
/// error). So the trigger is a directed cycle over those kept edges that
/// contains at least one buildtime edge -- the shape a group merge can
/// never absorb, since a buildtime edge is unignorable at every rung.
///
/// Edge sources mirror `find_hard_cycles`: the walk's `edge_kind_map` hard
/// arm (precomputed unsatisfied-buildtime, cp-level) plus the selected
/// `||`-branch `deps` walk (`kept_alt_branches`, real's digraph holding
/// the resolved edge only), widened from buildtime-only to
/// buildtime-or-runtime with real's own installed gate
/// (`dep_edge_satisfied_by_installed`, so an installed-provided dep stays
/// breakable exactly like real's `satisfied` rung). `PDEPEND`-only
/// (`runtime_post` without `runtime`) edges stay out -- real's rung
/// ignores those too.
///
/// Returns one index cycle per hard edge that closes a loop (each element
/// depends on the next, the last wrapping to the first, the
/// `find_hard_cycles` convention, so the recorder below can reuse real's
/// `circular_child = cycle[index - 1]` rotation). One cycle per hard
/// edge rather than full elementary enumeration: disjoint co-cycles
/// converge over successive restarts (each retry strictly grows the
/// recorded map -- see the `unsolved_cycle` gate at the call site), while
/// enumeration would blow up on big graphs. Empty when no hard edge
/// closes a loop (every ordinary resolve, and every pure-runtime cycle).
pub(crate) fn find_restart_cycles(
    entries: &[GraphEntry],
    edge_kind_map: &EdgeKindMap,
    root: &Path,
    circular: &HashMap<RootCp, Vec<CircularDepChild>>,
) -> Vec<Vec<usize>> {
    let cp_index = merge_bound_index(entries);
    // Track X Slice C (#242): the per-(root, cp) resolution view (see
    // `find_hard_cycles`).
    let root_index = merge_bound_root_index(entries);
    // Trigger adjacency: owner index -> (dep index, hard). `hard` is an
    // unsatisfied buildtime edge; any entry is an unsatisfied
    // non-optional buildtime-or-runtime edge (real's kept set).
    let mut adj: Vec<Vec<(usize, bool)>> = vec![Vec::new(); entries.len()];
    let add_edge = |adj: &mut [Vec<(usize, bool)>], oi: usize, di: usize, hard: bool| {
        if oi == di {
            return;
        }
        match adj[oi].iter_mut().find(|(t, _)| *t == di) {
            Some(slot) => {
                slot.1 |= hard;
            }
            None => adj[oi].push((di, hard)),
        }
    };
    // The walk's hard arm, same reading as `find_hard_cycles`.
    for ((dep_cp, owner_cp), (has_hard, _)) in edge_kind_map {
        if !has_hard {
            continue;
        }
        let (Some(oi), Some(di)) = (
            root_cp_lookup(&root_index, &cp_index, owner_cp),
            root_cp_lookup(&root_index, &cp_index, dep_cp),
        ) else {
            continue;
        };
        add_edge(&mut adj, oi, di, true);
    }
    // The `deps` walk: selected branches only, real's installed gate.
    let kept = kept_alt_branches(entries, root, circular);
    for oi in 0..entries.len() {
        if merge_bound_cpv(&entries[oi]).is_none() {
            continue;
        }
        for (ei, d) in entries[oi].deps.iter().enumerate() {
            if d.alt.is_some() && !kept[oi].contains(&ei) {
                continue;
            }
            if d.priority.optional {
                continue;
            }
            if !(d.priority.buildtime || d.priority.runtime) {
                continue;
            }
            // Track X Slice C (#242): the child's resolving-root
            // instance (same rule as `find_hard_cycles`' slot-op arm).
            let Some(di) =
                dep_child_lookup(&root_index, &cp_index, entries[oi].targets_running_root, d)
            else {
                continue;
            };
            if dep_edge_satisfied_by_installed(root, d, Some(&entries[di])) {
                continue;
            }
            add_edge(&mut adj, oi, di, d.priority.buildtime);
        }
    }
    for v in &mut adj {
        v.sort_unstable();
    }
    // One BFS per hard edge: from the hard edge's target back to its
    // owner over trigger edges; success is a hard-containing cycle.
    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for (u, targets) in adj.iter().enumerate() {
        for &(v, hard) in targets {
            if !hard {
                continue;
            }
            // BFS v -> ... -> u.
            let mut prev: HashMap<usize, usize> = HashMap::new();
            let mut seen: HashSet<usize> = HashSet::from([v]);
            let mut frontier = vec![v];
            let mut found = false;
            while !frontier.is_empty() && !found {
                let mut next = Vec::new();
                for &x in &frontier {
                    for &(w, _) in &adj[x] {
                        if w == u {
                            prev.insert(w, x);
                            found = true;
                            break;
                        }
                        if seen.insert(w) {
                            prev.insert(w, x);
                            next.push(w);
                        }
                    }
                    if found {
                        break;
                    }
                }
                frontier = next;
            }
            if !found {
                continue;
            }
            // Walk u backwards to v: [u, ..., v]; reversed and
            // re-rooted at u it reads u -> v -> ... -> u.
            let mut rev = vec![u];
            let mut cur = u;
            while cur != v {
                cur = prev[&cur];
                rev.push(cur);
            }
            rev.reverse();
            rev.truncate(rev.len() - 1);
            let mut cycle = vec![u];
            cycle.extend(rev);
            cycles.push(cycle);
        }
    }
    cycles
}

/// Backlog #193: real `depgraph._virt_deps_visible`'s virtual-cycle rule
/// (`lib/_emerge/depgraph.py:6208-6222` in 3rdparty/portage 3.0.82.2).
/// While resolving a `virtual/` atom, real recursively probes the
/// selected provider's own `RDEPEND` closure
/// (`_virt_deps_visible_imp`, `:6224-6256`, reached through `dep_check`
/// → `_dep_check_composite_db.match_pkgs`, `:11833` → `_visible`,
/// `:11921` → `_virt_deps_visible`, `:11945`), and re-entering a
/// virtual already on the probe stack (`_virt_deps_visible_recursion`,
/// `:873`) raises `_virtual_cycle_error` (class `:11685`, `:6215-6216`).
/// `select_files` (`:5002-5013`) catches it into `_virtual_cycle`,
/// prints `\n\n!!! virtual cycle detected:\n\n` plus one
/// `  {cpv}::{repo}` line per stack member (sorted) plus `\n`
/// (`writemsg`, stderr), and fails the resolve (`_skip_restart`, no
/// merge list, exit 1). Upstream `test_virtual_cycle.py` (bug 965570):
/// `virtual/gzip-1` whose `RDEPEND` is itself, and the
/// `virtual/A-1 → B-1 → C-1 → A-1` ring, both fail (the playground
/// surfaces the members as `virtual_cycle`,
/// `ResolverPlayground.py:1241-1242`). Grounded live by the g193 probe
/// (`podman run localhost/test-portuale:latest`, portage 3.0.81.3 --
/// same message shape in 3.0.82.2 by source read --
/// `emerge -p --color=n app-misc/{bar,foo}` on the staged #50
/// fixtures, rc 1 both, members `virtual/gzip-1` /
/// `virtual/A-1,B-1,C-1` each with `::testrepo`).
///
/// Ported as a settled-graph check over the provider selection the walk
/// already made: DFS from every merge-bound `virtual/` entry along
/// kept-branch `RDEPEND` (`DepEdge.key == 0`, the only key real's imp
/// expands) edges to merge-bound `virtual/` targets; revisiting an
/// on-stack entry reports the whole stack -- real reports the full
/// recursion set, so a lollipop (a virtual pulling a self-looping
/// virtual) reports every virtual on the path, not just the loop.
/// Members return as `cpv::repo` lines in real's sorted order (cp, then
/// `vercmp` version, then repo). Empty when no merge-bound virtual
/// (transitively) requires itself; a `virtual/ → non-virtual →
/// virtual/` ring is NOT one (real's stack holds virtuals only, so
/// that shape probes clean) and stays an ordinary merge.
///
/// Documented cuts: a dep target that is not merge-bound
/// (`AlreadyInstalled`, unresolvable) is not expanded -- real probes
/// the installed/available package's own recorded `RDEPEND` there too,
/// but no fixture shapes an installed virtual with a cyclic recorded
/// `RDEPEND` that nothing merges; and a suppressed `||` alternative is
/// never probed -- real's `dep_check` may probe a non-selected branch
/// first and fail where this succeeds (no fixture shapes that
/// either). A virtual the walk considered but did not select is likewise
/// never a start node (real may probe such a candidate mid-search;
/// single-provider fixtures cannot tell).
pub(crate) fn find_virtual_cycles(entries: &[GraphEntry], root: &Path) -> Vec<String> {
    // Cheap gate: no merge-bound virtual, no probe (every ordinary resolve).
    if !entries
        .iter()
        .any(|e| e.category == "virtual" && merge_bound_cpv(e).is_some())
    {
        return Vec::new();
    }
    let cp_index = merge_bound_index(entries);
    // Branch selection reuses `kept_alt_branches`, the same set
    // `build_digraph` collapses to (real's digraph holds the resolved
    // edge only) -- computed lazily so resolves without a `||` edge on
    // any merge-bound virtual never touch the digraph prelude. The empty
    // map is backlog #193's own settled behaviour (this check predates
    // the circular backtrack map; its pins must stay byte-identical):
    // branch switching from a later backtrack pass is already reflected
    // in the walk's own selection, which is what this re-derives.
    let kept: Vec<HashSet<usize>> = if entries.iter().any(|e| {
        e.category == "virtual"
            && merge_bound_cpv(e).is_some()
            && e.deps.iter().any(|d| d.alt.is_some())
    }) {
        kept_alt_branches(entries, root, &HashMap::new())
    } else {
        vec![HashSet::new(); entries.len()]
    };
    // Successors: kept-branch RDEPEND edges to merge-bound virtual targets.
    let succ = |idx: usize| -> Vec<usize> {
        let mut out: Vec<usize> = entries[idx]
            .deps
            .iter()
            .enumerate()
            .filter(|(ei, d)| {
                d.key == 0 && d.category == "virtual" && (d.alt.is_none() || kept[idx].contains(ei))
            })
            .filter_map(|(_, d)| cp_index.get(&(d.category.as_str(), d.package.as_str())))
            .copied()
            .filter(|&t| entries[t].category == "virtual")
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    };
    fn visit(
        idx: usize,
        succ: &dyn Fn(usize) -> Vec<usize>,
        stack: &mut Vec<usize>,
        on_stack: &mut HashSet<usize>,
        done: &mut HashSet<usize>,
    ) -> Option<Vec<usize>> {
        for t in succ(idx) {
            // The revisit check is the `_virt_deps_visible_recursion`
            // membership test (`:6215`): the reported members are the
            // whole stack, like real's `list(recursion_set)`.
            if on_stack.contains(&t) {
                return Some(stack.clone());
            }
            if !done.contains(&t) {
                stack.push(t);
                on_stack.insert(t);
                if let Some(members) = visit(t, succ, stack, on_stack, done) {
                    return Some(members);
                }
                stack.pop();
                on_stack.remove(&t);
            }
        }
        done.insert(idx);
        None
    }
    for start in 0..entries.len() {
        if entries[start].category != "virtual" {
            continue;
        }
        if merge_bound_cpv(&entries[start]).is_none() {
            continue;
        }
        let mut stack = vec![start];
        let mut on_stack: HashSet<usize> = HashSet::from([start]);
        let mut done: HashSet<usize> = HashSet::new();
        if let Some(path) = visit(start, &succ, &mut stack, &mut on_stack, &mut done) {
            let mut members: Vec<(&str, &str, &str, &str)> = path
                .iter()
                .map(|&i| {
                    let e = &entries[i];
                    (
                        e.category.as_str(),
                        e.package.as_str(),
                        merge_bound_version(&e.outcome)
                            .expect("merge-bound")
                            .as_str(),
                        e.repo_name.as_deref().unwrap_or("unknown"),
                    )
                })
                .collect();
            // Real `sorted()` on `Package` (`Package.py:830`: cp, then
            // `vercmp` version); the `::repo` suffix plays no part there.
            members.sort_by(|a, b| {
                a.0.cmp(b.0)
                    .then(a.1.cmp(b.1))
                    .then(vercmp_ordering(a.2, b.2))
                    .then(a.3.cmp(b.3))
            });
            return members
                .into_iter()
                .map(|(c, p, v, r)| format!("{c}/{p}-{v}::{r}"))
                .collect();
        }
    }
    Vec::new()
}

/// Backlog #228: one label per edge of a `find_hard_cycles` cycle --
/// real `_prepare_circular_dep_message`
/// (`resolver/circular_dependency.py`, the `({pkg}
/// ({priorities[-1]}))` on every line after `depends on`).
/// `labels[i]` is the edge `cycle[i] -> cycle[(i + 1) % len]`: the
/// `DepPriority.__str__` (`_emerge/DepPriority.py`) of the hardest
/// `deps` priority the owner's entry carries toward the dep's cp.
/// Real keeps the per-edge priorities sorted by hardness
/// (`digraph.add`'s `bisect.insort`), so `[-1]` is the max -- taken
/// here with `max_by_key(dep_priority_rank)`, the ordering
/// `resolver_trace::max_priority` implements, rendered with the same
/// `dep_priority_str` the `--debug` digraph dump already prints per
/// edge. `satisfied` never affects a label (real ranks by
/// hardness only); a `||` group contributes every branch's priority
/// (real's digraph holds only the resolved branch -- an approximation,
/// immaterial whenever the branches share a key, which they do by
/// construction). `"buildtime"` when the owner carries no `deps` edge
/// there at all (a map-only edge from a `deps`-less synthetic entry;
/// every resolver-built cycle edge has its atom queued, so the
/// fallback never fires on a real resolve).
pub fn cycle_edge_labels(entries: &[GraphEntry], cycle: &[String]) -> Vec<String> {
    if cycle.is_empty() {
        return Vec::new();
    }
    cycle
        .iter()
        .enumerate()
        .map(|(i, owner_cpv)| {
            let dep_cpv = &cycle[(i + 1) % cycle.len()];
            let hardest = entries
                .iter()
                .find(|e| merge_bound_cpv(e).as_deref() == Some(owner_cpv.as_str()))
                .into_iter()
                .flat_map(|e| e.deps.iter())
                .filter_map(|d| {
                    split_cpv(dep_cpv).and_then(|(cat, pkg, _)| {
                        (d.category == cat && d.package == pkg).then_some(d.priority)
                    })
                })
                .max_by_key(crate::resolver_trace::dep_priority_rank);
            hardest
                .map(|p| crate::resolver_trace::dep_priority_str(&p).to_string())
                .unwrap_or_else(|| "buildtime".to_string())
        })
        .collect()
}

/// Split a `category/package-version` string into its three parts (the
/// last `-`-separated boundary whose right half `ververify`s).
pub fn split_cpv(cpv: &str) -> Option<(String, String, String)> {
    let (cat, rest) = cpv.split_once('/')?;
    let parts: Vec<&str> = rest.split('-').collect();
    for i in 1..parts.len() {
        let version = parts[i..].join("-");
        if portage_versions::ververify(&version) {
            return Some((cat.to_string(), parts[..i].join("-"), version));
        }
    }
    None
}

/// Every USE flag name referenced anywhere in a `REQUIRED_USE` string --
/// real `portage.dep.get_required_use_flags`, as a plain token scan
/// (strip a trailing `?`, a leading `!`, skip the grouping operators).
pub(crate) fn required_use_flag_names(required_use: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for tok in required_use.split_whitespace() {
        let t = tok.strip_suffix('?').unwrap_or(tok);
        let t = t.strip_prefix('!').unwrap_or(t);
        if matches!(t, "(" | ")" | "||" | "^^" | "??" | "") {
            continue;
        }
        if t.starts_with(|c: char| c.is_ascii_alphanumeric())
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+_@-".contains(c))
        {
            out.insert(t.to_string());
        }
    }
    out
}

/// One entry of real `circular_dependency_handler.suggestions`: a
/// specific `Change USE:` fix that would break the cycle at one edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircularSuggestion {
    /// `category/package-version` of the package whose USE would change.
    pub parent_cpv: String,
    /// `(flag, enable)` diffs from the package's current USE, sorted.
    pub changes: Vec<(String, bool)>,
    /// Real `followup_change`: a grandparent's atom conditionally
    /// constrains one of these flags, so the fix may cascade upward.
    pub followup: bool,
}

/// Rust port of `circular_dependency_handler._find_suggestions`
/// (`lib/_emerge/resolver/circular_dependency.py:114`): for each edge of
/// the shortest cycle, the minimal `package.use` changes on the
/// *depending* package that would drop the offending atom from its
/// build-time deps without violating its own `REQUIRED_USE`. Returns them
/// deduped and sorted (real iterates a `set()`, order undefined -- this
/// crate sorts for a deterministic render, the same choice made
/// everywhere else portuale diverges from a real non-determinism).
///
/// Simplifications vs real, all documented in `docs/history/find-suggestions-plan.md`:
/// `_pkg_use_enabled` is `effective_use_flags` with the autounmask-USE
/// overlay applied (real `_emerge/depgraph.py:7669` returns the
/// `_needed_use_config_changes` set, autounmask included);
/// autounmask-*changed* flags are still honoured as untouchable (real
/// `resolver/circular_dependency.py:104-114`); the grandparent-atom set
/// is re-derived by scanning each puller entry's raw
/// `DEPEND`/`BDEPEND`/`RDEPEND`/`PDEPEND` for atoms on `parent`'s
/// `cat/pkg` rather than real's exact recorded `_parent_atoms`.
pub fn circular_dep_solutions(
    cycle: &[String],
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    autounmask_use_changes: &[AutounmaskChange],
    entries: &[GraphEntry],
) -> Vec<CircularSuggestion> {
    const MAX_AFFECTING_USE: usize = 10;
    let mut out: Vec<CircularSuggestion> = Vec::new();
    let n = cycle.len();
    if n == 0 {
        return out;
    }

    for pos in 0..n {
        let parent_cpv = &cycle[(pos + n - 1) % n];
        let child_cpv = &cycle[pos];
        let (Some((pcat, ppkg, pver)), Some((ccat, cpkg, _))) =
            (split_cpv(parent_cpv), split_cpv(child_cpv))
        else {
            continue;
        };
        let Ok(pcands) = list_candidates(repos, &pcat, &ppkg) else {
            continue;
        };
        let Some(pc) = pcands.iter().find(|c| c.version == pver) else {
            continue;
        };
        let Ok(md) = repo_aux_metadata(&pc.repo_location, &pcat, &format!("{ppkg}-{pver}")) else {
            continue;
        };
        let get = |k: &str| md.get(k).map(String::as_str).unwrap_or("");
        let dep = format!("{} {}", get("DEPEND"), get("BDEPEND"));
        let dep_tokens: Vec<String> = dep.split_whitespace().map(str::to_string).collect();
        let required_use = get("REQUIRED_USE");
        let iuse_names: HashSet<String> = pc
            .iuse
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .collect();
        let parent_str = format!(
            "{pcat}/{ppkg}-{pver}:{}/{}::{}",
            pc.slot, pc.sub_slot, pc.repo_name
        );
        // Real `_pkg_use_enabled` (`_emerge/depgraph.py:7669`) returns the
        // `_needed_use_config_changes` set when the parent carries one --
        // the autounmask-USE overlay applied on top of the configured
        // USE. The changed flags themselves stay untouchable below (real
        // `_get_autounmask_changes`, `circular_dependency.py:104`).
        let mut parent_use: HashSet<String> =
            effective_use_flags(config, &pc.iuse, &pc.keywords, &parent_str, &pcat, &ppkg)
                .as_ref()
                .clone();
        for ch in autounmask_use_changes {
            if matches_config_entry(&ch.atom, &parent_str, &pcat, &ppkg) {
                for tok in ch.token.split_whitespace() {
                    if let Some(flag) = tok.strip_prefix('-') {
                        parent_use.remove(flag);
                    } else {
                        parent_use.insert(tok.trim_start_matches('+').to_string());
                    }
                }
            }
        }

        // `parent_atom`: the (unevaluated) token in `dep` that pulls the
        // child -- real `all_parent_atoms[pkg]` filtered to this parent,
        // `.unevaluated_atom`.
        let ccp = format!("{ccat}/{cpkg}");
        let Some(parent_atom) = dep_tokens.iter().find(|t| {
            portage_dep::parse_atom(t)
                .is_some_and(|a| format!("{}/{}", a.category, a.package) == ccp)
        }) else {
            continue;
        };
        let parent_atom = parent_atom.clone();

        let Some(mut affecting) = portage_dep::extract_affecting_use(&dep, &parent_atom) else {
            continue;
        };

        // untouchable = use.mask ∪ use.force ∪ autounmask-changed
        let mut untouchable =
            forced_or_masked_flags(&pc.iuse, &pc.keywords, &parent_str, &pcat, &ppkg, config);
        for ch in autounmask_use_changes {
            if matches_config_entry(&ch.atom, &parent_str, &pcat, &ppkg) {
                for tok in ch.token.split_whitespace() {
                    untouchable.insert(tok.trim_start_matches(['+', '-']).to_string());
                }
            }
        }
        affecting.retain(|f| !untouchable.contains(f));

        // REQUIRED_USE entanglement (bug #374397 -- bounded).
        let ruf = required_use_flag_names(required_use);
        if affecting.iter().any(|f| ruf.contains(f)) {
            let mut total: HashSet<String> = affecting.clone();
            total.extend(ruf.iter().cloned());
            total.retain(|f| !untouchable.contains(f));
            if total.len() <= MAX_AFFECTING_USE {
                affecting = total;
            }
        }

        if affecting.is_empty() {
            continue;
        }
        let mut affecting: Vec<String> = affecting.into_iter().collect();
        affecting.sort();
        if affecting.len() > MAX_AFFECTING_USE {
            // bug #555698: drop irrelevant not-currently-enabled flags.
            affecting.retain(|f| parent_use.contains(f));
            if affecting.len() > MAX_AFFECTING_USE {
                continue;
            }
        }

        // Every enable/disable assignment of the affecting flags whose
        // reduced `dep` no longer contains `parent_atom` and still
        // satisfies `REQUIRED_USE`, recorded as a minimal diff.
        let n_aff = affecting.len();
        let mut solutions: HashSet<Vec<(String, bool)>> = HashSet::new();
        for mask in 0u32..(1u32 << n_aff) {
            let mut cur = parent_use.clone();
            for (i, f) in affecting.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    cur.insert(f.clone());
                } else {
                    cur.remove(f);
                }
            }
            let Ok(reduced) = portage_use_reduce::use_reduce_flat(
                &dep_tokens,
                &cur,
                portage_use_reduce::MatchMode::Normal,
            ) else {
                continue;
            };
            if reduced.contains(&parent_atom) {
                continue;
            }
            if !required_use.trim().is_empty()
                && !matches!(
                    portage_required_use::check_required_use(required_use, &cur, &iuse_names),
                    Ok(true)
                )
            {
                continue;
            }
            let mut sol: Vec<(String, bool)> = Vec::new();
            for (i, f) in affecting.iter().enumerate() {
                let enabled = mask & (1 << i) != 0;
                if enabled && !parent_use.contains(f) {
                    sol.push((f.clone(), true));
                } else if !enabled && parent_use.contains(f) {
                    sol.push((f.clone(), false));
                }
            }
            sol.sort();
            solutions.insert(sol);
        }

        let all: Vec<Vec<(String, bool)>> = solutions.into_iter().collect();
        for sol in &all {
            // Drop a solution that's a strict superset of another.
            let sol_set: HashSet<&(String, bool)> = sol.iter().collect();
            if all
                .iter()
                .any(|o| o != sol && o.iter().all(|x| sol_set.contains(x)))
            {
                continue;
            }
            let (ignore, followup) = grandparent_use_conflict(&pcat, &ppkg, sol, repos, entries);
            if ignore {
                continue;
            }
            out.push(CircularSuggestion {
                parent_cpv: format!("{pcat}/{ppkg}-{pver}"),
                changes: sol.clone(),
                followup,
            });
        }
    }

    out.sort_by(|a, b| {
        (a.parent_cpv.as_str(), &a.changes).cmp(&(b.parent_cpv.as_str(), &b.changes))
    });
    out.dedup();
    out
}

/// Real `_find_suggestions` step 9: does a grandparent's own atom on
/// `parent`'s `cat/pkg` constrain a flag in `solution`? A hard `[flag]` /
/// `[-flag]` clash disqualifies the solution; a conditional
/// (`flag?`/`flag=`) clash only flags `followup_change`. Grandparents are
/// re-derived by scanning each puller entry's raw dep strings (see
/// `circular_dep_solutions`' own doc comment).
pub(crate) fn grandparent_use_conflict(
    pcat: &str,
    ppkg: &str,
    solution: &[(String, bool)],
    repos: &[RepoConfig],
    entries: &[GraphEntry],
) -> (bool, bool) {
    let pcp = format!("{pcat}/{ppkg}");
    let mut followup = false;
    let Some(pe) = entries
        .iter()
        .find(|e| e.category == pcat && e.package == ppkg)
    else {
        return (false, false);
    };
    for (ocat, opkg) in &pe.required_by {
        let Some(over) = entries
            .iter()
            .find(|e| &e.category == ocat && &e.package == opkg)
            .and_then(merge_bound_cpv)
            .and_then(|cpv| split_cpv(&cpv).map(|(_, _, v)| v))
        else {
            continue;
        };
        let Ok(ocands) = list_candidates(repos, ocat, opkg) else {
            continue;
        };
        let Some(oc) = ocands.iter().find(|c| c.version == over) else {
            continue;
        };
        let Ok(omd) = repo_aux_metadata(&oc.repo_location, ocat, &format!("{opkg}-{over}")) else {
            continue;
        };
        for key in ["DEPEND", "BDEPEND", "RDEPEND", "PDEPEND"] {
            let Some(dep) = omd.get(key) else { continue };
            for tok in dep.split_whitespace() {
                let Some(a) = portage_dep::parse_atom(tok) else {
                    continue;
                };
                if format!("{}/{}", a.category, a.package) != pcp {
                    continue;
                }
                let Some(use_deps) = &a.use_deps else {
                    continue;
                };
                for (flag, _) in solution {
                    for ud in use_deps {
                        if &ud.flag != flag {
                            continue;
                        }
                        match ud.op {
                            portage_dep::UseDepOp::Enabled | portage_dep::UseDepOp::Disabled => {
                                return (true, followup);
                            }
                            _ => followup = true,
                        }
                    }
                }
            }
        }
    }
    (false, followup)
}
