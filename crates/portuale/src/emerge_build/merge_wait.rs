//! Merge-wait barriers and cycle-aware build readiness (`--jobs`).
//!
//! Portage 3.0.83 holds some merges until no build job runs
//! (`_merge_wait_queue`): the `@system` set plus its transitive runtime
//! dependencies by default (`deep`), selectable via `--merge-wait-scope`
//! (`system`, `toolchain`, `none`). Inside a dependency cycle, post-merge
//! (`PDEPEND`) edges do not gate a build, matching what the merge order
//! already assumes. Small pure helpers live here; the dispatch loop in
//! [`super::scheduler`] owns the queue.

use super::{entry_match_string, entry_matches_any};
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
/// The merge-wait indices: the resolved `@system` set (`system_packages`)
/// plus, under `deep`, its transitive runtime dependencies. Empty without a
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
                let index = RuntimeIndex::build(entries);
                let mut stack: Vec<usize> = set.iter().copied().collect();
                while let Some(i) = stack.pop() {
                    for d in runtime_children(&index, entries, i) {
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

/// Same-cp membership, each entry's resolved runtime targets and the
/// entries naming each cp in `required_by`, built once per
/// `merge_wait_set` call so the deep walk never rescans the list.
// Portage: _find_deep_system_runtime_deps.py, child_nodes per package node
struct RuntimeIndex<'a> {
    members: HashMap<(&'a str, &'a str), Vec<usize>>,
    forward: Vec<Vec<usize>>,
    required_by: HashMap<(&'a str, &'a str), Vec<usize>>,
}

/// Whether `atom` selects entry `j`: undecidable (no candidate, unparsable
/// atom) keeps the edge, like the merge-order digraph.
fn edge_matches(candidate: &[Option<String>], atom: &str, j: usize) -> bool {
    let Some(cand) = candidate[j].as_deref() else {
        return true;
    };
    match portage_dep::match_from_list(atom, &[cand]) {
        Some(m) => !m.is_empty(),
        None => true,
    }
}

impl<'a> RuntimeIndex<'a> {
    fn build(entries: &'a [GraphEntry]) -> Self {
        let mut members: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            members
                .entry((e.category.as_str(), e.package.as_str()))
                .or_default()
                .push(i);
        }
        let candidate: Vec<Option<String>> = entries.iter().map(entry_match_string).collect();
        let mut forward: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
        for (i, e) in entries.iter().enumerate() {
            for dep in &e.deps {
                if !(dep.priority.runtime || dep.priority.runtime_post) {
                    continue;
                }
                let Some(peers) = members.get(&(dep.category.as_str(), dep.package.as_str()))
                else {
                    continue;
                };
                for &j in peers {
                    if j != i && edge_matches(&candidate, &dep.atom, j) && !forward[i].contains(&j)
                    {
                        forward[i].push(j);
                    }
                }
            }
        }
        let mut required_by: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
        for (d, e) in entries.iter().enumerate() {
            for (c, p) in &e.required_by {
                required_by
                    .entry((c.as_str(), p.as_str()))
                    .or_default()
                    .push(d);
            }
        }
        Self {
            members,
            forward,
            required_by,
        }
    }
}

/// Indices `i` depends on through a runtime edge (`RDEPEND`/`IDEPEND` or
/// `PDEPEND`); a `required_by`-only edge with no `deps` record counts as
/// runtime, like the merge-order digraph's synthesized fallback.
// Portage: _find_deep_system_runtime_deps.py, ignore_priority
fn runtime_children(index: &RuntimeIndex, entries: &[GraphEntry], i: usize) -> Vec<usize> {
    let mut out = index.forward[i].clone();
    // Fallback: `i` depends on `d` exactly when `i`'s cp sits in `d`'s
    // `required_by` (the same inversion the dispatch loop builds `deps`
    // from), and `i`'s own `deps` hold no record of it.
    let cp = (entries[i].category.as_str(), entries[i].package.as_str());
    for &d_idx in index.required_by.get(&cp).map_or(&[][..], Vec::as_slice) {
        if d_idx == i || out.contains(&d_idx) {
            continue;
        }
        let d = &entries[d_idx];
        if entries[i]
            .deps
            .iter()
            .any(|e| e.category == d.category && e.package == d.package)
        {
            continue;
        }
        // Entry-keyed: a same-cp sibling reaching `d` through its own forward
        // edge owns this `required_by` link, so `i` must not claim it too.
        let sibling_reaches = index.members.get(&cp).is_some_and(|peers| {
            peers
                .iter()
                .any(|&p| p != i && index.forward[p].contains(&d_idx))
        });
        if sibling_reaches {
            continue;
        }
        out.push(d_idx);
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
