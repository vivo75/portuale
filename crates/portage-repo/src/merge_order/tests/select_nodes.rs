use super::*;

#[test]
fn select_nodes_pins_the_greedy_batch_and_root_leaf_order() {
    // 1 -> 2, plus the parentless leaf 0: at rung NONE the batch pops
    // both leaves in order; one-at-a-time would pick the parented 2 first.
    let entries: Vec<GraphEntry> = (0..3)
        .map(|i| new_entry("dev-libs", &format!("p{i}"), "1.0", Vec::new()))
        .collect();
    let mut g = test_graph(3, &[(1, 2, prio(2))]);
    let root = Path::new("/nonexistent-root-for-unit-test");
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(order, vec![0, 2, 1], "greedy rung-NONE pops both leaves");
}

#[test]
fn select_nodes_harvests_a_runtime_cycle() {
    // 0 <-> 1 runtime plus 2 -> 0: no NORMAL rung leaf exists, so the
    // cycle handler harvests the ring (installed leaf first).
    let entries: Vec<GraphEntry> = (0..3)
        .map(|i| new_entry("dev-libs", &format!("c{i}"), "1.0", Vec::new()))
        .collect();
    let mut g = test_graph(3, &[(0, 1, prio(2)), (1, 0, prio(2)), (2, 0, prio(2))]);
    g.installed[1] = true;
    let root = Path::new("/nonexistent-root-for-unit-test");
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(
        order,
        vec![1, 0, 2],
        "the ring harvest prefers the installed leaf, then the requirer"
    );
}

#[test]
fn select_nodes_escalates_to_the_satisfied_range() {
    // A ring of satisfied buildtime edges: only the `drop_satisfied`
    // escalation to the satisfied range breaks it.
    let entries: Vec<GraphEntry> = (0..2)
        .map(|i| new_entry("dev-libs", &format!("s{i}"), "1.0", Vec::new()))
        .collect();
    let mut g = test_graph(2, &[(0, 1, prio(65)), (1, 0, prio(65))]);
    g.installed[1] = true;
    let root = Path::new("/nonexistent-root-for-unit-test");
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(
        order,
        vec![1, 0],
        "the satisfied-range harvest prefers the installed leaf"
    );
}

#[test]
fn select_nodes_promotes_an_unsatisfied_pdep_child_to_asap() {
    // virtual/libc is installed and seeds its provider P as asap; P's
    // still-alive unsatisfied PDEPEND child C is promoted ahead of the
    // bias order.
    let mut virt = new_entry(
        "virtual",
        "libc",
        "1.0",
        vec![
            rdepend_edge("sys-libs/libc-prov", "sys-libs", "libc-prov"),
            runtime_edge("dev-libs/other", "dev-libs", "other"),
        ],
    );
    virt.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![
        virt,
        new_entry(
            "sys-libs",
            "libc-prov",
            "1.0",
            vec![{
                let mut e = runtime_edge("dev-libs/consumer", "dev-libs", "consumer");
                e.priority = DepPriority {
                    runtime_post: true,
                    ..DepPriority::default()
                };
                e
            }],
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
        vec![1, 2, 3, 0],
        "the promoted PDEPEND child merges before the unrelated dep"
    );
}

// -----------------------------------------------------------------
// Fix pass: the reviewer's counterexamples
// -----------------------------------------------------------------

#[test]
fn select_nodes_prefers_a_leaf_whose_parent_is_an_asap_node() {
    // The asap block cannot select and the cycle search's asap-only set
    // is empty, so the rung loop reaches the asap-parent preference:
    // leaf 4 (parent 1 is asap) wins over 2/3.
    let entries = vec![
        new_entry(
            "virtual",
            "libc",
            "1.0",
            vec![rdepend_edge("sys-libs/prov", "sys-libs", "prov")],
        ),
        new_entry(
            "sys-libs",
            "prov",
            "1.0",
            vec![
                plain_edge(
                    "dev-libs/x",
                    "dev-libs",
                    "x",
                    DepPriority {
                        buildtime: true,
                        ..DepPriority::default()
                    },
                ),
                runtime_edge("dev-libs/leaf", "dev-libs", "leaf"),
                {
                    let mut e = runtime_edge("dev-libs/npost", "dev-libs", "npost");
                    e.priority = DepPriority {
                        runtime_post: true,
                        ..DepPriority::default()
                    };
                    e
                },
            ],
        ),
        new_entry("dev-libs", "bparented", "1.0", Vec::new()),
        new_entry("dev-libs", "npost", "1.0", Vec::new()),
        new_entry("dev-libs", "leaf", "1.0", Vec::new()),
        new_entry("dev-libs", "x", "1.0", Vec::new()),
        new_entry(
            "dev-libs",
            "cparent",
            "1.0",
            vec![runtime_edge("dev-libs/bparented", "dev-libs", "bparented")],
        ),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let mut g = test_graph(
        7,
        &[
            (0, 1, prio(1)),
            (1, 5, prio(1)),
            (1, 4, prio(2)),
            (1, 3, prio(4)),
            (6, 2, prio(2)),
        ],
    );
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(
        order,
        vec![4, 5, 1, 0, 3, 2, 6],
        "the asap-parent preference picks leaf 4 before the fallback would"
    );
}

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

#[test]
fn select_nodes_escalates_to_the_satisfied_range_before_the_roots() {
    // The early SATISFIED escalation must not steal the selection from
    // the roots-last-resort fallback running in the same iteration.
    let entries = vec![
        new_entry("virtual", "libc", "1.0", Vec::new()),
        new_entry("dev-libs", "p1", "1.0", Vec::new()),
        new_entry("dev-libs", "p2", "1.0", Vec::new()),
        new_entry("dev-libs", "p3", "1.0", Vec::new()),
        new_entry("dev-libs", "p4", "1.0", Vec::new()),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let mut g = test_graph(5, &[(0, 4, prio(1)), (3, 2, prio(64)), (4, 0, prio(8))]);
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(
        order,
        vec![2, 1, 3, 4, 0],
        "the roots fallback keeps its turn"
    );
}

#[test]
fn select_nodes_pins_the_normal_range_cycle_pass() {
    // When the range is already SATISFIED the NORMAL cycle pass must
    // still run.
    let mut virt = new_entry(
        "virtual",
        "libc",
        "1.0",
        vec![rdepend_edge("sys-libs/p1", "sys-libs", "p1")],
    );
    virt.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![
        virt,
        new_entry("sys-libs", "p1", "1.0", Vec::new()),
        new_entry("dev-libs", "p2", "1.0", Vec::new()),
        new_entry("dev-libs", "p3", "1.0", Vec::new()),
        new_entry("dev-libs", "p4", "1.0", Vec::new()),
        new_entry("dev-libs", "p5", "1.0", Vec::new()),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let mut g = test_graph(
        6,
        &[
            (1, 2, prio(6)),
            (1, 4, prio(6)),
            (2, 0, prio(4)),
            (2, 1, prio(32)),
            (3, 5, prio(64)),
            (4, 0, prio(6)),
            (4, 2, prio(33)),
            (4, 5, prio(1)),
        ],
    );
    let order = select_nodes(&mut g, &entries, root, None);
    assert_eq!(order, vec![1, 0, 5, 2, 3, 4], "the NORMAL cycle pass runs");
}
