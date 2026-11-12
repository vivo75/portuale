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

/// Which build step each package falls in when `jobs` packages can be built
/// at once: step 1 is the first, and packages sharing a step are built at the
/// same time. Every package is taken to need one step, so a step is a round,
/// not a duration. A package starts once everything it waits for has
/// finished in an earlier step; when more than `jobs` are ready, the ones
/// with the longest chain of dependents above them go first (then by name),
/// so the choice is the same on every run.
pub fn schedule(
    nodes: &BTreeSet<String>,
    edges: &Edges,
    loose: &BTreeSet<(String, String)>,
    jobs: usize,
) -> BTreeMap<String, usize> {
    let mut waits_for: BTreeMap<&str, usize> = nodes.iter().map(|n| (n.as_str(), 0)).collect();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges.keys().filter(|e| !loose.contains(*e)) {
        if let (Some(count), true) = (waits_for.get_mut(from.as_str()), nodes.contains(to)) {
            *count += 1;
            dependents.entry(to).or_default().push(from);
        }
    }
    let mut above: BTreeMap<&str, usize> = BTreeMap::new();
    for node in nodes {
        chain_above(&dependents, node, &mut above);
    }
    let mut step_of = BTreeMap::new();
    let mut ready: Vec<&str> = waits_for
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut step = 1;
    while !ready.is_empty() {
        ready.sort_by(|a, b| above[b].cmp(&above[a]).then(a.cmp(b)));
        let now: Vec<&str> = ready.drain(..jobs.min(ready.len())).collect();
        for done in &now {
            step_of.insert((*done).to_string(), step);
        }
        for done in now {
            for parent in dependents.get(done).into_iter().flatten() {
                let count = waits_for.get_mut(parent).expect("every node is counted");
                *count -= 1;
                if *count == 0 {
                    ready.push(parent);
                }
            }
        }
        step += 1;
    }
    step_of
}

/// Longest chain of dependents above `node`; `dependents` has no cycle.
fn chain_above<'a>(
    dependents: &BTreeMap<&'a str, Vec<&'a str>>,
    node: &'a str,
    memo: &mut BTreeMap<&'a str, usize>,
) -> usize {
    if let Some(known) = memo.get(node) {
        return *known;
    }
    let n = dependents
        .get(node)
        .into_iter()
        .flatten()
        .map(|up| chain_above(dependents, up, memo) + 1)
        .max()
        .unwrap_or(0);
    memo.insert(node, n);
    n
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

    fn steps(
        edges: &[(&str, &str, u8)],
        loose: &[(&str, &str)],
        jobs: usize,
    ) -> Vec<(String, usize)> {
        let edges: Edges = edges.iter().map(|(a, b, k)| (edge(a, b), *k)).collect();
        let nodes = edges
            .keys()
            .flat_map(|(a, b)| [a.clone(), b.clone()])
            .collect();
        let loose = loose.iter().map(|(a, b)| edge(a, b)).collect();
        schedule(&nodes, &edges, &loose, jobs).into_iter().collect()
    }

    #[test]
    fn one_job_numbers_every_package_differently_dependencies_first() {
        // t waits for x and y.
        let got = steps(&[("t", "x", DEPEND), ("t", "y", DEPEND)], &[], 1);
        assert_eq!(got, [("t".into(), 3), ("x".into(), 1), ("y".into(), 2)]);
    }

    #[test]
    fn two_jobs_build_independent_packages_in_the_same_step() {
        let got = steps(&[("t", "x", DEPEND), ("t", "y", DEPEND)], &[], 2);
        assert_eq!(got, [("t".into(), 2), ("x".into(), 1), ("y".into(), 1)]);
    }

    #[test]
    fn a_chain_ignores_the_job_count_and_the_longest_chain_goes_first() {
        // a -> b -> c is a chain of three; z is alone.
        let edges = [("a", "b", DEPEND), ("b", "c", DEPEND), ("a", "z", DEPEND)];
        let got = steps(&edges, &[], 1);
        // c has two packages above it, z one: c first.
        assert_eq!(
            got,
            [
                ("a".into(), 4),
                ("b".into(), 2),
                ("c".into(), 1),
                ("z".into(), 3)
            ]
        );
        let many = steps(&edges, &[], 8);
        assert_eq!(
            many,
            [
                ("a".into(), 3),
                ("b".into(), 2),
                ("c".into(), 1),
                ("z".into(), 1)
            ]
        );
    }

    #[test]
    fn a_package_left_out_of_the_numbering_does_not_make_others_wait() {
        // a -> b -> c, with c already installed: b can start in step 1.
        let edges: Edges = [(edge("a", "b"), DEPEND), (edge("b", "c"), DEPEND)].into();
        let building = BTreeSet::from(["a".to_string(), "b".to_string()]);
        let got = schedule(&building, &edges, &BTreeSet::new(), 1);
        assert_eq!(got, BTreeMap::from([("a".into(), 2), ("b".into(), 1)]));
    }

    #[test]
    fn a_loose_edge_does_not_make_a_package_wait() {
        let got = steps(&[("a", "b", PDEPEND)], &[("a", "b")], 1);
        assert_eq!(got, [("a".into(), 1), ("b".into(), 2)]);
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
