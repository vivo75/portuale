use super::*;

#[test]
fn build_digraph_drops_self_edges_except_unsatisfied_buildtime() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // A runtime self-edge is dropped.
    let entries = vec![new_entry(
        "dev-libs",
        "selfish",
        "1.0",
        vec![plain_edge(
            "dev-libs/selfish",
            "dev-libs",
            "selfish",
            runtime,
        )],
    )];
    let g = build_digraph(
        &entries,
        &["dev-libs/selfish".to_string()],
        root,
        &HashMap::new(),
    );
    assert!(
        g.children[0].is_empty(),
        "runtime self-edge dropped: {:?}",
        g.children[0]
    );
    // An unsatisfied build-time self-edge is kept.
    let entries = vec![new_entry(
        "dev-libs",
        "selfish",
        "1.0",
        vec![plain_edge(
            "dev-libs/selfish",
            "dev-libs",
            "selfish",
            DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
        )],
    )];
    let g = build_digraph(
        &entries,
        &["dev-libs/selfish".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![0],
        "unsatisfied buildtime self-edge kept: {:?}",
        g.children[0]
    );
}

#[test]
fn deep_system_deps_follows_only_runtime_edges() {
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sys".to_string()],
        ..portage_profile::Config::default()
    };
    let entries = vec![
        new_entry("dev-libs", "sys", "1.0", Vec::new()),
        new_entry("dev-libs", "rt", "1.0", Vec::new()),
        new_entry("dev-libs", "bt", "1.0", Vec::new()),
        new_entry("dev-libs", "post", "1.0", Vec::new()),
    ];
    let g = test_graph(
        4,
        &[
            (0, 1, prio(2)), // runtime
            (0, 2, prio(1)), // buildtime
            (1, 3, prio(4)), // runtime_post
        ],
    );
    assert_eq!(
        deep_system_deps(&g, &entries, &config),
        vec![true, true, false, true]
    );
}
