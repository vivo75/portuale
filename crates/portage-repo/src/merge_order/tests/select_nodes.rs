use super::*;

#[test]
fn select_nodes_promotes_only_an_unsatisfied_pdep_child() {
    // The provider's optional child is already satisfied for the
    // promotion predicate, so it must NOT be promoted.
    let mut virt = new_entry(
        "virtual",
        "libc",
        "1.0",
        vec![rdepend_edge("sys-libs/prov", "sys-libs", "prov")],
    );
    virt.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![
        virt,
        new_entry(
            "sys-libs",
            "prov",
            "1.0",
            vec![plain_edge(
                "dev-libs/consumer",
                "dev-libs",
                "consumer",
                DepPriority {
                    buildtime: true,
                    optional: true,
                    ..DepPriority::default()
                },
            )],
        ),
        new_entry("dev-libs", "consumer", "1.0", Vec::new()),
        new_entry("dev-libs", "other", "1.0", Vec::new()),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let mut g = build_digraph(
        &entries,
        &["virtual/libc".to_string()],
        root,
        &HashMap::new(),
    );
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(
        order,
        vec![1, 0, 2, 3],
        "an already-satisfied optional PDEPEND child is not promoted"
    );
}
