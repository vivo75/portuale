//! Build-order edges that run through installed packages.
//!
//! `kate` links `libQt6TextToSpeech` through the installed `ktexteditor`, which
//! is not in the plan; `qtspeech` is. Nothing in the plan names the pair, so a
//! parallel scheduler may build `kate` while the old `qtspeech` still sits on
//! disk. Walking an installed package's runtime dependencies finds the pair.
//! Portage: depgraph.py, _add_pkg_deps (installed packages keep their edges in
//! the complete graph) and Scheduler.py, _dependent_on_scheduled_merges.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use portage_repo::GraphEntry;

type Cp = (String, String);

/// `(dependent, dependency)` plan indices reached by walking the installed
/// runtime dependencies of every package an entry names but the plan does not
/// contain. The walk stops at the first plan entry on each path.
pub(crate) fn installed_bridge_edges(entries: &[GraphEntry], root: &Path) -> Vec<(usize, usize)> {
    // Only entries that will be built are targets: one the plan merely lists as
    // already installed (`kate-lib` for `kate`) is walked through like any
    // installed package.
    let mut in_plan: HashMap<Cp, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        if !super::scheduler_needs_build(e) {
            continue;
        }
        in_plan
            .entry((e.category.clone(), e.package.clone()))
            .or_default()
            .push(i);
    }
    let mut out = Vec::new();
    // One installed package's own runtime deps never change within a pass.
    let mut memo: HashMap<Cp, Vec<Cp>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        // An entry with nothing to build counts as merged from the start: an edge
        // from it gates nothing and could only crowd out a real one.
        if !super::scheduler_needs_build(e) {
            continue;
        }
        let mut seen: HashSet<Cp> = HashSet::new();
        let mut stack: Vec<Cp> = e
            .deps
            .iter()
            .map(|d| (d.category.clone(), d.package.clone()))
            .filter(|cp| !in_plan.contains_key(cp))
            .collect();
        let mut found: HashSet<usize> = HashSet::new();
        while let Some(cp) = stack.pop() {
            if !seen.insert(cp.clone()) {
                continue;
            }
            let children = memo
                .entry(cp.clone())
                .or_insert_with(|| portage_repo::installed_runtime_dep_cps(root, &cp.0, &cp.1));
            for child in children.iter() {
                match in_plan.get(child) {
                    Some(idxs) => found.extend(idxs.iter().copied().filter(|&j| j != i)),
                    None => stack.push(child.clone()),
                }
            }
        }
        let mut found: Vec<usize> = found.into_iter().collect();
        found.sort_unstable();
        out.extend(found.into_iter().map(|j| (i, j)));
    }
    out
}

/// Whether `from` reaches `to` through `deps`.
pub(crate) fn reaches(deps: &[HashSet<usize>], from: usize, to: usize) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(n) = stack.pop() {
        if n == to {
            return true;
        }
        if seen.insert(n) {
            stack.extend(deps[n].iter().copied());
        }
    }
    false
}
