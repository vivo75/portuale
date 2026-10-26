use super::*;
use crate::pretend::MergeWaitScope;
use portage_repo::{DepEdge, DepPriority};
use std::collections::HashSet;

fn wait_entry(category: &str, package: &str) -> GraphEntry {
    let mut e = source_entry(
        package,
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    e.category = category.into();
    e.package = package.into();
    e
}

fn edge_to(category: &str, package: &str, key: u8, priority: DepPriority) -> DepEdge {
    DepEdge {
        atom: format!("{category}/{package}"),
        evaluated: format!("{category}/{package}"),
        category: category.into(),
        package: package.into(),
        priority,
        disjunctive: false,
        alt: None,
        key,
    }
}

fn runtime_edge(category: &str, package: &str) -> DepEdge {
    edge_to(
        category,
        package,
        0,
        DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
    )
}

fn post_edge(category: &str, package: &str) -> DepEdge {
    edge_to(
        category,
        package,
        2,
        DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        },
    )
}

fn build_edge(category: &str, package: &str) -> DepEdge {
    edge_to(
        category,
        package,
        3,
        DepPriority {
            buildtime: true,
            ..DepPriority::default()
        },
    )
}

fn cps(entries: &[GraphEntry], set: &HashSet<usize>) -> HashSet<String> {
    set.iter()
        .map(|&i| format!("{}/{}", entries[i].category, entries[i].package))
        .collect()
}

/// Oracle: upstream `test_merge_wait_scope.py` (`testMergeWaitScope`):
/// `none` selects nothing, even over `@system` members.
#[test]
fn none_scope_selects_nothing() {
    let entries = vec![
        wait_entry("sys-libs", "glibc"),
        wait_entry("app-misc", "sysextra"),
    ];
    let set = merge_wait_set(
        &entries,
        &["*sys-libs/glibc".to_string()],
        MergeWaitScope::None,
    );
    assert!(set.is_empty(), "{set:?}");
}

/// Oracle: same upstream test, `toolchain` selects only the fixed core
/// toolchain (`sys-libs/glibc` is in it, `app-misc/sysextra` is not).
#[test]
fn toolchain_scope_selects_only_toolchain_members() {
    let entries = vec![
        wait_entry("sys-libs", "glibc"),
        wait_entry("app-misc", "sysextra"),
        wait_entry("app-shells", "bash"),
    ];
    let set = merge_wait_set(&entries, &[], MergeWaitScope::Toolchain);
    assert_eq!(
        cps(&entries, &set),
        HashSet::from(["sys-libs/glibc".to_string(), "app-shells/bash".to_string()])
    );
}

/// Oracle: same upstream test, `system` selects the `@system` members and
/// nothing reachable from them.
#[test]
fn system_scope_selects_only_system_members() {
    let mut glibc = wait_entry("sys-libs", "glibc");
    glibc.deps = vec![runtime_edge("dev-libs", "runtime-dep")];
    let entries = vec![
        glibc,
        wait_entry("app-misc", "sysextra"),
        wait_entry("dev-libs", "runtime-dep"),
    ];
    let set = merge_wait_set(
        &entries,
        &[
            "*sys-libs/glibc".to_string(),
            "*app-misc/sysextra".to_string(),
        ],
        MergeWaitScope::System,
    );
    assert_eq!(
        cps(&entries, &set),
        HashSet::from([
            "sys-libs/glibc".to_string(),
            "app-misc/sysextra".to_string()
        ])
    );
}

/// Oracle: same upstream test, `deep` adds the transitive runtime deps
/// (`runtime-dep`, `deeper-runtime-dep`) but not the build-time-only
/// reachable `build-dep` (DEPEND, never merge-wait).
#[test]
fn deep_scope_adds_transitive_runtime_but_not_buildtime() {
    let mut glibc = wait_entry("sys-libs", "glibc");
    glibc.deps = vec![runtime_edge("dev-libs", "runtime-dep")];
    let mut rt = wait_entry("dev-libs", "runtime-dep");
    rt.deps = vec![runtime_edge("dev-libs", "deeper-runtime-dep")];
    let mut unrelated = wait_entry("app-misc", "unrelated");
    unrelated.deps = vec![build_edge("dev-libs", "build-dep")];
    let entries = vec![
        glibc,
        wait_entry("app-misc", "sysextra"),
        rt,
        wait_entry("dev-libs", "deeper-runtime-dep"),
        unrelated,
        wait_entry("dev-libs", "build-dep"),
    ];
    let set = merge_wait_set(
        &entries,
        &[
            "*sys-libs/glibc".to_string(),
            "*app-misc/sysextra".to_string(),
        ],
        MergeWaitScope::Deep,
    );
    assert_eq!(
        cps(&entries, &set),
        HashSet::from([
            "sys-libs/glibc".to_string(),
            "app-misc/sysextra".to_string(),
            "dev-libs/runtime-dep".to_string(),
            "dev-libs/deeper-runtime-dep".to_string(),
        ])
    );
}

/// A `required_by`-only edge (no `deps` record) still counts as runtime
/// for the deep walk, like the merge-order digraph's synthesized fallback.
#[test]
fn deep_scope_follows_required_by_fallback_edges() {
    let sys = wait_entry("dev-libs", "sys");
    let mut child = wait_entry("dev-libs", "child");
    // `sys` depends on `child`: recorded only on the child's side.
    child.required_by = vec![("dev-libs".into(), "sys".into())];
    let entries = vec![sys, child];
    let set = merge_wait_set(
        &entries,
        &["*dev-libs/sys".to_string()],
        MergeWaitScope::Deep,
    );
    assert_eq!(
        cps(&entries, &set),
        HashSet::from(["dev-libs/sys".to_string(), "dev-libs/child".to_string()])
    );
}

/// `PDEPEND` is a runtime-post edge: still a deep runtime dep.
/// Portage: `ignore_priority` keeps `runtime or runtime_post`.
#[test]
fn deep_scope_follows_pdepend_edges() {
    let mut owner = wait_entry("dev-libs", "owner");
    owner.deps = vec![post_edge("dev-libs", "post")];
    let entries = vec![owner, wait_entry("dev-libs", "post")];
    let set = merge_wait_set(
        &entries,
        &["*dev-libs/owner".to_string()],
        MergeWaitScope::Deep,
    );
    assert_eq!(
        cps(&entries, &set),
        HashSet::from(["dev-libs/owner".to_string(), "dev-libs/post".to_string()])
    );
}

/// Portage `lib/_emerge/_find_deep_system_runtime_deps.py:73-81` walks each
/// package node's own children, so two slots of one cp keep separate runtime
/// closures: seeding only the childless slot must not pull the sibling's child
/// in through the cp-level `required_by` fallback.
#[test]
fn deep_scope_keeps_two_slots_of_one_cp_apart() {
    let mut slotted = wait_entry("dev-libs", "dup");
    slotted.slot = Some("1".into());
    slotted.deps = vec![runtime_edge("dev-libs", "child")];
    let mut other = wait_entry("dev-libs", "dup");
    other.slot = Some("2".into());
    let mut child = wait_entry("dev-libs", "child");
    child.required_by = vec![("dev-libs".into(), "dup".into())];
    let entries = vec![slotted, other, child];
    // Seeding only the childless slot leaves the child out.
    let set = merge_wait_set(
        &entries,
        &["*dev-libs/dup:2".to_string()],
        MergeWaitScope::Deep,
    );
    assert_eq!(set, HashSet::from([1]));
    // Seeding only the slot with the runtime edge still reaches the child.
    let set = merge_wait_set(
        &entries,
        &["*dev-libs/dup:1".to_string()],
        MergeWaitScope::Deep,
    );
    assert_eq!(set, HashSet::from([0, 2]));
}

/// A two-cycle is cyclic; a chain and a lone self-edge are not.
/// Portage: `len(component) > 1`.
#[test]
fn cyclic_nodes_needs_more_than_one_member() {
    assert_eq!(cyclic_nodes(&[vec![1], vec![0]]), HashSet::from([0, 1]));
    assert!(cyclic_nodes(&[vec![1], vec![2], vec![]]).is_empty());
    assert!(cyclic_nodes(&[vec![0]]).is_empty());
    assert!(cyclic_nodes(&[vec![]]).is_empty());
}

/// Oracle: commit `3d677b7b2` (bug 452172): inside a cycle the post-merge
/// edge stops gating the build, while the hard edge still does.
#[test]
fn blocking_deps_ignores_post_edges_of_cyclic_owners() {
    let mut a = wait_entry("dev-libs", "cycA");
    a.deps = vec![runtime_edge("dev-libs", "cycB")];
    a.required_by = vec![("dev-libs".into(), "cycB".into())];
    let mut b = wait_entry("dev-libs", "cycB");
    b.deps = vec![post_edge("dev-libs", "cycA")];
    b.required_by = vec![("dev-libs".into(), "cycA".into())];
    let entries = vec![a, b];
    let deps = vec![HashSet::from([1]), HashSet::from([0])];
    let cyclic: HashSet<usize> = HashSet::from([0, 1]);
    let blocking = blocking_deps(&entries, &deps, &cyclic);
    assert_eq!(blocking[0], HashSet::from([1]), "RDEPEND still blocks");
    assert!(blocking[1].is_empty(), "PDEPEND no longer blocks");
}

/// Acyclic graphs dispatch exactly as before: `blocking_deps` is the
/// identity when nothing is cyclic (the `deep` default stays
/// byte-identical there).
#[test]
fn blocking_deps_leaves_acyclic_graphs_alone() {
    let entries = vec![wait_entry("dev-libs", "a"), wait_entry("dev-libs", "b")];
    let deps = vec![HashSet::from([1]), HashSet::new()];
    let blocking = blocking_deps(&entries, &deps, &HashSet::new());
    assert_eq!(blocking, deps);
}

/// `merge-wait` matches as a whole `FEATURES` token only.
#[test]
fn features_hold_all_merges_matches_whole_tokens() {
    assert!(features_hold_all_merges("sandbox merge-wait userpriv"));
    assert!(!features_hold_all_merges("sandbox -merge-wait userpriv"));
    assert!(!features_hold_all_merges(""));
    assert!(!features_hold_all_merges("merge-wait-something"));
}

/// `entry_matches_any` (the `findAtomForPackage` equivalent) backs the
/// `system`/`deep` seeds: `*`-prefixed profile atoms match the entry.
#[test]
fn system_seed_matches_star_profile_atoms() {
    let glibc = wait_entry("sys-libs", "glibc");
    assert!(entry_matches_any(&glibc, &["*sys-libs/glibc".to_string()]));
    assert!(!entry_matches_any(
        &glibc,
        &["*app-misc/sysextra".to_string()]
    ));
    assert!(!entry_matches_any(&glibc, &[]));
}
