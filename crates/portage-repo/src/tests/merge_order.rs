use super::*;
use crate::merge_order::TreeDisplayOrderArgs;

/// `tree_display_order` sequences the p2b tree like Portage's tree-mode retlist.
#[test]
fn tree_display_order_sequences_the_p2b_tree_like_portage() {
    let atoms = vec![
        "dev-libs/p2btarget".to_string(),
        "dev-libs/p2bowner".to_string(),
        "dev-libs/newpkg".to_string(),
    ];
    // The `@system` seed matters: the fixture profile lists `*dev-libs/newpkg`,
    // so the bias pulls `newpkg` first like Portage's own bias does.
    let mut order_config = test_config();
    order_config.system_packages = vec!["dev-libs/newpkg".to_string()];
    let mut entries = graph_update_entries(
        &["dev-libs/p2btarget", "dev-libs/p2bowner", "dev-libs/newpkg"],
        &order_config,
    );
    // The resolve must still graph exactly the three merge-bound entries.
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
    // Display-time (merge) order, as `run()` passes it.
    let rank = |e: &GraphEntry| match (e.category.as_str(), e.package.as_str()) {
        ("dev-libs", "newpkg") => 0,
        ("dev-libs", "p2btarget") => 1,
        ("dev-libs", "p2bowner") => 2,
        (c, p) => panic!("unexpected entry in p2b graph: {c}/{p}"),
    };
    entries.sort_by_key(rank);
    let order = merge_order::tree_display_order(merge_order::TreeDisplayOrderArgs {
        entries: &entries,
        top_level_atoms: &atoms,
        config: &order_config,
        root: &root,
        implicit_system_deps: true,
        repos: &repos,
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
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

/// An admitted removal waits on its owner in tree mode.
#[test]
fn tree_display_order_schedules_an_admitted_removal_after_its_owner() {
    let atoms = vec!["dev-libs/blockerpkg".to_string()];
    let entries = graph_update_entries(&["dev-libs/blockerpkg"], &test_config());
    let root = fixtures_root();
    let repos = find_repos(&root).expect("repos");
    let order = merge_order::tree_display_order(merge_order::TreeDisplayOrderArgs {
        entries: &entries,
        top_level_atoms: &atoms,
        config: &test_config(),
        root: &root,
        implicit_system_deps: true,
        repos: &repos,
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
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
