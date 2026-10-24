//! Merge-wait barriers and cycle-aware build readiness (`--jobs`).
//!
//! Portage 3.0.83 holds some merges until no build job runs
//! (`_merge_wait_queue`): the `@system` set plus its transitive runtime
//! dependencies by default (`deep`), selectable via `--merge-wait-scope`
//! (`system`, `toolchain`, `none`). Inside a dependency cycle, post-merge
//! (`PDEPEND`) edges do not gate a build, matching what the merge order
//! already assumes. Small pure helpers live here; the dispatch loop in
//! [`super::scheduler`] owns the queue.

use super::entry_matches_any;
use crate::pretend::MergeWaitScope;
use portage_repo::GraphEntry;
use std::collections::{HashMap, HashSet};

/// Packages whose ebuild environment runs under essentially every build, so
/// a half-merged one can break an unrelated concurrent build. Fixed list,
/// not derived from the graph: nothing in the metadata says it.
// Portage: _emerge/_find_deep_system_runtime_deps.py, CORE_TOOLCHAIN
pub(crate) const CORE_TOOLCHAIN: &[(&str, &str)] = &[
    ("app-shells", "bash"),
    ("dev-lang", "perl"),
    ("dev-lang", "python"),
    ("dev-libs", "libffi"),
    ("sys-apps", "baselayout"),
    ("sys-apps", "coreutils"),
    ("sys-apps", "sandbox"),
    ("sys-devel", "binutils"),
    ("sys-devel", "gcc"),
    ("sys-devel", "gettext"),
    ("sys-libs", "glibc"),
    ("sys-libs", "musl"),
];

/// Whether the resolved `FEATURES` list carries `merge-wait` (on by
/// default): then every merge barriers, whatever the scope selects.
// Portage: _emerge/Scheduler.py, _build_exit
pub(crate) fn features_hold_all_merges(features: &str) -> bool {
    features.split_whitespace().any(|t| t == "merge-wait")
}
/// is the resolved `@system` set (`system_packages`); empty without a
/// resolved config, so every scope but `toolchain` then selects nothing.
// Portage: _emerge/_find_deep_system_runtime_deps.py
pub(crate) fn merge_wait_set(
    entries: &[GraphEntry],
    system_atoms: &[String],
    scope: MergeWaitScope,
) -> HashSet<usize> {
    match scope {
        MergeWaitScope::None => HashSet::new(),
        MergeWaitScope::Toolchain => entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                CORE_TOOLCHAIN
                    .iter()
                    .any(|(c, p)| e.category == *c && e.package == *p)
            })
            .map(|(i, _)| i)
            .collect(),
        MergeWaitScope::System | MergeWaitScope::Deep => {
            let mut set: HashSet<usize> = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| entry_matches_any(e, system_atoms))
                .map(|(i, _)| i)
                .collect();
            if scope == MergeWaitScope::Deep {
                let cp_to_idx: HashMap<(String, String), usize> = entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| ((e.category.clone(), e.package.clone()), i))
                    .collect();
                let mut stack: Vec<usize> = set.iter().copied().collect();
                while let Some(i) = stack.pop() {
                    for d in runtime_children(entries, &cp_to_idx, i) {
                        if set.insert(d) {
                            stack.push(d);
                        }
                    }
                }
            }
            set
        }
    }
}

/// Indices `i` depends on through a runtime edge (`RDEPEND`/`IDEPEND` or
/// `PDEPEND`); a `required_by`-only edge with no `deps` record counts as
/// runtime, like the merge-order digraph's synthesized fallback.
// Portage: _find_deep_system_runtime_deps.py, ignore_priority
fn runtime_children(
    entries: &[GraphEntry],
    cp_to_idx: &HashMap<(String, String), usize>,
    i: usize,
) -> Vec<usize> {
    let mut out = Vec::new();
    for dep in &entries[i].deps {
        let Some(&d) = cp_to_idx.get(&(dep.category.clone(), dep.package.clone())) else {
            continue;
        };
        if d == i {
            continue;
        }
        if dep.priority.runtime || dep.priority.runtime_post {
            out.push(d);
        }
    }
    // Fallback: `i` depends on `d` exactly when `i`'s cp sits in `d`'s
    // `required_by` (the same inversion the dispatch loop builds `deps`
    // from), and `i`'s own `deps` hold no record of it.
    let i_cp = (entries[i].category.clone(), entries[i].package.clone());
    for (d_idx, d) in entries.iter().enumerate() {
        if d_idx == i || out.contains(&d_idx) {
            continue;
        }
        if cp_to_idx.get(&(d.category.clone(), d.package.clone())) != Some(&d_idx) {
            continue;
        }
        if d.required_by.contains(&i_cp)
            && !entries[i].deps.iter().any(|e| {
                (e.category.clone(), e.package.clone()) == (d.category.clone(), d.package.clone())
            })
        {
            out.push(d_idx);
        }
    }
    out
}

/// Nodes on a dependency cycle: component of more than one node over the
/// blocking edges. Portage drops `optional` edges first (`ignore_soft`);
/// portuale keeps them, which agrees on source-merge graphs (that flag
/// marks build-time keys of already-built packages, and nothing here is
/// built yet). A self-edge alone is not a cycle, like Portage's `len > 1`
/// test.
// Portage: _emerge/Scheduler.py, _find_cyclic_nodes (3d677b7b2)
pub(crate) fn cyclic_nodes(adj: &[Vec<usize>]) -> HashSet<usize> {
    let comp = portage_repo::strongly_connected_components(adj);
    let mut size: HashMap<usize, usize> = HashMap::new();
    for &c in &comp {
        *size.entry(c).or_insert(0) += 1;
    }
    comp.iter()
        .enumerate()
        .filter(|&(_, &c)| size.get(&c).is_some_and(|&n| n > 1))
        .map(|(i, _)| i)
        .collect()
}

/// Whether the `owner -> dep` edge still blocks the owner's build once the
/// owner is known cyclic: post-merge-only (`PDEPEND`) edges do not, since
/// the merge order already ignores them to break the cycle.
// Portage: _emerge/DepPrioritySatisfiedRange.py, ignore_medium_post
fn post_edge_blocks(owner: &GraphEntry, dep_cp: &(String, String)) -> bool {
    let mut known = false;
    let mut blocked = false;
    for e in &owner.deps {
        if (e.category.clone(), e.package.clone()) != *dep_cp {
            continue;
        }
        known = true;
        let p = &e.priority;
        // Not droppable: `optional` aside, anything with a buildtime or
        // runtime side still blocks; only pure post-merge edges go.
        if p.optional || ((p.runtime_post) && !p.buildtime && !p.runtime) {
            continue;
        }
        blocked = true;
    }
    // No `deps` record (a `required_by` fallback) is a runtime edge: blocks.
    !known || blocked
}

/// Per-entry blocking sets: `deps` minus the post-merge edges of cyclic
/// owners. Everything else dispatches exactly as before, so acyclic graphs
/// (and the `deep` default over them) behave byte-identically.
pub(crate) fn blocking_deps(
    entries: &[GraphEntry],
    deps: &[HashSet<usize>],
    cyclic: &HashSet<usize>,
) -> Vec<HashSet<usize>> {
    deps.iter()
        .enumerate()
        .map(|(i, ds)| {
            if !cyclic.contains(&i) {
                return ds.clone();
            }
            let mut blocking = HashSet::new();
            for &d in ds {
                let dep_cp = (entries[d].category.clone(), entries[d].package.clone());
                if post_edge_blocks(&entries[i], &dep_cp) {
                    blocking.insert(d);
                }
            }
            blocking
        })
        .collect()
}
