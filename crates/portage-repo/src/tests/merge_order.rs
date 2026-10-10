use super::*;

/// #131 S1: `tree_display_order` sequences the p2b tree like real's
/// tree-mode retlist (bed `l0-fx-20260924T090502Z`: real's display
/// walks `[p2bowner, newpkg, (nomerge p2bowner,) blocks b,
/// p2btarget]`, i.e. merge direction `[p2btarget, newpkg,
/// p2bowner]`). The entries are first put in the display-time
/// (flat merge) order `[newpkg, p2btarget, p2bowner]` -- pinned
/// independently by the flat pin -- because the scheduler's
/// discovery seed follows the input Vec order, and `run()` hands
/// `print_tree` the merge-ordered slice, not resolver order
/// (everything `tree_display_order` reads -- deps, blockers,
/// `required_by` -- is value-addressed, so the reorder is safe).
#[test]
fn tree_display_order_sequences_the_p2b_tree_like_portage() {
    let atoms = vec![
        "dev-libs/p2btarget".to_string(),
        "dev-libs/p2bowner".to_string(),
        "dev-libs/newpkg".to_string(),
    ];
    // The `@system` seed matters: the fixture profile lists
    // `*dev-libs/newpkg` in `repo/profiles/base/packages`, and the
    // display-time run resolves with that profile config -- so the
    // bias pulls `newpkg` first exactly like real's own bias does.
    // Plain `test_config()` has an empty system set and would seed
    // nothing (its batch comes out `[owner, newpkg]`).
    let mut order_config = test_config();
    order_config.system_packages = vec!["dev-libs/newpkg".to_string()];
    let mut entries = graph_update_entries(
        &["dev-libs/p2btarget", "dev-libs/p2bowner", "dev-libs/newpkg"],
        &order_config,
    );
    // The resolve must still graph exactly the three merge-bound
    // entries -- the system seed reorders, never re-members.
    let mut names: Vec<String> = entries
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["dev-libs/newpkg", "dev-libs/p2bowner", "dev-libs/p2btarget"],
        "system seed must not change p2b membership: {names:?}"
    );
    let root = fixtures_root();
    let repos = find_repos(&root).expect("repos");
    // Display-time (merge) order, as `run()` passes it: the
    // scheduler's discovery seed follows the input Vec order.
    let rank = |e: &GraphEntry| match (e.category.as_str(), e.package.as_str()) {
        ("dev-libs", "newpkg") => 0,
        ("dev-libs", "p2btarget") => 1,
        ("dev-libs", "p2bowner") => 2,
        (c, p) => panic!("unexpected entry in p2b graph: {c}/{p}"),
    };
    entries.sort_by_key(rank);
    let order = merge_order::tree_display_order(
        &entries,
        &atoms,
        &order_config,
        &root,
        true,
        &repos,
        true,
        &HashMap::new(),
    );
    let names: Vec<String> = order
        .iter()
        .map(|&i| format!("{}/{}", entries[i].category, entries[i].package))
        .collect();
    assert_eq!(
        names,
        vec!["dev-libs/p2btarget", "dev-libs/newpkg", "dev-libs/p2bowner"],
        "tree-mode retlist order, merge direction: {names:?}"
    );
}

/// #131 S1: an admitted removal waits on its owner in tree mode
/// (real reverses the validation edge on admission,
/// `depgraph.py:9998-10020`). u1's tree retlist is `[blockerpkg,
/// samepkg-uninstall, (Blocker appended at selection)]` — the owner
/// first. Without the wait the one-at-a-time pick takes the removal
/// (it has parents, the owner does not) and the display opens on the
/// merge row, losing real's leading `[nomerge]` ancestor.
#[test]
fn tree_display_order_schedules_an_admitted_removal_after_its_owner() {
    let atoms = vec!["dev-libs/blockerpkg".to_string()];
    let entries = graph_update_entries(&["dev-libs/blockerpkg"], &test_config());
    let root = fixtures_root();
    let repos = find_repos(&root).expect("repos");
    let order = merge_order::tree_display_order(
        &entries,
        &atoms,
        &test_config(),
        &root,
        true,
        &repos,
        true,
        &HashMap::new(),
    );
    let names: Vec<String> = order
        .iter()
        .map(|&i| format!("{}/{}", entries[i].category, entries[i].package))
        .collect();
    assert_eq!(
        names,
        vec!["dev-libs/blockerpkg", "dev-libs/samepkg"],
        "owner before its admitted removal: {names:?}"
    );
    assert!(
        matches!(entries[order[1]].outcome, PretendOutcome::Uninstall { .. }),
        "second node is the uninstall: {:?}",
        entries[order[1]].outcome
    );
}

#[test]
fn uninstall_entries_follow_their_owner_in_merge_order() {
    // #68/#72 B3 / B0 u1: `blockerpkg` blocks installed `samepkg-1.0`,
    // resolved by removing it -- the removal entry real serializes
    // right after its owner (real capture: ebuild row, `[uninstall]`
    // row, `[blocks b]`).
    let entries = graph_entries_real("dev-libs/blockerpkg");
    let names: Vec<(&str, Option<&str>)> = entries
        .iter()
        .map(|e| (e.package.as_str(), crate::merge_order::entry_version(e)))
        .collect();
    assert_eq!(
        names,
        vec![("blockerpkg", Some("1.0")), ("samepkg", Some("1.0"))]
    );
    assert!(matches!(
        entries[1].outcome,
        PretendOutcome::Uninstall { .. }
    ));
    assert_eq!(
        entries[1].required_by,
        vec![("dev-libs".to_string(), "blockerpkg".to_string())]
    );

    // B0 u3: the owner's own dependency still merges first, then the
    // owner, then the removal.
    let entries = graph_entries_real("dev-libs/blockerorderpkg");
    let names: Vec<&str> = entries.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["newpkg", "blockerorderpkg", "samepkg"]);
    assert!(matches!(
        entries[2].outcome,
        PretendOutcome::Uninstall { .. }
    ));
}
