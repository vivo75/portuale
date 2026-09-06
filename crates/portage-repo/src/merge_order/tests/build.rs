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
