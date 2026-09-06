use super::*;

#[test]
fn find_hard_cycles_reports_a_build_time_cycle_and_ignores_a_run_time_one() {
    let hardcpv = |p: &str| format!("dev-libs/{p}-1.0");
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());

    // Pure hard cycle: reported from the earliest-inserted node; no `deps`, so no
    // slot-op arm and no vdb consulted.
    let mut hard: EdgeKindMap = HashMap::new();
    hard.insert((cp("hcb"), cp("hca")), (true, false));
    hard.insert((cp("hca"), cp("hcb")), (true, false));
    // Explicit `discovery`: hca discovered first (unit entries default to 0).
    let mut first = graph_entry("dev-libs", "hca", "1.0");
    first.discovery = 0;
    let mut second = graph_entry("dev-libs", "hcb", "1.0");
    second.discovery = 1;
    let entries = vec![first, second];
    assert_eq!(
        find_hard_cycles(
            &entries,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        ),
        vec![vec![hardcpv("hcb"), hardcpv("hca")]]
    );

    // One edge with a run-time alternative: breakable, nothing reported.
    let mut mixed: EdgeKindMap = HashMap::new();
    mixed.insert((cp("hcb"), cp("hca")), (true, false));
    mixed.insert((cp("hca"), cp("hcb")), (true, true));
    assert!(
        find_hard_cycles(
            &entries,
            &mixed,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );

    // An AlreadyInstalled node can't be in a build-time cycle.
    let installed = vec![
        graph_entry("dev-libs", "hca", "1.0"),
        GraphEntry {
            discovery: 0,
            outcome: PretendOutcome::AlreadyInstalled {
                version: "1.0".to_string(),
            },
            ..graph_entry("dev-libs", "hcb", "1.0")
        },
    ];
    assert!(
        find_hard_cycles(
            &installed,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );
}

/// A true self-loop reports the one-member ring; a same-cp cross-slot edge does
/// not.
#[test]
fn find_hard_cycles_reports_a_true_self_loop_but_not_a_cross_slot_edge() {
    let hardcpv = |p: &str, v: &str| format!("dev-libs/{p}-{v}");
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let self_edge = |pkg: &str, atom: &str, branch: Option<u32>| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string(),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority: buildtime,
        disjunctive: branch.is_some(),
        alt: branch.map(|b| (0, b)),
        key: 4,
    };
    let mut entries = vec![graph_entry("dev-libs", "hcs", "1.0")];
    entries[0].deps = vec![self_edge("hcs", ">=dev-libs/hcs-1.0", Some(0))];
    let mut hard: EdgeKindMap = HashMap::new();
    hard.insert((cp("hcs"), cp("hcs")), (true, false));
    assert_eq!(
        find_hard_cycles(
            &entries,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        ),
        vec![vec![hardcpv("hcs", "1.0")]]
    );

    // Cross-slot edge: the atom names another version, so no cycle.
    let mut cross = vec![graph_entry("dev-libs", "hcs", "1.0")];
    cross[0].deps = vec![self_edge("hcs", "=dev-libs/hcs-2.0", Some(0))];
    assert!(
        find_hard_cycles(
            &cross,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );

    // An inline (non-`||`) self edge with a matching atom loops too.
    let mut inline = vec![graph_entry("dev-libs", "hcs", "1.0")];
    inline[0].deps = vec![self_edge("hcs", "dev-libs/hcs", None)];
    assert_eq!(
        find_hard_cycles(
            &inline,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        ),
        vec![vec![hardcpv("hcs", "1.0")]]
    );

    // An installed instance keeps a self-loop invisible (out of scope: reporting it
    // would flip an outcome-correct resolve into a spurious abort).
    let root = TempDir::new("portuale-selfloop").keep();
    let inst_dir = root.join("var/db/pkg/dev-libs/hcs-1.0");
    std::fs::create_dir_all(&inst_dir).unwrap();
    std::fs::write(inst_dir.join("SLOT"), "0\n").unwrap();
    assert!(find_hard_cycles(&entries, &hard, &root, &HashMap::new(), None).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn find_hard_cycles_ignores_a_suppressed_disjunctive_slot_op_branch() {
    // Portage's digraph holds the selected `||` branch (first resolvable branch in
    // written order), not a suppressed one: the discarded `:=` branch must not
    // harden `own -> kea`.
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let runtime_slot_op = DepPriority {
        runtime: true,
        runtime_slot_op: true,
        ..DepPriority::default()
    };
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let alt_edge = |pkg: &str, atom: &str, priority: DepPriority, branch: u32| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string(),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: true,
        alt: Some((0, branch)),
        key: 0,
    };
    let mut own = graph_entry("dev-libs", "own", "1.0");
    own.deps = vec![
        alt_edge("keb", "dev-libs/keb", runtime, 0),
        alt_edge("kea", "dev-libs/kea:=", runtime_slot_op, 1),
    ];
    let mut kea = graph_entry("dev-libs", "kea", "1.0");
    kea.deps = vec![DepEdge {
        atom: "dev-libs/own".to_string(),
        evaluated: "dev-libs/own".to_string(),
        category: "dev-libs".to_string(),
        package: "own".to_string(),
        priority: buildtime,
        disjunctive: false,
        alt: None,
        key: 3,
    }];
    let keb = graph_entry("dev-libs", "keb", "1.0");
    let entries = vec![own, kea, keb];
    // Kept branch and hard back-edge recorded; the suppressed branch has no entry.
    let mut map: EdgeKindMap = HashMap::new();
    map.insert((cp("keb"), cp("own")), (false, true));
    map.insert((cp("own"), cp("kea")), (true, false));
    assert!(
        find_hard_cycles(
            &entries,
            &map,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            None
        )
        .is_empty()
    );
}
