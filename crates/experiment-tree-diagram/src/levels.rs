//! Build levels: a package sits one row above the highest of the packages
//! it must wait for, so everything in one row can be built at the same time.

use std::collections::{BTreeMap, BTreeSet};

/// Bits of `portage_world::DEP_KEYS`.
pub const DEPEND: u8 = 1;
pub const RDEPEND: u8 = 1 << 1;
#[cfg(test)]
pub const PDEPEND: u8 = 1 << 2;
pub const BDEPEND: u8 = 1 << 3;
pub const IDEPEND: u8 = 1 << 4;

/// Needed before the build starts: these win when a cycle must be cut.
const BUILD: u8 = DEPEND | BDEPEND;
/// Needed before the package is usable. `PDEPEND` is in neither group: it
/// never makes a package wait.
const RUN: u8 = RDEPEND | IDEPEND;

/// `(package, package it depends on)` -> dependency kinds.
pub type Edges = BTreeMap<(String, String), u8>;

/// Where every package goes and which edges had no say in it.
pub struct Levels {
    /// Level 0 has no dependency to wait for and is built first.
    pub level: BTreeMap<String, usize>,
    /// Edges that order nothing: `PDEPEND`-only ones and those that would
    /// close a cycle.
    pub loose: BTreeSet<(String, String)>,
}

/// Levels of `nodes` under `edges`. Build-time edges are placed first, so
/// a cycle is cut at a run-time edge whenever it has one.
pub fn levels(nodes: &BTreeSet<String>, edges: &Edges) -> Levels {
    let mut kept: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut loose = BTreeSet::new();
    let build = edges.iter().filter(|(_, k)| **k & BUILD != 0);
    let run = edges.iter().filter(|(_, k)| **k & BUILD == 0);
    for ((from, to), kinds) in build.chain(run) {
        if kinds & (BUILD | RUN) == 0 || from == to || reaches(&kept, to, from) {
            loose.insert((from.clone(), to.clone()));
        } else {
            kept.entry(from).or_default().push(to);
        }
    }
    let mut level = BTreeMap::new();
    for node in nodes {
        height(&kept, node, &mut level);
    }
    Levels { level, loose }
}

/// Drops every ordering edge a longer path already implies (`a -> c` when
/// `a -> b -> c` exists), whatever its kinds: the order is the same without
/// it. Loose edges neither count as a path nor are dropped.
pub fn reduce(edges: &mut Edges, loose: &BTreeSet<(String, String)>) {
    let mut below: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges.keys().filter(|e| !loose.contains(*e)) {
        below.entry(from).or_default().push(to);
    }
    let implied: Vec<(String, String)> = below
        .iter()
        .flat_map(|(from, tos)| tos.iter().map(move |to| (*from, *to)))
        .filter(|(from, to)| {
            below[from]
                .iter()
                .any(|via| via != to && reaches(&below, via, to))
        })
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect();
    for edge in implied {
        edges.remove(&edge);
    }
}

/// Whether `to` can be reached from `from` along `kept`.
fn reaches(kept: &BTreeMap<&str, Vec<&str>>, from: &str, to: &str) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut todo = vec![from];
    while let Some(node) = todo.pop() {
        if node == to {
            return true;
        }
        for next in kept.get(node).into_iter().flatten() {
            if seen.insert(next) {
                todo.push(next);
            }
        }
    }
    false
}

/// Longest way down from `node`; `kept` has no cycle.
fn height(
    kept: &BTreeMap<&str, Vec<&str>>,
    node: &str,
    level: &mut BTreeMap<String, usize>,
) -> usize {
    if let Some(known) = level.get(node) {
        return *known;
    }
    let h = kept
        .get(node)
        .into_iter()
        .flatten()
        .map(|dep| height(kept, dep, level) + 1)
        .max()
        .unwrap_or(0);
    level.insert(node.to_string(), h);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(edges: &[(&str, &str, u8)]) -> Levels {
        let edges: Edges = edges
            .iter()
            .map(|(a, b, k)| ((a.to_string(), b.to_string()), *k))
            .collect();
        let nodes = edges
            .keys()
            .flat_map(|(a, b)| [a.clone(), b.clone()])
            .collect();
        levels(&nodes, &edges)
    }

    #[test]
    fn a_package_sits_above_everything_it_waits_for() {
        let l = run(&[("a", "b", DEPEND), ("b", "c", RDEPEND), ("a", "c", BDEPEND)]);
        assert_eq!((l.level["a"], l.level["b"], l.level["c"]), (2, 1, 0));
        assert!(l.loose.is_empty());
    }

    #[test]
    fn independent_packages_share_a_level() {
        let l = run(&[("a", "b", DEPEND), ("a", "c", DEPEND), ("d", "c", IDEPEND)]);
        assert_eq!(l.level["b"], l.level["c"]);
        assert_eq!(l.level["a"], l.level["d"]);
    }

    fn edge(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn reduce_drops_an_edge_a_longer_path_implies_whatever_its_kind() {
        let mut edges: Edges = [
            (edge("a", "b"), DEPEND),
            (edge("b", "c"), RDEPEND),
            (edge("c", "d"), DEPEND),
            (edge("a", "c"), BDEPEND),
            (edge("a", "d"), DEPEND),
        ]
        .into();
        reduce(&mut edges, &BTreeSet::new());
        let left: Vec<_> = edges.keys().cloned().collect();
        assert_eq!(left, [edge("a", "b"), edge("b", "c"), edge("c", "d")]);
    }

    #[test]
    fn reduce_leaves_loose_edges_alone_and_does_not_walk_them() {
        let mut edges: Edges = [
            (edge("a", "b"), DEPEND),
            (edge("b", "c"), PDEPEND),
            (edge("a", "c"), DEPEND),
        ]
        .into();
        let loose = BTreeSet::from([edge("b", "c")]);
        reduce(&mut edges, &loose);
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn pdepend_orders_nothing() {
        let l = run(&[("a", "b", PDEPEND)]);
        assert_eq!((l.level["a"], l.level["b"]), (0, 0));
        assert!(l.loose.contains(&("a".into(), "b".into())));
    }

    #[test]
    fn a_cycle_is_cut_at_its_run_time_edge() {
        let l = run(&[("a", "b", RDEPEND), ("b", "a", DEPEND)]);
        assert_eq!((l.level["b"], l.level["a"]), (1, 0));
        assert_eq!(
            l.loose,
            BTreeSet::from([("a".to_string(), "b".to_string())])
        );
    }
}
