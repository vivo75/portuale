//! The order a list of slots is shown in.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{DEP_KEYS, SlotSet, World};

/// How [`World::ordered`] sorts the slots.
pub enum Order<'a> {
    /// By `category/package`, then slot. No claim about build order.
    Alpha,
    /// A slot after the slots it needs before it is merged or usable
    /// (`DEPEND`, `BDEPEND`, `RDEPEND`, `IDEPEND`), as the first pass followed
    /// them. A `PDEPEND`-only edge puts no constraint on the order (PMS).
    /// A cycle is cut where the walk meets it.
    Depend,
    /// By the position in an order the caller got elsewhere (the resolver's
    /// merge list), keyed `cat/pkg:slot`; a slot not in it comes last, in
    /// alphabetical order.
    Rank(&'a HashMap<String, usize>),
}

impl World {
    /// The slots `@world` needs that a plan would change, in `how` order.
    #[must_use]
    pub fn ordered(&self, how: &Order<'_>) -> Vec<&SlotSet> {
        let mut changed: Vec<&SlotSet> = self
            .slots
            .iter()
            .filter(|s| self.reached.contains(&s.node()) && s.change().is_some())
            .collect();
        changed.sort_by_key(|s| s.node());
        match how {
            Order::Alpha => {}
            Order::Rank(rank) => {
                changed.sort_by_key(|s| rank.get(&s.node()).copied().unwrap_or(usize::MAX));
            }
            Order::Depend => {
                let position: HashMap<String, usize> = depend_order(&self.ordering_edges())
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
/// every run; an edge back into a node being walked is ignored.
pub(crate) fn depend_order(edges: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
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
        let o = depend_order(&g);
        assert_eq!(o.len(), 3);
        assert!(pos(&o, "c/base:0") < pos(&o, "b/lib:0"));
        assert!(pos(&o, "b/lib:0") < pos(&o, "a/app:0"));
    }

    #[test]
    fn a_cycle_is_cut_and_every_node_is_listed_once() {
        let g = graph(&[("a/x:0", &["a/y:0"]), ("a/y:0", &["a/x:0"])]);
        let o = depend_order(&g);
        assert_eq!(o, ["a/y:0", "a/x:0"]);
    }

    #[test]
    fn independent_nodes_come_in_name_order() {
        let g = graph(&[("b/two:0", &[]), ("a/one:0", &[]), ("c/three:0", &[])]);
        assert_eq!(depend_order(&g), ["a/one:0", "b/two:0", "c/three:0"]);
    }
}
