//! The order a list of slots is shown in: each slot after the slots it
//! needs, one row per slot. A dependency cycle is cut at a cuttable
//! runtime-only edge when it has one; a cycle with none is genuine and is
//! ordered implicitly, then reported for the checker to fail (charter 2a).
//! No slot is ever listed twice.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{Change, DEP_KEYS, SlotSet, World};

/// How [`World::ordered`] sorts the slots.
pub enum Order<'a> {
    /// By `category/package`, then slot. No claim about build order.
    Alpha,
    /// A slot after the slots it needs before it is merged or usable
    /// (`DEPEND`, `BDEPEND`, `RDEPEND`, `IDEPEND`). A `PDEPEND`-only edge
    /// puts no constraint on the order (PMS). A cycle is cut at a cuttable
    /// runtime-only edge when it has one (a runtime edge out of a package
    /// the plan builds with still orders, PMS 8.1); a cycle with none is
    /// genuine: the walk below orders it implicitly and
    /// [`build_time_cycles`] reports it, for the checker to fail (charter
    /// 2a: a cycle no order satisfies is reported, never papered over by
    /// building a package twice).
    Depend,
    /// By the position in an order the caller got elsewhere (the resolver's
    /// merge list), keyed `cat/pkg:slot`; a slot not in it comes last, in
    /// alphabetical order.
    Rank(&'a HashMap<String, usize>),
}

impl World {
    /// What a plan does to `slot`: its own change, else a rebuild.
    #[must_use]
    pub fn change(&self, slot: &SlotSet) -> Option<Change> {
        slot.change().or_else(|| {
            self.rebuilds
                .contains(&slot.node())
                .then_some(Change::Rebuild)
        })
    }

    /// The slots `@world` needs that a plan would change, in `how` order.
    /// One row per slot: no slot is ever listed twice (charter 2a is
    /// report-only — a cycle no order satisfies is failed by the checker,
    /// never papered over by building a package twice).
    #[must_use]
    pub fn ordered(&self, how: &Order<'_>) -> Vec<&SlotSet> {
        let mut changed: Vec<&SlotSet> = self
            .slots
            .iter()
            .filter(|s| self.reached.contains(&s.node()) && self.change(s).is_some())
            .collect();
        changed.sort_by_key(|s| s.node());
        match how {
            Order::Alpha => changed,
            Order::Rank(rank) => {
                changed.sort_by_key(|s| rank.get(&s.node()).copied().unwrap_or(usize::MAX));
                changed
            }
            Order::Depend => {
                let by_node: HashMap<String, &SlotSet> =
                    changed.iter().map(|s| (s.node(), *s)).collect();
                let edges = self.ordering_edges();
                let order = depend_order(&edges, &self.edge_kinds);
                order
                    .into_iter()
                    .filter_map(|name| by_node.get(&name).copied())
                    .collect()
            }
        }
    }
}

impl World {
    /// `edges` without the edges that only a `PDEPEND` names.
    fn ordering_edges(&self) -> BTreeMap<String, BTreeSet<String>> {
        let post = 1u8 << DEP_KEYS.iter().position(|k| *k == "PDEPEND").unwrap_or(0);
        self.edges
            .iter()
            .map(|(node, to)| {
                let kept = to
                    .iter()
                    .filter(|dep| {
                        self.edge_kinds
                            .get(&(node.clone(), (*dep).clone()))
                            .is_none_or(|mask| mask & !post != 0)
                    })
                    .cloned()
                    .collect();
                (node.clone(), kept)
            })
            .collect()
    }
}

/// Every node of `edges` (and every node it points at), each after the
/// nodes it points at. Walks in name order, so the result is the same on
/// every run. A cycle is cut at the first cuttable runtime-only edge (a
/// runtime edge out of a package the plan builds with still orders, so it is
/// not cuttable); a cycle with no cuttable edge is genuine: no cut is
/// chosen for it, the walk below orders it implicitly, and
/// [`build_time_cycles`] reports it for the checker to fail. `kinds`
/// carries the `*DEPEND` variables behind each edge, as bits of
/// [`crate::DEP_KEYS`]; an edge without one counts as build-time.
pub(crate) fn depend_order(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
) -> Vec<String> {
    let build_used = build_time_used(kinds);
    // Cutting one runtime edge can open the next cycle: repeat until every
    // remaining cycle is all build-time, then order what is left.
    let mut working = edges.clone();
    while let Some((from, to)) = runtime_cut(&working, kinds, &build_used) {
        if let Some(next) = working.get_mut(&from) {
            next.remove(&to);
        }
    }
    unordered(&working)
}

/// The walk of old: every node after the nodes it points at, a cycle cut
/// where the walk meets it.
fn unordered(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut nodes: BTreeSet<&String> = edges.keys().collect();
    nodes.extend(edges.values().flatten());
    let mut done: HashSet<&String> = HashSet::new();
    let mut out = Vec::new();
    for start in nodes {
        if done.contains(start) {
            continue;
        }
        // (node, next child index): an explicit stack, the closure is deep.
        let mut stack: Vec<(&String, Vec<&String>, usize)> = Vec::new();
        done.insert(start);
        stack.push((start, children(edges, start), 0));
        while let Some((node, kids, next)) = stack.last_mut() {
            if let Some(kid) = kids.get(*next).copied() {
                *next += 1;
                if done.insert(kid) {
                    let k = children(edges, kid);
                    stack.push((kid, k, 0));
                }
            } else {
                out.push((*node).clone());
                stack.pop();
            }
        }
    }
    out
}

fn children<'a>(edges: &'a BTreeMap<String, BTreeSet<String>>, node: &String) -> Vec<&'a String> {
    edges
        .get(node)
        .map_or_else(Vec::new, |s| s.iter().collect())
}

/// The `*DEPEND` bits that order a package before what needs it at build time.
fn build_mask() -> u8 {
    portage_use_reduce::BUILD_KIND_MASK
}

/// Whether the edge only carries runtime `*DEPEND` variables.
fn is_runtime_edge(kinds: &BTreeMap<(String, String), u8>, edge: &(String, String)) -> bool {
    kinds.get(edge).is_some_and(|mask| mask & build_mask() == 0)
}

/// The nodes the plan builds with: the targets of a `DEPEND`, `BDEPEND` or
/// `IDEPEND` edge, except `virtual/*` targets. PMS 8.1 makes such a package
/// usable, so its `RDEPEND` closure constrains the order too. Excluding
/// virtuals is a heuristic that keeps the perl/libcrypt rebuild cluster
/// ordered, not PMS soundness: `BDEPEND virtual/foo` makes the provider a
/// build input in fact, so a kept virtual's provider edge stays cuttable
/// even when the installed provider is broken. The known hole (and the real
/// fix) is filed as #751. One pass: build-time use must not propagate
/// through `RDEPEND`, or every cycle edge would count as build-time in fact
/// and no cycle could be cut.
pub(crate) fn build_time_used(kinds: &BTreeMap<(String, String), u8>) -> BTreeSet<String> {
    kinds
        .iter()
        .filter(|(_, mask)| *mask & build_mask() != 0)
        .map(|((_, to), _)| to.clone())
        .filter(|to| !to.starts_with("virtual/"))
        .collect()
}

/// Whether the runtime-only edge can be cut to serialize a cycle: an edge
/// out of a package the plan builds with carries a build-time need in fact
/// (backlog #746), so cutting it would build against an unusable package.
pub(crate) fn is_cuttable_runtime_edge(
    kinds: &BTreeMap<(String, String), u8>,
    build_used: &BTreeSet<String>,
    edge: &(String, String),
) -> bool {
    is_runtime_edge(kinds, edge) && !build_used.contains(&edge.0)
}

/// The cut the first cycle the walk meets needs, if it is cyclic. The walk
/// is in name order, so the cut is the same on every run. `None` when the
/// graph is acyclic.
fn runtime_cut(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
    build_used: &BTreeSet<String>,
) -> Option<(String, String)> {
    let mut done: HashSet<&String> = HashSet::new();
    let mut walking: HashSet<&String> = HashSet::new();
    let mut nodes: BTreeSet<&String> = edges.keys().collect();
    nodes.extend(edges.values().flatten());
    for start in nodes {
        if done.contains(start) {
            continue;
        }
        done.insert(start);
        walking.insert(start);
        // (node, next child index): an explicit stack, the closure is deep.
        let mut stack: Vec<(&String, Vec<&String>, usize)> = Vec::new();
        stack.push((start, children(edges, start), 0));
        while let Some((_, kids, next)) = stack.last_mut() {
            if let Some(kid) = kids.get(*next).copied() {
                *next += 1;
                if walking.contains(kid) {
                    if let Some(cut) = cycle_cut(&stack, kid, kinds, build_used) {
                        return Some(cut);
                    }
                } else if done.insert(kid) {
                    walking.insert(kid);
                    stack.push((kid, children(edges, kid), 0));
                }
            } else {
                let (node, _, _) = stack.pop().expect("a node is being walked");
                walking.remove(node);
            }
        }
    }
    None
}

/// What the cycle the walk just closed needs: the stack from `kid` (the
/// ancestor the back edge points at) to the current node, closed by the back
/// edge last. The first cuttable runtime-only edge in that order; the back
/// edge itself when only it is cuttable; `None` when nothing is: no cuttable
/// edge means the cycle is genuine, the walk below orders it implicitly and
/// [`build_time_cycles`] reports it.
fn cycle_cut(
    stack: &[(&String, Vec<&String>, usize)],
    kid: &String,
    kinds: &BTreeMap<(String, String), u8>,
    build_used: &BTreeSet<String>,
) -> Option<(String, String)> {
    let start = stack.iter().position(|(n, _, _)| *n == kid)?;
    let mut cycle: Vec<(String, String)> = stack[start..]
        .windows(2)
        .map(|w| (w[0].0.clone(), w[1].0.clone()))
        .collect();
    cycle.push((stack.last()?.0.clone(), kid.clone()));
    cycle
        .into_iter()
        .find(|edge| is_cuttable_runtime_edge(kinds, build_used, edge))
}

/// The build-time cycles of `edges`: strongly connected components over
/// build-kind edges only (`build_mask`; an edge without kind information
/// counts as build-time, as before), members in walk order, deterministic.
/// Empty means every build need is serializable; non-empty is genuine: the
/// plan-world report reads this, nothing else, and the checker fails it
/// (charter 2a: a cycle no order satisfies is reported, never papered over
/// by building a package twice).
pub fn build_time_cycles(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
) -> Vec<Vec<String>> {
    let mask = build_mask();
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    let mut kids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (from, to) in edges {
        nodes.insert(from.clone());
        let mut kept: Vec<String> = Vec::new();
        for t in to {
            nodes.insert(t.clone());
            if kinds
                .get(&(from.clone(), t.clone()))
                .is_none_or(|m| m & mask != 0)
            {
                kept.push(t.clone());
            }
        }
        kids.insert(from.clone(), kept);
    }
    // Tarjan, iterative (the closure is deep): `at` is also the walk order,
    // so popping a component root lists its members in walk order.
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut low: HashMap<String, usize> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut on_stack: HashSet<String> = HashSet::new();
    let mut next = 0usize;
    let mut cycles: Vec<Vec<String>> = Vec::new();
    for root in &nodes {
        if at.contains_key(root) {
            continue;
        }
        at.insert(root.clone(), next);
        low.insert(root.clone(), next);
        next += 1;
        stack.push(root.clone());
        on_stack.insert(root.clone());
        let mut work: Vec<(String, usize)> = vec![(root.clone(), 0)];
        while let Some((node, i)) = work.last_mut() {
            let kid = kids.get(node.as_str()).and_then(|k| k.get(*i)).cloned();
            if let Some(kid) = kid {
                *i += 1;
                if !at.contains_key(&kid) {
                    at.insert(kid.clone(), next);
                    low.insert(kid.clone(), next);
                    next += 1;
                    stack.push(kid.clone());
                    on_stack.insert(kid.clone());
                    work.push((kid, 0));
                } else if on_stack.contains(&kid) {
                    let low_node = low[node.as_str()].min(at[&kid]);
                    low.insert(node.clone(), low_node);
                }
            } else {
                let (node, _) = work.pop().expect("a node is being walked");
                if let Some((parent, _)) = work.last() {
                    let low_parent = low[parent.as_str()].min(low[node.as_str()]);
                    low.insert(parent.clone(), low_parent);
                }
                if low[node.as_str()] == at[node.as_str()] {
                    let mut members: Vec<String> = Vec::new();
                    while let Some(top) = stack.pop() {
                        on_stack.remove(&top);
                        let done = top == node;
                        members.push(top);
                        if done {
                            break;
                        }
                    }
                    members.reverse();
                    // A lone node is no cycle — unless it builds against
                    // itself.
                    if members.len() > 1
                        || kids.get(node.as_str()).is_some_and(|k| k.contains(&node))
                    {
                        cycles.push(members);
                    }
                }
            }
        }
    }
    cycles.sort();
    cycles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        pairs
            .iter()
            .map(|(n, to)| {
                (
                    (*n).to_string(),
                    to.iter().map(|t| (*t).to_string()).collect(),
                )
            })
            .collect()
    }

    fn pos(order: &[String], n: &str) -> usize {
        order.iter().position(|x| x == n).unwrap()
    }

    /// No kind information: every edge counts as a build-time one, as before.
    fn no_kinds() -> BTreeMap<(String, String), u8> {
        BTreeMap::new()
    }

    /// Edge kinds by `*DEPEND` variable name, as bits of [`crate::DEP_KEYS`].
    fn kinds(pairs: &[((&str, &str), &str)]) -> BTreeMap<(String, String), u8> {
        pairs
            .iter()
            .map(|((from, to), key)| {
                let bit = 1u8 << crate::DEP_KEYS.iter().position(|k| *k == *key).unwrap_or(0);
                (((*from).to_string(), (*to).to_string()), bit)
            })
            .collect()
    }

    /// The `cat/pkg` of the slots `odw0a` (and what it pulls in, one
    /// package per `*DEPEND` variable) would install, in `--sort depend` order.
    fn odw0_order() -> Vec<String> {
        let world = crate::testkit::analyse_fixture_with(&[], &[], |o| {
            o.targets = vec!["dev-libs/odw0a".into()];
        });
        world
            .ordered(&Order::Depend)
            .iter()
            .map(|s| s.cp.clone())
            .collect()
    }

    /// PMS `dependencies.tex`: a `PDEPEND` package is merged "at some point
    /// before the package manager finishes the batch", so it imposes no order.
    #[test]
    fn a_post_dependency_does_not_order() {
        let o = odw0_order();
        let at = |p: &str| o.iter().position(|x| x == p).unwrap();
        assert!(at("dev-libs/odw0a") < at("dev-libs/odw0d"), "{o:?}");
    }

    /// PMS `dependencies.tex`: `DEPEND`, `BDEPEND`, `RDEPEND` and `IDEPEND`
    /// packages come before the package that needs them.
    #[test]
    fn a_run_time_dependency_still_orders() {
        let o = odw0_order();
        let at = |p: &str| o.iter().position(|x| x == p).unwrap();
        for dep in ["odw0b", "odw0c", "odw0e", "odw0f"] {
            assert!(
                at(&format!("dev-libs/{dep}")) < at("dev-libs/odw0a"),
                "{o:?}"
            );
        }
    }

    #[test]
    fn a_dependency_comes_before_its_user() {
        let g = graph(&[("a/app:0", &["b/lib:0"]), ("b/lib:0", &["c/base:0"])]);
        let o = depend_order(&g, &no_kinds());
        assert_eq!(o.len(), 3);
        assert!(pos(&o, "c/base:0") < pos(&o, "b/lib:0"));
        assert!(pos(&o, "b/lib:0") < pos(&o, "a/app:0"));
    }

    /// Charter property 2a (backlog #749): a pure build-time cycle has no
    /// cuttable edge, so no cut is chosen for it — the walk orders it
    /// implicitly (here `y` before `x`, the walk's name order) and
    /// `build_time_cycles` reports the pair, which the checker fails.
    /// Nothing is ever listed twice.
    #[test]
    fn an_all_build_time_cycle_is_ordered_once_and_reported() {
        let g = graph(&[("a/x:0", &["a/y:0"]), ("a/y:0", &["a/x:0"])]);
        let kinds = kinds(&[
            ((("a/x:0"), ("a/y:0")), "BDEPEND"),
            ((("a/y:0"), ("a/x:0")), "BDEPEND"),
        ]);
        let o = depend_order(&g, &kinds);
        assert_eq!(o, ["a/y:0", "a/x:0"]);
        assert_eq!(
            build_time_cycles(&g, &kinds),
            [vec!["a/x:0".to_string(), "a/y:0".to_string()]]
        );
    }

    #[test]
    fn independent_nodes_come_in_name_order() {
        let g = graph(&[("b/two:0", &[]), ("a/one:0", &[]), ("c/three:0", &[])]);
        assert_eq!(
            depend_order(&g, &no_kinds()),
            ["a/one:0", "b/two:0", "c/three:0"]
        );
    }

    /// PMS `dependencies.tex`, `sec:dependency-classes` (a build dependency is
    /// merged before the package needing it) / charter property 2: a cycle is
    /// cut at a runtime-only edge, so the changed provider still comes first.
    #[test]
    fn a_cycle_is_cut_on_a_runtime_edge_first() {
        let prov = "dev-libs/w743prov:0";
        let zeb = "dev-libs/w743zeb:0";
        let virt = "dev-libs/w743virt:0";
        let user = "dev-libs/w743user:0";
        let g = graph(&[
            (prov, &[zeb]),
            (zeb, &[virt]),
            (virt, &[prov]),
            (user, &[virt]),
        ]);
        let kinds = kinds(&[
            ((prov, zeb), "RDEPEND"),
            ((zeb, virt), "RDEPEND"),
            ((virt, prov), "RDEPEND"),
            ((user, virt), "BDEPEND"),
        ]);
        let o = depend_order(&g, &kinds);
        assert!(pos(&o, prov) < pos(&o, virt), "{o:?}");
        assert!(pos(&o, virt) < pos(&o, user), "{o:?}");
    }

    /// Charter property 2 (PMS 8.1: a build tool must be usable, so its
    /// `RDEPEND` closure constrains the order): the cut spares a runtime-only
    /// edge out of a package the plan builds with, and cuts the next
    /// runtime-only edge instead, so the repaired provider still comes first
    /// (backlog #746: `help2man -> Locale-gettext`, not `automake -> help2man`).
    #[test]
    fn a_runtime_edge_that_carries_a_build_time_need_is_not_cut() {
        let auto = "dev-libs/w746auto:0";
        let help = "dev-libs/w746help:0";
        let loc = "dev-libs/w746loc:0";
        let get = "dev-libs/w746get:0";
        let icu = "dev-libs/w746icu:0";
        let g = graph(&[
            (auto, &[help]),
            (help, &[loc]),
            (loc, &[get]),
            (get, &[icu]),
            (icu, &[auto]),
        ]);
        let kinds = kinds(&[
            ((auto, help), "BDEPEND"),
            ((help, loc), "RDEPEND"),
            ((loc, get), "RDEPEND"),
            ((get, icu), "BDEPEND"),
            ((icu, auto), "BDEPEND"),
        ]);
        assert_eq!(
            runtime_cut(&g, &kinds, &build_time_used(&kinds)),
            Some((loc.to_string(), get.to_string()))
        );
        let o = depend_order(&g, &kinds);
        assert!(pos(&o, loc) < pos(&o, auto), "{o:?}");
    }

    /// Charter property 2 (PMS 8.1; backlog #751, plan `02.751` §6-A): a
    /// kept `virtual/*` middle confers no build-time status, so with `prov`
    /// build-time-used the cycle's only cuttable edge is the virtual's own
    /// provider edge — cut as the back edge once the genuine `base <-> prov`
    /// pair is left to the implicit walk (`docs/evidence/02.751/`).
    /// Theoretical, pinned, not proven safe: no live shape exhibits a
    /// consumer building against a broken installed provider.
    #[test]
    fn the_virtual_provider_edge_is_the_only_cuttable_edge() {
        let base = "dev-libs/w751base:0";
        let prov = "dev-libs/w751prov:0";
        let user = "dev-libs/w751user:0";
        let virt = "virtual/w751virt:0";
        let g = graph(&[
            (base, &[prov]),
            (prov, &[base, user]),
            (user, &[virt]),
            (virt, &[prov]),
        ]);
        let kinds = kinds(&[
            ((base, prov), "BDEPEND"),
            ((prov, base), "RDEPEND"),
            ((prov, user), "RDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "RDEPEND"),
        ]);
        let used = build_time_used(&kinds);
        // `base BDEPEND prov` makes `prov` build-time-used; the `BDEPEND`
        // on the virtual confers nothing — the `virtual/*` exclusion.
        assert_eq!(used, BTreeSet::from([prov.to_string()]));
        assert!(!is_cuttable_runtime_edge(
            &kinds,
            &used,
            &(prov.to_string(), user.to_string())
        ));
        assert!(!is_cuttable_runtime_edge(
            &kinds,
            &used,
            &(prov.to_string(), base.to_string())
        ));
        assert!(is_cuttable_runtime_edge(
            &kinds,
            &used,
            &(virt.to_string(), prov.to_string())
        ));
        // The first cycle the walk meets is the genuine `base <-> prov`
        // pair, with no cuttable edge: the cut loop leaves it alone (the
        // walk orders it implicitly and the checker fails it — backlog
        // #749: reported, never a second build) and cuts the remaining
        // cycle at the virtual's provider edge instead — the back edge,
        // and the only cuttable one. It is not a build-time cycle: the
        // `prov -> base` half is `RDEPEND`, so the report reads nothing.
        assert_eq!(
            runtime_cut(&g, &kinds, &used),
            Some((virt.to_string(), prov.to_string()))
        );
        assert!(build_time_cycles(&g, &kinds).is_empty());
        // ... and with that edge gone the remaining cycle cuts at the
        // virtual's provider edge — the back edge, and the only cuttable one.
        let mut rest = g.clone();
        rest.get_mut(prov).unwrap().remove(base);
        assert_eq!(
            runtime_cut(&rest, &kinds, &used),
            Some((virt.to_string(), prov.to_string()))
        );
    }

    /// Charter property 2: a cycle with a cuttable runtime-only edge is cut,
    /// so every node is still listed once (backlog #743).
    #[test]
    fn a_cycle_with_a_cuttable_runtime_edge_lists_each_node_once() {
        let prov = "dev-libs/w743prov:0";
        let zeb = "dev-libs/w743zeb:0";
        let virt = "dev-libs/w743virt:0";
        let user = "dev-libs/w743user:0";
        let g = graph(&[
            (prov, &[zeb]),
            (zeb, &[virt]),
            (virt, &[prov]),
            (user, &[virt]),
        ]);
        let kinds = kinds(&[
            ((prov, zeb), "RDEPEND"),
            ((zeb, virt), "RDEPEND"),
            ((virt, prov), "RDEPEND"),
            ((user, virt), "BDEPEND"),
        ]);
        let o = depend_order(&g, &kinds);
        assert_eq!(o.len(), 4);
        assert!(pos(&o, prov) < pos(&o, virt), "{o:?}");
        assert!(pos(&o, virt) < pos(&o, user), "{o:?}");
    }

    /// Charter property 2 (backlog #745): the middle link — `depend_order`
    /// on the mixed `R/B/R` `w745` graph yields the cut
    /// `[prov, virt, user]`, which the literal checker passes with no
    /// finding (every build atom is satisfied at start).
    #[test]
    fn the_mixed_cycle_cut_is_the_order_the_closure_walks() {
        let prov = "dev-libs/w745prov:0";
        let user = "dev-libs/w745user:0";
        let virt = "dev-libs/w745virt:0";
        let g = graph(&[(prov, &[user]), (user, &[virt]), (virt, &[prov])]);
        let kinds = kinds(&[
            ((prov, user), "RDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "RDEPEND"),
        ]);
        assert_eq!(depend_order(&g, &kinds), [prov, virt, user]);
    }
}
