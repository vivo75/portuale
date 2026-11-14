//! The order a list of slots is shown in.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{Change, DEP_KEYS, SlotSet, World};

/// How [`World::ordered`] sorts the slots.
pub enum Order<'a> {
    /// By `category/package`, then slot. No claim about build order.
    Alpha,
    /// A slot after the slots it needs before it is merged or usable
    /// (`DEPEND`, `BDEPEND`, `RDEPEND`, `IDEPEND`), as the first pass followed
    /// them. A `PDEPEND`-only edge puts no constraint on the order (PMS).
    /// A cycle is cut at a cuttable runtime-only edge when it has one (a
    /// runtime edge out of a package the plan builds with still orders, PMS
    /// 8.1); a cycle with none is broken by building its consumer twice
    /// (charter 2a).
    Depend,
    /// By the position in an order the caller got elsewhere (the resolver's
    /// merge list), keyed `cat/pkg:slot`; a slot not in it comes last, in
    /// alphabetical order.
    Rank(&'a HashMap<String, usize>),
}

/// One row of an [`Order::Depend`] plan: the slot, and whether it is the
/// slot's first build or the charter-2a rebuild that breaks a cycle.
pub struct Row<'a> {
    /// The slot this row merges.
    pub slot: &'a SlotSet,
    /// [`Pass::First`] for every row but the second build of a breaker.
    pub pass: Pass,
}

/// Which build of the slot a [`Row`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pass {
    /// The slot's only, or first, build.
    First,
    /// The charter-2a second build that breaks an all-build-time cycle.
    Rebuild,
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
    /// Under [`Order::Depend`] a breaker slot comes twice, first as
    /// [`Pass::First`] and again as [`Pass::Rebuild`].
    #[must_use]
    pub fn ordered(&self, how: &Order<'_>) -> Vec<Row<'_>> {
        let mut changed: Vec<&SlotSet> = self
            .slots
            .iter()
            .filter(|s| self.reached.contains(&s.node()) && self.change(s).is_some())
            .collect();
        changed.sort_by_key(|s| s.node());
        match how {
            Order::Alpha => changed
                .into_iter()
                .map(|slot| Row {
                    slot,
                    pass: Pass::First,
                })
                .collect(),
            Order::Rank(rank) => {
                changed.sort_by_key(|s| rank.get(&s.node()).copied().unwrap_or(usize::MAX));
                changed
                    .into_iter()
                    .map(|slot| Row {
                        slot,
                        pass: Pass::First,
                    })
                    .collect()
            }
            Order::Depend => {
                let by_node: HashMap<String, &SlotSet> =
                    changed.iter().map(|s| (s.node(), *s)).collect();
                let installed: BTreeSet<String> = self
                    .slots
                    .iter()
                    .filter(|s| s.installed.is_some())
                    .map(|s| s.node())
                    .collect();
                let rows: BTreeSet<String> = changed.iter().map(|s| s.node()).collect();
                let edges = self.ordering_edges();
                let order = depend_order(&edges, &self.edge_kinds);
                // Charter 2a, second instrument (backlog #745): duplicate
                // the first package whose build-time need chain the cut
                // order leaves unmet, after its closure becomes satisfied.
                let order =
                    close_usability_gaps(&edges, &self.edge_kinds, order, &installed, &rows);
                let mut seen: HashMap<String, usize> = HashMap::new();
                order
                    .into_iter()
                    .filter_map(|name| by_node.get(&name).copied())
                    .map(|slot| {
                        let count = seen.entry(slot.node()).or_insert(0);
                        *count += 1;
                        Row {
                            slot,
                            pass: if *count == 1 {
                                Pass::First
                            } else {
                                Pass::Rebuild
                            },
                        }
                    })
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
/// not cuttable); a cycle with no cuttable edge is broken by building its
/// consumer twice, right after the cycle's last member. `kinds` carries the
/// `*DEPEND` variables behind each edge, as bits of [`crate::DEP_KEYS`]; an
/// edge without one counts as build-time.
pub(crate) fn depend_order(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
) -> Vec<String> {
    let build_used = build_time_used(kinds);
    // Cutting one runtime edge can open the next cycle: repeat until every
    // remaining cycle is all build-time, then order what is left.
    let mut working = edges.clone();
    let mut second: Vec<String> = Vec::new();
    let mut members_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    loop {
        match runtime_cut(&working, kinds, &build_used) {
            None => break,
            Some(CycleCut::Edge(from, to)) => {
                if let Some(next) = working.get_mut(&from) {
                    next.remove(&to);
                }
            }
            // The back edge is the only cuttable one: cut it too, rather
            // than stopping the scan. Stopping would leave every later
            // cycle to the walk below, whose implicit cuts are not chosen
            // by any rule (backlog #746: the perl cluster moved late).
            Some(CycleCut::BackEdge(from, to)) => {
                if let Some(next) = working.get_mut(&from) {
                    next.remove(&to);
                }
            }
            Some(CycleCut::Rebuild(consumer, members)) => {
                let back = members.first().cloned().unwrap_or_default();
                if let Some(next) = working.get_mut(&consumer) {
                    next.remove(&back);
                }
                // Charter 2a builds a package twice, not more: a repeat
                // consumer only widens the member set its second build
                // follows.
                members_of
                    .entry(consumer.clone())
                    .or_default()
                    .extend(members);
                if !second.contains(&consumer) {
                    second.push(consumer);
                }
            }
        }
    }
    let mut out = unordered(&working);
    for consumer in &second {
        // The second build comes after every member of every cycle the
        // consumer breaks, so each of them meets charter 2a's excuse.
        let empty = Vec::new();
        let members = members_of.get(consumer).unwrap_or(&empty);
        if let Some(last) = members
            .iter()
            .filter_map(|m| out.iter().position(|n| n == m))
            .max()
        {
            out.insert(last + 1, consumer.clone());
        }
    }
    out
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
    let mut mask = 0u8;
    for (i, key) in crate::DEP_KEYS.iter().enumerate() {
        if ["DEPEND", "BDEPEND", "IDEPEND"].contains(key) {
            mask |= 1u8 << i;
        }
    }
    mask
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

/// What the first cycle the walk meets needs: cut this runtime edge, cut the
/// closing back edge itself when only it is cuttable, or build the back-edge
/// consumer twice when nothing in the cycle is cuttable.
#[derive(Debug, PartialEq, Eq)]
enum CycleCut {
    /// Cut this edge and look for the next cycle.
    Edge(String, String),
    /// The back edge is the only cuttable one: cut it and keep scanning.
    BackEdge(String, String),
    /// The consumer to build twice, and the members of its cycle.
    Rebuild(String, Vec<String>),
}

/// The cut the first cycle the walk meets needs, if it is cyclic. The walk
/// is in name order, so the cut is the same on every run. `None` when the
/// graph is acyclic.
fn runtime_cut(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
    build_used: &BTreeSet<String>,
) -> Option<CycleCut> {
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
/// edge itself when only it is cuttable; a rebuild of the current node (the
/// consumer of the back edge's need) when nothing is.
fn cycle_cut(
    stack: &[(&String, Vec<&String>, usize)],
    kid: &String,
    kinds: &BTreeMap<(String, String), u8>,
    build_used: &BTreeSet<String>,
) -> Option<CycleCut> {
    let start = stack.iter().position(|(n, _, _)| *n == kid)?;
    let mut cycle: Vec<(String, String)> = stack[start..]
        .windows(2)
        .map(|w| (w[0].0.clone(), w[1].0.clone()))
        .collect();
    cycle.push((stack.last()?.0.clone(), kid.clone()));
    let last = cycle.len() - 1;
    let first = cycle
        .iter()
        .enumerate()
        .find(|(_, edge)| is_cuttable_runtime_edge(kinds, build_used, edge));
    match first {
        None => {
            let members: Vec<String> = stack[start..]
                .iter()
                .map(|(n, _, _)| (*n).clone())
                .collect();
            Some(CycleCut::Rebuild(stack.last()?.0.clone(), members))
        }
        Some((i, _)) if i == last => {
            let back = cycle.last().cloned().expect("the cycle closes");
            Some(CycleCut::BackEdge(back.0, back.1))
        }
        Some((_, edge)) => Some(CycleCut::Edge(edge.0.clone(), edge.1.clone())),
    }
}

/// Shared inputs of the usability walk: the graph, its `*DEPEND` kinds,
/// the installed slots, and the plan's rows.
struct Usability<'a> {
    edges: &'a BTreeMap<String, BTreeSet<String>>,
    kinds: &'a BTreeMap<(String, String), u8>,
    installed: &'a BTreeSet<String>,
    rows: &'a BTreeSet<String>,
}

/// Second builds for packages whose build-time need chain the cut order
/// leaves unmet (charter 2a, backlog #745): walk `order` in merge order
/// and duplicate the first package with an unmet chain, at the first later
/// position where its closure becomes satisfied; repeat until every merge
/// order position holds. A chain that is never satisfiable gets no second
/// build. At most twice per slot. Deterministic: first in merge order wins,
/// edges are walked in name order.
pub(crate) fn close_usability_gaps(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
    order: Vec<String>,
    installed: &BTreeSet<String>,
    rows: &BTreeSet<String>,
) -> Vec<String> {
    let ctx = Usability {
        edges,
        kinds,
        installed,
        rows,
    };
    let mut out = order;
    loop {
        let mut found: Option<(String, usize)> = None;
        for (i, name) in out.iter().enumerate() {
            if out.iter().filter(|n| *n == name).count() >= 2 {
                continue;
            }
            if chain_unmet(name, i, &out, &ctx).is_none() {
                continue;
            }
            if let Some(pos) =
                ((i + 1)..=out.len()).find(|&o| chain_unmet(name, o, &out, &ctx).is_none())
            {
                found = Some((name.clone(), pos));
                break;
            }
        }
        let Some((name, pos)) = found else {
            break;
        };
        out.insert(pos, name);
    }
    out
}

/// `Some` chain (the merge itself first) when a build-time need of the
/// merge of `node` at `pos` is not met there; `None` when its chain holds.
/// Mirrors the checker's usability walk (`_Moment.gap`): a build need must
/// be usable, i.e. every package in its `RDEPEND` closure is installed or
/// merged before `pos`; the merge's own `RDEPEND` needs are satisfiable by
/// the end and do not count. A cycle back to a node on the chain counts as
/// usable. Like the checker, an unmet need counts only when its provider
/// is a plan merge (a kept provider's unmet need was already broken before
/// the plan) and only when the need can be met at all, by an installed
/// package or a row; the rest is not fixable by building twice.
fn chain_unmet(
    node: &str,
    pos: usize,
    order: &[String],
    ctx: &Usability<'_>,
) -> Option<Vec<String>> {
    let mut visiting = vec![node.to_string()];
    for dep in build_deps(ctx.edges, ctx.kinds, node) {
        if !is_available(dep, pos, order, ctx.installed) {
            if !ctx.installed.contains(dep.as_str()) && !ctx.rows.contains(dep) {
                continue;
            }
            let mut members = visiting.clone();
            members.push(dep.clone());
            return Some(members);
        }
        if let Some(members) = usable_unmet(dep, pos, order, ctx, &mut visiting) {
            return Some(members);
        }
    }
    None
}

/// `Some` chain when `provider` is not usable for a merge at `pos`.
/// A need unmet by anything installed or merged is pre-existing breakage
/// unless its provider is itself merged by the plan.
fn usable_unmet(
    provider: &str,
    pos: usize,
    order: &[String],
    ctx: &Usability<'_>,
    visiting: &mut Vec<String>,
) -> Option<Vec<String>> {
    let targets: Vec<&String> = ctx.edges.get(provider).map_or_else(Vec::new, |s| {
        s.iter()
            .filter(|dep| {
                ctx.kinds
                    .get(&(provider.to_string(), (*dep).clone()))
                    .is_some_and(|mask| mask & build_mask() == 0)
            })
            .collect()
    });
    for dep in targets {
        if !is_available(dep, pos, order, ctx.installed) {
            // The plan builds `provider` itself, so an unmet need is the
            // plan's gap to close; a kept provider's unmet need was already
            // broken before the plan and is not ours to build twice for.
            if !ctx.rows.contains(provider) {
                continue;
            }
            return Some(visiting.clone());
        }
        if visiting.contains(dep) {
            continue;
        }
        visiting.push(dep.clone());
        if let Some(members) = usable_unmet(dep, pos, order, ctx, visiting) {
            return Some(members);
        }
        visiting.pop();
    }
    None
}

/// The build-time out-edges of `node`: `DEPEND`, `BDEPEND` or `IDEPEND`
/// (an edge without kind information counts as build-time, as before).
fn build_deps<'a>(
    edges: &'a BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
    node: &str,
) -> Vec<&'a String> {
    edges.get(node).map_or_else(Vec::new, |s| {
        s.iter()
            .filter(|dep| {
                kinds
                    .get(&(node.to_string(), (*dep).clone()))
                    .is_none_or(|mask| mask & build_mask() != 0)
            })
            .collect()
    })
}

/// Whether `node` is installed, or merged before `pos`.
fn is_available(node: &str, pos: usize, order: &[String], installed: &BTreeSet<String>) -> bool {
    installed.contains(node) || order[..pos].iter().any(|n| n == node)
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
            .map(|r| r.slot.cp.clone())
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

    /// Charter property 2a: a pure build-time cycle is broken by building
    /// its consumer twice (changed on purpose: every node was listed once
    /// before, backlog #745).
    #[test]
    fn an_all_build_time_cycle_lists_the_consumer_twice() {
        let g = graph(&[("a/x:0", &["a/y:0"]), ("a/y:0", &["a/x:0"])]);
        let kinds = kinds(&[
            ((("a/x:0"), ("a/y:0")), "BDEPEND"),
            ((("a/y:0"), ("a/x:0")), "BDEPEND"),
        ]);
        let o = depend_order(&g, &kinds);
        assert_eq!(o, ["a/y:0", "a/x:0", "a/y:0"]);
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
            Some(CycleCut::Edge(loc.to_string(), get.to_string()))
        );
        let o = depend_order(&g, &kinds);
        assert!(pos(&o, loc) < pos(&o, auto), "{o:?}");
    }

    /// Charter property 2a: an all-build-time cycle has no cuttable
    /// runtime-only edge, so the back-edge consumer is built twice, the
    /// second build after the cycle's last member (backlog #745).
    #[test]
    fn a_build_time_cycle_is_broken_by_rebuilding_the_consumer_once() {
        let prov = "dev-libs/w745prov:0";
        let user = "dev-libs/w745user:0";
        let virt = "dev-libs/w745virt:0";
        let g = graph(&[(prov, &[user]), (user, &[virt]), (virt, &[prov])]);
        let kinds = kinds(&[
            ((prov, user), "BDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "BDEPEND"),
        ]);
        let o = depend_order(&g, &kinds);
        assert_eq!(o, [virt, user, prov, virt]);
        assert_eq!(depend_order(&g, &kinds), o);
    }

    /// Charter property 2: a cycle with a cuttable runtime-only edge is cut,
    /// not rebuilt, so every node is still listed once (backlog #743).
    #[test]
    fn a_cycle_with_a_cuttable_runtime_edge_needs_no_second_build() {
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

    /// Charter property 2a (backlog #745): the cut leaves an unmet
    /// build-time need chain (`user` needs `virt` usable, `virt` needs
    /// `prov` usable, `prov` needs `user`), so the consumer is built again
    /// after the position where the closure becomes satisfied.
    #[test]
    fn a_usability_gap_in_the_cut_order_gets_a_second_build() {
        let prov = "dev-libs/w745prov:0";
        let user = "dev-libs/w745user:0";
        let virt = "dev-libs/w745virt:0";
        let g = graph(&[(prov, &[user]), (user, &[virt]), (virt, &[prov])]);
        let kinds = kinds(&[
            ((prov, user), "RDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "RDEPEND"),
        ]);
        let cut = vec![prov.to_string(), virt.to_string(), user.to_string()];
        let rows: BTreeSet<String> = cut.iter().cloned().collect();
        let closed = close_usability_gaps(&g, &kinds, cut, &BTreeSet::new(), &rows);
        assert_eq!(closed, [prov, virt, user, user]);
        assert_eq!(
            close_usability_gaps(&g, &kinds, closed.clone(), &BTreeSet::new(), &rows),
            closed
        );
    }

    /// Charter property 2a (backlog #745): with the consumer installed the
    /// closure is already satisfied, so the cut order stands as is.
    #[test]
    fn an_installed_consumer_needs_no_second_build() {
        let prov = "dev-libs/w745prov:0";
        let user = "dev-libs/w745user:0";
        let virt = "dev-libs/w745virt:0";
        let g = graph(&[(prov, &[user]), (user, &[virt]), (virt, &[prov])]);
        let kinds = kinds(&[
            ((prov, user), "RDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "RDEPEND"),
        ]);
        let cut = vec![prov.to_string(), virt.to_string(), user.to_string()];
        let installed = BTreeSet::from([user.to_string()]);
        let rows: BTreeSet<String> = cut.iter().cloned().collect();
        assert_eq!(
            close_usability_gaps(&g, &kinds, cut.clone(), &installed, &rows),
            cut
        );
    }

    /// Charter property 2a (backlog #745): at most twice per slot — a node
    /// the cut already duplicated is not duplicated again.
    #[test]
    fn a_second_build_is_never_a_third_build() {
        let prov = "dev-libs/w745prov:0";
        let user = "dev-libs/w745user:0";
        let virt = "dev-libs/w745virt:0";
        let g = graph(&[(prov, &[user]), (user, &[virt]), (virt, &[prov])]);
        let kinds = kinds(&[
            ((prov, user), "RDEPEND"),
            ((user, virt), "BDEPEND"),
            ((virt, prov), "RDEPEND"),
        ]);
        let cut = vec![
            prov.to_string(),
            virt.to_string(),
            user.to_string(),
            user.to_string(),
        ];
        let rows: BTreeSet<String> = cut.iter().cloned().collect();
        assert_eq!(
            close_usability_gaps(&g, &kinds, cut.clone(), &BTreeSet::new(), &rows),
            cut
        );
    }
}
