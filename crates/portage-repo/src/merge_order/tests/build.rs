use super::*;

#[test]
fn self_referential_disjunctive_bdepend_skips_the_self_branch() {
    // The resolver picks the non-circular `mogoboot` branch; the self
    // branch must not win via its trivial "matches the owner's own node" match.
    // Portage: dep_check.py, dep_zapdeps (L0 finding D)
    let mogo_deps = vec![
        DepEdge {
            atom: ">=dev-lang/mogo-1.24".to_string(),
            evaluated: ">=dev-lang/mogo-1.24".to_string().clone(),
            category: "dev-lang".to_string(),
            package: "mogo".to_string(),
            priority: DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 0)),
            key: 4,
        },
        DepEdge {
            atom: ">=dev-lang/mogoboot-1.24".to_string(),
            evaluated: ">=dev-lang/mogoboot-1.24".to_string(),
            category: "dev-lang".to_string(),
            package: "mogoboot".to_string(),
            priority: DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 1)),
            key: 4,
        },
    ];
    let entries = vec![
        new_entry("dev-lang", "mogo", "1.26", mogo_deps),
        new_entry("dev-lang", "mogoboot", "1.24", Vec::new()),
    ];
    let g = build_digraph(
        &entries,
        &["dev-lang/mogo".to_string()],
        Path::new("/nonexistent-root-for-unit-test"),
        &HashMap::new(),
    );
    assert!(
        g.children[0].iter().all(|&(c, _)| c != 0),
        "mogo must not depend on itself: {:?}",
        g.children[0]
    );
    let to_boot = g.children[0].iter().find(|&&(c, _)| c == 1);
    assert!(
        to_boot.is_some(),
        "mogo must depend on mogoboot: {:?}",
        g.children[0]
    );
    let (_, prios) = to_boot.unwrap();
    assert!(
        prios.iter().any(|p| p.buildtime),
        "the mogo -> mogoboot edge must be the real buildtime one, \
         not the weaker required_by-fallback edge: {prios:?}"
    );
}

#[test]
fn nvc_entries_do_not_participate_in_minimize_elimination() {
    // A `NoVisibleCandidate` entry never enters the minimize elimination.
    // Portage: depgraph.py, _select_package returns None for its atom.
    let plain = |atom: &str| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: "dev-libs".to_string(),
        package: "partly".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut nvc = new_entry("dev-libs", "partly", "1.0", Vec::new());
    nvc.outcome = PretendOutcome::NoVisibleCandidate;
    nvc.slot = Some("0".into());
    nvc.sub_slot = Some("0".into());
    let mut installed = new_entry("dev-libs", "partly", "1.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    installed.slot = Some("0".into());
    installed.sub_slot = Some("0".into());
    let user = new_entry("dev-libs", "user", "1.0", vec![plain("dev-libs/partly")]);
    // NVC first, like the resolver records it. Assert on `build_digraph`
    // (the scheduler view); the tree view never edges to an installed node.
    let entries = vec![nvc, installed, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let g = build_digraph(
        &entries,
        &["dev-libs/user".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
    assert_eq!(
        children,
        vec![0],
        "the NVC entry keeps the edge: {children:?}"
    );
}

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
fn build_digraph_walks_inline_before_deferred_disjunctive_edges() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut deferred = plain_edge("dev-libs/late", "dev-libs", "late", runtime);
    deferred.disjunctive = true;
    deferred.alt = Some((0, 0));
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![
            plain_edge("dev-libs/early", "dev-libs", "early", runtime),
            deferred,
        ],
    );
    let entries = vec![
        owner,
        new_entry("dev-libs", "early", "1.0", Vec::new()),
        new_entry("dev-libs", "late", "1.0", Vec::new()),
    ];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.order,
        vec![0, 1, 2],
        "inline edge discovered before the deferred disjunctive bundle: {:?}",
        g.order
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
