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
    /// A cycle is cut at a runtime-only edge when it has one, else where the
    /// walk meets it.
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
    #[must_use]
    pub fn ordered(&self, how: &Order<'_>) -> Vec<&SlotSet> {
        let mut changed: Vec<&SlotSet> = self
            .slots
            .iter()
            .filter(|s| self.reached.contains(&s.node()) && self.change(s).is_some())
            .collect();
        changed.sort_by_key(|s| s.node());
        match how {
            Order::Alpha => {}
            Order::Rank(rank) => {
                changed.sort_by_key(|s| rank.get(&s.node()).copied().unwrap_or(usize::MAX));
            }
            Order::Depend => {
                let position: HashMap<String, usize> =
                    depend_order(&self.ordering_edges(), &self.edge_kinds)
                        .into_iter()
                        .enumerate()
                        .map(|(i, n)| (n, i))
                        .collect();
                changed.sort_by_key(|s| position.get(&s.node()).copied().unwrap_or(usize::MAX));
            }
        }
        changed
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
/// every run. A cycle is cut at a runtime-only edge when it has one (a
/// runtime dependency only needs its target merged by the end, so cutting it
/// keeps every build-time edge); otherwise the edge back into the node being
/// walked is ignored. `kinds` carries the `*DEPEND` variables behind each
/// edge, as bits of [`crate::DEP_KEYS`]; an edge without one counts as
/// build-time.
pub(crate) fn depend_order(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
) -> Vec<String> {
    // Cutting one runtime edge can open the next cycle: repeat until every
    // remaining cycle is all build-time, then order what is left.
    let mut working = edges.clone();
    while let Some((from, to)) = runtime_cut(&working, kinds) {
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

/// The first runtime-only edge of the first cycle the walk meets, unless it
/// is the closing back edge itself (cutting that edge is right and needs no
/// restart). The walk is in name order, so the edge is the same on every run.
/// `None` when the cycle has no runtime-only edge at all.
fn runtime_cut(
    edges: &BTreeMap<String, BTreeSet<String>>,
    kinds: &BTreeMap<(String, String), u8>,
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
                    if let Some(cut) = cycle_cut(&stack, kid, kinds) {
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

/// The runtime-only edge to cut in the cycle the walk just closed: the stack
/// from `kid` (the ancestor the back edge points at) to the current node,
/// closed by the back edge last. The first runtime-only edge in that order;
/// `None` when the back edge is the only runtime-only one (cutting it is
/// right) or the cycle has none at all.
fn cycle_cut(
    stack: &[(&String, Vec<&String>, usize)],
    kid: &String,
    kinds: &BTreeMap<(String, String), u8>,
) -> Option<(String, String)> {
    let start = stack.iter().position(|(n, _, _)| *n == kid)?;
    let mut cycle: Vec<(String, String)> = stack[start..]
        .windows(2)
        .map(|w| (w[0].0.clone(), w[1].0.clone()))
        .collect();
    cycle.push((stack.last()?.0.clone(), kid.clone()));
    let last = cycle.len() - 1;
    cycle
        .into_iter()
        .enumerate()
        .find(|(_, edge)| is_runtime_edge(kinds, edge))
        .and_then(|(i, edge)| (i != last).then_some(edge))
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

    #[test]
    fn a_cycle_is_cut_and_every_node_is_listed_once() {
        let g = graph(&[("a/x:0", &["a/y:0"]), ("a/y:0", &["a/x:0"])]);
        let o = depend_order(&g, &no_kinds());
        assert_eq!(o, ["a/y:0", "a/x:0"]);
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
}
