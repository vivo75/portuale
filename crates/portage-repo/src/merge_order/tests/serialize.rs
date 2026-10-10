use super::*;

#[test]
fn serialize_merge_order_returns_a_permutation_in_dependency_order() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let config = portage_profile::Config::default();
    let entries = vec![
        new_entry(
            "dev-libs",
            "top",
            "1.0",
            vec![runtime_edge("dev-libs/dep", "dev-libs", "dep")],
        ),
        new_entry("dev-libs", "dep", "1.0", Vec::new()),
        new_entry("dev-libs", "isolated", "1.0", Vec::new()),
    ];
    let (order, _) = serialize_merge_order(
        &entries,
        &["dev-libs/top".to_string()],
        &config,
        root,
        true,
        &[],
        true,
        &HashMap::new(),
    );
    assert_eq!(
        order.len(),
        3,
        "every entry is scheduled exactly once: {order:?}"
    );
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![0, 1, 2],
        "a permutation of the entries: {order:?}"
    );
    assert!(
        order.iter().position(|&i| i == 1) < order.iter().position(|&i| i == 0),
        "the dependency merges before its owner: {order:?}"
    );
}

#[test]
fn schedule_graph_prunes_rootless_synthetic_nodes_and_gates_complete_mode() {
    let root = mo_tmp_vdb(
        "schedule",
        &[
            ("dev-libs", "sysseed-1.0", &[("SLOT", "0")]),
            (
                "dev-libs",
                "dep-1.0",
                &[("SLOT", "0"), ("RDEPEND", "dev-libs/grand")],
            ),
            ("dev-libs", "grand-1.0", &[("SLOT", "0")]),
        ],
    );
    let repos: &[RepoConfig] = &[];
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sysseed".to_string()],
        ..portage_profile::Config::default()
    };
    // An all-New resolve is virtuals_only: no installed closure.
    let entries = vec![new_entry("dev-libs", "top", "1.0", Vec::new())];
    let (ext, g, real_n, _) = schedule_graph(
        &entries,
        &["dev-libs/top".to_string()],
        &config,
        &root,
        true,
        repos,
        true,
        &HashMap::new(),
    );
    assert_eq!(
        ext.len(),
        1,
        "no complete-mode closure for an all-New resolve"
    );
    assert_eq!(real_n, 1);
    assert!(g.order.iter().all(|&i| i < real_n));
    // A Reinstall entry turns complete mode on: the @system seed is
    // added, walked, and then pruned as a rootless nomerge node.
    let mut top = new_entry("dev-libs", "top", "1.0", Vec::new());
    top.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: vec!["flip".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let entries = vec![top];
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sysseed".to_string()],
        ..portage_profile::Config::default()
    };
    let (ext, g, real_n, _) = schedule_graph(
        &entries,
        &["dev-libs/top".to_string()],
        &config,
        &root,
        true,
        repos,
        true,
        &HashMap::new(),
    );
    assert_eq!(real_n, 1);
    let seed = ext
        .iter()
        .position(|e| e.package == "sysseed")
        .expect("the @system seed is added");
    assert!(seed >= real_n);
    assert!(!g.alive[seed], "a rootless synthetic node is pruned");
    assert!(!g.order.contains(&seed));
}
