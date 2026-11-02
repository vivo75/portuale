//! #572 (from #567's S0): an installed new-style virtual's `||` must be
//! re-resolved in the consumer's graph, so a co-scheduled upgrade that replaces
//! the arm it bound to makes the walk fall through to the next arm.
//!
//! Expected values are Portage 3.0.83's own resolver on the same shape
//! (`portage/tests/resolver/ResolverPlayground.py`; captures and the mechanism
//! write-up in `LLM/evidence/2026-10-28-572/`): `p572enc` is merged as a child of
//! the kept `[nomerge] virtual/p572virt-2.0`, because the scheduled
//! `p572prov-5.44` replaces the installed `5.42` in slot 0 and
//! `dep_zapdeps`'s downgrade guard (bug #531656) drops the `=p572prov-5.42*` arm.

use super::*;
use crate::tests::tests_consumer_pin_conflict::{hermetic_root, request_in, resolve_in};

/// Backlog #572 review focus: `--nodeps` follows no dependencies, so the
/// fall-through arm must not merge even though the upgrade is co-scheduled.
/// Oracle: hermetic playground `probe572_nodeps_onlydeps.py`
/// (`nodeps MERGELIST ['dev-lang/perl-5.44.0', 'dev-perl/Encode-Locale-1.5']`).
#[test]
fn nodeps_merges_the_arguments_but_not_the_fall_through_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf"]);
    let mut req = request_in(&root, &["=dev-libs/p572prov-5.44", "dev-libs/p572leaf"]);
    req.nodeps = true;
    let result = req.resolve().unwrap();
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        !names.iter().any(|n| n == "dev-libs/p572enc"),
        "no deps are followed under --nodeps: {names:?}"
    );
}

/// Backlog #572 review focus: `--onlydeps` merges build-time deps only, so the
/// runtime fall-through arm must not merge.
/// Oracle: hermetic playground `probe572_nodeps_onlydeps.py`
/// (`onlydeps MERGELIST []`).
#[test]
fn onlydeps_does_not_merge_the_fall_through_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf"]);
    let mut req = request_in(&root, &["=dev-libs/p572prov-5.44", "dev-libs/p572leaf"]);
    req.onlydeps = true;
    let result = req.resolve().unwrap();
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        !names.iter().any(|n| n == "dev-libs/p572enc"),
        "only build-time deps merge under --onlydeps: {names:?}"
    );
}

/// The `#567` p02 shape: no `--update`, no `--deep`. The perl-shape upgrade is
/// an argument; the virtual is installed and kept; its `||` must fall through.
///
/// Green since #572 Task 2 (was RED on `main`: the `||` never reached the graph).
/// (`LLM/evidence/2026-10-28-572/REPORT.md`).
#[test]
fn or_group_of_an_installed_virtual_falls_through_when_an_upgrade_replaces_its_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf"]);
    let result = resolve_in(&root, &["=dev-libs/p572prov-5.44", "dev-libs/p572leaf"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/p572enc"),
        "the || fell through to dev-libs/p572enc-3.210 in Portage, portuale merged: {names:?}"
    );
}

/// Tripwire (Task 2 Step 1): the installed `virtual/libc-1.0` shape stays
/// silent — no `U`, no `N fakeglibc` row. Same oracle as
/// `tests::graph::update_without_deep_keeps_an_installed_virtual_provider_silent`;
/// the #572 expansion must never walk a virtual with no tree ebuild.
#[test]
fn installed_virtual_without_tree_candidate_stays_silent() {
    let entries = graph_update("dev-libs/toolchainroot");
    assert_eq!(
        entries,
        vec![
            (
                "dev-libs/plainleaf".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            ),
            (
                "virtual/libc".to_string(),
                PretendOutcome::AlreadyInstalled {
                    version: "1.0".to_string()
                }
            ),
            (
                "dev-libs/toolchainroot".to_string(),
                PretendOutcome::New {
                    version: "1.0".to_string()
                }
            )
        ]
    );
}

/// Tripwire (Task 2 Step 1): the Q19 sibling-virtual skip rows stay as
/// `tests_consumer_or_view` pins them. Same oracle
/// (`q19-merge-portage.txt`); the #572 expansion must not erase the pins by
/// queueing the chosen arm as a hard dep.
#[test]
fn q19_sibling_virtual_skip_rows_stay() {
    let root = hermetic_root(&[]);
    let result = resolve_in(&root, &["dev-libs/p590xpull"]);
    let mut parents: Vec<(&str, &str)> = result
        .skipped_updates
        .iter()
        .map(|r| (r.consumer_cpv.as_str(), r.atom.as_str()))
        .collect();
    parents.sort_unstable();
    assert_eq!(
        parents,
        vec![
            (
                "virtual/p590vva-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
            (
                "virtual/p590vvb-1:0/0::testrepo",
                "=dev-libs/p590vprov-5.42*"
            ),
        ]
    );
}

/// The `#567` p03 shape (nested virtual): Try-Tiny's `||` names the installed
/// virtual first, but the virtual's own `||` is doubly dead once the
/// co-scheduled upgrade replaces its match, so the choice must fall through
/// to `dev-libs/p572sub`.
///
/// Portage playground oracle (`LLM/evidence/2026-10-28-572/probe572_p03.py`):
/// mergelist `['dev-perl/Sub-Name-0.280.0-r1', 'dev-lang/perl-5.44.0',
/// 'dev-perl/Try-Tiny-0.320.0']` — the fall-through leaf plus the perl
/// upgrade plus the requested leaf, the virtual kept (nomerge).
///
/// Ignored RED: needs a walk-time mechanism Task 2's feed cannot supply
/// (filed as #643).
#[test]
#[ignore = "#643: nested virtual needs consumer-side || re-resolution"]
fn or_group_of_a_nested_installed_virtual_falls_through_to_the_next_arm() {
    let root = hermetic_root(&["dev-libs/p572try"]);
    let result = resolve_in(&root, &["dev-libs/p572try", "=dev-libs/p572prov-5.44"]);
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(
        names.iter().any(|n| n == "dev-libs/p572sub"),
        "the nested || fell through to dev-libs/p572sub-1.0 in Portage, portuale merged: {names:?}"
    );
}

/// The same graph under `-uD @world`: already green before #572, and it must stay
/// green — the deep walk reaches the virtual by a different route. Asserts the
/// fall-through arm merges exactly once (review focus: no duplicated child).
#[test]
fn deep_update_still_lists_the_fall_through_arm() {
    let root = hermetic_root(&["dev-libs/p572leaf", "dev-libs/p572prov"]);
    let mut req = request_in(&root, &["dev-libs/p572leaf", "dev-libs/p572prov"]);
    req.update = true;
    req.deep = Deep::Unlimited;
    req.selective = true;
    let result = req.resolve().unwrap();
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(names.contains(&"dev-libs/p572enc".to_string()), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| *n == "dev-libs/p572enc").count(),
        1,
        "one node, one parent edge — the seed must not duplicate the walked child: {names:?}"
    );
}

/// `--emptytree` rebuilds the installed virtual, so the walk reaches its
/// `||` directly; the feed's merge-bound dedup must not fire a second node.
#[test]
fn emptytree_merges_the_fall_through_arm_once() {
    let root = hermetic_root(&["dev-libs/p572leaf"]);
    let mut req = request_in(&root, &["dev-libs/p572leaf", "=dev-libs/p572prov-5.44"]);
    req.empty = true;
    let result = req.resolve().unwrap();
    let names: Vec<String> = result
        .entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    assert!(names.contains(&"dev-libs/p572enc".to_string()), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| *n == "dev-libs/p572enc").count(),
        1,
        "one node, one parent edge: {names:?}"
    );
}
