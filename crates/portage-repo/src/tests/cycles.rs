use super::*;

/// `find_restart_cycles` finds the hard-containing cycle a stranded walk leaves
/// behind, one index cycle per closing hard edge.
#[test]
fn find_restart_cycles_reports_a_hard_two_cycle() {
    let dep = |pkg: &str, priority: DepPriority| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}"),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let mut a = graph_entry("dev-libs", "cyc-a", "1.0");
    let mut b = graph_entry("dev-libs", "cyc-b", "1.0");
    a.deps = vec![dep("cyc-b", buildtime)];
    b.deps = vec![dep("cyc-a", buildtime)];
    let entries = vec![a, b];
    let mut map: EdgeKindMap = HashMap::new();
    map.insert(
        (
            (false, "dev-libs".to_string(), "cyc-b".to_string()),
            (false, "dev-libs".to_string(), "cyc-a".to_string()),
        ),
        (true, false),
    );
    map.insert(
        (
            (false, "dev-libs".to_string(), "cyc-a".to_string()),
            (false, "dev-libs".to_string(), "cyc-b".to_string()),
        ),
        (true, false),
    );
    let cycles = find_restart_cycles(
        &entries,
        &map,
        Path::new("/nonexistent-root"),
        &HashMap::new(),
    );
    assert_eq!(cycles.len(), 2, "{cycles:?}");
    for cycle in &cycles {
        assert_eq!(cycle.len(), 2, "{cycles:?}");
    }
    // Canonical rotations of the same loop.
    let mut canon: Vec<Vec<usize>> = cycles
        .iter()
        .map(|c| {
            let min = c.iter().enumerate().min_by_key(|&(_, &i)| i).unwrap().0;
            c.iter().cycle().skip(min).take(c.len()).copied().collect()
        })
        .collect();
    canon.sort();
    canon.dedup();
    assert_eq!(canon, vec![vec![0, 1]], "{cycles:?}");
}

/// Runtime-closing cycle (bug-705986 shape): invisible to `find_hard_cycles` but
/// still a restart trigger while it holds an unsatisfied buildtime edge.
#[test]
fn find_restart_cycles_reports_a_runtime_closing_cycle() {
    let dep = |pkg: &str, priority: DepPriority, key: u8| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}"),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key,
    };
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut a = graph_entry("dev-libs", "cyc-a", "1.0");
    let mut v = graph_entry("dev-libs", "cyc-v", "1.0");
    a.deps = vec![dep("cyc-v", buildtime, 3)];
    v.deps = vec![dep("cyc-a", runtime, 0)];
    let entries = vec![a, v];
    // Only the buildtime half reaches the hard arm.
    let mut map: EdgeKindMap = HashMap::new();
    map.insert(
        (
            (false, "dev-libs".to_string(), "cyc-v".to_string()),
            (false, "dev-libs".to_string(), "cyc-a".to_string()),
        ),
        (true, false),
    );
    let cycles = find_restart_cycles(
        &entries,
        &map,
        Path::new("/nonexistent-root"),
        &HashMap::new(),
    );
    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycles[0].len(), 2, "{cycles:?}");
}

/// A pure-runtime two-cycle is group-merged, never restarted; an acyclic pair is
/// quiet too.
#[test]
fn find_restart_cycles_ignores_runtime_only_and_acyclic_graphs() {
    let dep = |pkg: &str, priority: DepPriority| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}"),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut a = graph_entry("dev-libs", "cyc-a", "1.0");
    let mut b = graph_entry("dev-libs", "cyc-b", "1.0");
    a.deps = vec![dep("cyc-b", runtime)];
    b.deps = vec![dep("cyc-a", runtime)];
    let entries = vec![a, b];
    let mut map: EdgeKindMap = HashMap::new();
    map.insert(
        (
            (false, "dev-libs".to_string(), "cyc-b".to_string()),
            (false, "dev-libs".to_string(), "cyc-a".to_string()),
        ),
        (false, true),
    );
    map.insert(
        (
            (false, "dev-libs".to_string(), "cyc-a".to_string()),
            (false, "dev-libs".to_string(), "cyc-b".to_string()),
        ),
        (false, true),
    );
    assert!(
        find_restart_cycles(
            &entries,
            &map,
            Path::new("/nonexistent-root"),
            &HashMap::new()
        )
        .is_empty()
    );
    // Acyclic: b's edge dropped.
    let mut a = graph_entry("dev-libs", "cyc-a", "1.0");
    let b = graph_entry("dev-libs", "cyc-b", "1.0");
    a.deps = vec![dep(
        "cyc-b",
        DepPriority {
            buildtime: true,
            ..DepPriority::default()
        },
    )];
    let entries = vec![a, b];
    assert!(
        find_restart_cycles(
            &entries,
            &map,
            Path::new("/nonexistent-root"),
            &HashMap::new()
        )
        .is_empty()
    );
}

/// A build-time edge softened by a parallel run-time route hides from hard cycles
/// but still strands the walk, so restart cycles report it.
#[test]
fn softened_buildtime_edge_hides_from_hard_cycles_but_not_restart_cycles() {
    let dep = |pkg: &str, priority: DepPriority, key: u8| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}"),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key,
    };
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut a = graph_entry("dev-libs", "sbr-a", "1.0");
    let mut b = graph_entry("dev-libs", "sbr-b", "1.0");
    a.deps = vec![dep("sbr-b", buildtime, 4), dep("sbr-b", runtime, 0)];
    b.deps = vec![dep("sbr-a", buildtime, 4)];
    let entries = vec![a, b];
    let cp = |pkg: &str| (false, "dev-libs".to_string(), pkg.to_string());
    let mut map: EdgeKindMap = HashMap::new();
    // Hard + soft route: breakable, invisible to the hard reporter.
    map.insert((cp("sbr-b"), cp("sbr-a")), (true, true));
    // Hard only, but one directed edge is no cycle.
    map.insert((cp("sbr-a"), cp("sbr-b")), (true, false));
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
    let cycles = find_restart_cycles(
        &entries,
        &map,
        Path::new("/nonexistent-root"),
        &HashMap::new(),
    );
    // One index cycle per closing hard edge.
    assert_eq!(cycles.len(), 2, "{cycles:?}");
    let mut canon: Vec<Vec<usize>> = cycles
        .iter()
        .map(|c| {
            let min = c.iter().enumerate().min_by_key(|&(_, &i)| i).unwrap().0;
            c.iter().cycle().skip(min).take(c.len()).copied().collect()
        })
        .collect();
    canon.sort();
    canon.dedup();
    assert_eq!(canon, vec![vec![0, 1]], "{cycles:?}");
}

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

/// The printed cycle starts where Portage's does: earliest-inserted node first,
/// not a lowest-index rotation.
#[test]
fn find_hard_cycles_starts_where_portages_get_cycles_starts() {
    let cpv = |c: &str, p: &str| format!("{c}/{p}-1");
    let cp = |c: &str, p: &str| (false, c.to_string(), p.to_string());
    // Explicit `discovery` mirroring the resolve.
    let mut make = graph_entry("dev-util", "u249make", "1");
    make.discovery = 0;
    let mut json = graph_entry("dev-libs", "u249json", "1");
    json.discovery = 1;
    let entries = vec![make, json];
    let mut hard: EdgeKindMap = HashMap::new();
    hard.insert(
        (cp("dev-libs", "u249json"), cp("dev-util", "u249make")),
        (true, false),
    );
    hard.insert(
        (cp("dev-util", "u249make"), cp("dev-libs", "u249json")),
        (true, false),
    );
    assert_eq!(
        find_hard_cycles(
            &entries,
            &hard,
            Path::new("/nonexistent-root"),
            &HashMap::new(),
            // The insertion-rank path (what the resolver threads).
            Some(&[0, 1])
        ),
        vec![vec![
            cpv("dev-libs", "u249json"),
            cpv("dev-util", "u249make")
        ]]
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

/// The dead-end retry fires only for a fresh single-node self-loop closed by a
/// kept `||` self-branch.
#[test]
fn cycle_restartable_gates_the_retry_to_fresh_branch_self_loops() {
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
    let fresh_root = Path::new("/nonexistent-root");
    let mut fresh = vec![graph_entry("dev-libs", "hcs", "1.0")];
    fresh[0].deps = vec![self_edge("hcs", ">=dev-libs/hcs-1.0", Some(0))];
    let kept = kept_alt_branches(&fresh, fresh_root, &HashMap::new());
    assert!(cycle_restartable(
        &fresh,
        &kept,
        &["dev-libs/hcs-1.0".to_string()],
        fresh_root
    ));

    // A two-node ring never retries.
    let pair = vec![
        graph_entry("dev-libs", "hcs", "1.0"),
        graph_entry("dev-libs", "hct", "1.0"),
    ];
    let kept_pair = kept_alt_branches(&pair, fresh_root, &HashMap::new());
    assert!(!cycle_restartable(
        &pair,
        &kept_pair,
        &[
            "dev-libs/hcs-1.0".to_string(),
            "dev-libs/hct-1.0".to_string()
        ],
        fresh_root
    ));

    // An inline (branch-less) self-dep never retries.
    let mut inline = vec![graph_entry("dev-libs", "hcs", "1.0")];
    inline[0].deps = vec![self_edge("hcs", "dev-libs/hcs", None)];
    let kept_inline = kept_alt_branches(&inline, fresh_root, &HashMap::new());
    assert!(!cycle_restartable(
        &inline,
        &kept_inline,
        &["dev-libs/hcs-1.0".to_string()],
        fresh_root
    ));

    // An installed instance of the owner never retries.
    let root = TempDir::new("portuale-restartable").keep();
    let inst_dir = root.join("var/db/pkg/dev-libs/hcs-1.0");
    std::fs::create_dir_all(&inst_dir).unwrap();
    std::fs::write(inst_dir.join("SLOT"), "0\n").unwrap();
    let kept_fresh = kept_alt_branches(&fresh, &root, &HashMap::new());
    assert!(!cycle_restartable(
        &fresh,
        &kept_fresh,
        &["dev-libs/hcs-1.0".to_string()],
        &root
    ));
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

#[test]
fn find_hard_cycles_slot_op_gate_requires_the_child_slot_like_portage() {
    // Portage filters `inst_pkgs` to the child's slot for `:=`: a different-slot
    // installed instance does not satisfy the edge, a same-slot one does.
    let cp = |p: &str| (false, "dev-libs".to_string(), p.to_string());
    let runtime_slot_op = DepPriority {
        runtime: true,
        runtime_slot_op: true,
        ..DepPriority::default()
    };
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    let plain_edge = |pkg: &str, atom: &str, priority: DepPriority, key: u8| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string(),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key,
    };
    let make_entries = || {
        let mut sown = graph_entry("dev-libs", "sown", "1.0");
        sown.discovery = 0;
        sown.deps = vec![plain_edge("sdep", "dev-libs/sdep:=", runtime_slot_op, 0)];
        let mut sdep = graph_entry("dev-libs", "sdep", "1.0");
        sdep.discovery = 1;
        sdep.slot = Some("1".to_string());
        sdep.sub_slot = Some("0".to_string());
        sdep.deps = vec![plain_edge("sown", "dev-libs/sown", buildtime, 3)];
        vec![sown, sdep]
    };
    let make_map = || {
        let mut map: EdgeKindMap = HashMap::new();
        map.insert((cp("sdep"), cp("sown")), (false, true));
        map.insert((cp("sown"), cp("sdep")), (true, false));
        map
    };
    let make_vdb = |name: &str, slot: &str| {
        let root = masters_test_root(name);
        let vdb = root.join("var/db/pkg/dev-libs/sdep-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("SLOT"), format!("{slot}\n")).unwrap();
        root
    };
    // Installed slot `0`, child slot `1`: unsatisfied (hard). Starts at `sdep`
    // (earliest-inserted `sown`, so the ring closes through its child).
    let root = make_vdb("n228-slot-gate-other-slot", "0");
    assert_eq!(
        find_hard_cycles(&make_entries(), &make_map(), &root, &HashMap::new(), None),
        vec![vec![
            "dev-libs/sdep-1.0".to_string(),
            "dev-libs/sown-1.0".to_string()
        ]]
    );
    // Installed slot `1/0`, child slot `1/0`: satisfied (soft).
    let root = make_vdb("n228-slot-gate-same-slot", "1/0");
    assert!(
        find_hard_cycles(&make_entries(), &make_map(), &root, &HashMap::new(), None).is_empty()
    );
}

#[test]
fn find_virtual_cycles_reports_self_loop_ring_and_lollipop() {
    // Portage fails a virtual that (transitively) requires itself through
    // selected providers (upstream virtual-cycle test).
    let redge = |pkg: &str| DepEdge {
        atom: format!("virtual/{pkg}"),
        evaluated: format!("virtual/{pkg}"),
        category: "virtual".to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let virt = |pkg: &str, deps: Vec<DepEdge>| {
        let mut e = graph_entry("virtual", pkg, "1");
        e.deps = deps;
        e
    };
    let fake = Path::new("/nonexistent-root");
    // Self-loop: the single member reports.
    let gzip = virt("gzip", vec![redge("gzip")]);
    assert_eq!(
        find_virtual_cycles(&[gzip], fake),
        vec!["virtual/gzip-1::testrepo".to_string()]
    );
    // Three-ring: every member reports, sorted.
    let ring = vec![
        virt("A", vec![redge("B")]),
        virt("B", vec![redge("C")]),
        virt("C", vec![redge("A")]),
    ];
    assert_eq!(
        find_virtual_cycles(&ring, fake),
        vec![
            "virtual/A-1::testrepo".to_string(),
            "virtual/B-1::testrepo".to_string(),
            "virtual/C-1::testrepo".to_string(),
        ]
    );
    // Lollipop: the whole recursion set reports, not just the loop.
    let lollipop = vec![virt("X", vec![redge("Y")]), virt("Y", vec![redge("Y")])];
    assert_eq!(
        find_virtual_cycles(&lollipop, fake),
        vec![
            "virtual/X-1::testrepo".to_string(),
            "virtual/Y-1::testrepo".to_string(),
        ]
    );
}

#[test]
fn find_virtual_cycles_ignores_non_rdepend_and_non_virtual_shapes() {
    // Portage's stack holds virtuals only and expands RDEPEND alone.
    let redge = |cat: &str, pkg: &str, key: u8| DepEdge {
        atom: format!("{cat}/{pkg}"),
        evaluated: format!("{cat}/{pkg}"),
        category: cat.to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key,
    };
    let fake = Path::new("/nonexistent-root");
    // virtual/V → dev-libs/P → virtual/V: no report.
    let mut v = graph_entry("virtual", "V", "1");
    v.deps = vec![redge("dev-libs", "P", 0)];
    let mut p = graph_entry("dev-libs", "P", "1");
    p.deps = vec![redge("virtual", "V", 0)];
    assert!(find_virtual_cycles(&[v, p], fake).is_empty());
    // DEPEND-only loop: never expanded.
    let mut d1 = graph_entry("virtual", "D1", "1");
    d1.deps = vec![redge("virtual", "D2", 3)];
    let mut d2 = graph_entry("virtual", "D2", "1");
    d2.deps = vec![redge("virtual", "D1", 3)];
    assert!(find_virtual_cycles(&[d1, d2], fake).is_empty());
    // AlreadyInstalled virtual: not merge-bound, never expanded.
    let mut ai = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1".to_string(),
        },
        ..graph_entry("virtual", "AI", "1")
    };
    ai.deps = vec![redge("virtual", "AI", 0)];
    assert!(find_virtual_cycles(&[ai], fake).is_empty());
    // No virtual in the ring: the category gate skips it.
    let mut n1 = graph_entry("dev-libs", "N1", "1");
    n1.deps = vec![redge("dev-libs", "N2", 0)];
    let mut n2 = graph_entry("dev-libs", "N2", "1");
    n2.deps = vec![redge("dev-libs", "N1", 0)];
    assert!(find_virtual_cycles(&[n1, n2], fake).is_empty());
}

#[test]
fn find_virtual_cycles_ignores_a_suppressed_disjunctive_branch() {
    // The walk keeps the first written branch, so `O → D` is suppressed and the
    // loop never closes through it (a documented cut: Portage may probe the
    // suppressed branch during choice evaluation and fail instead).
    let alt_edge = |pkg: &str, branch: u32| DepEdge {
        atom: format!("virtual/{pkg}"),
        evaluated: format!("virtual/{pkg}"),
        category: "virtual".to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: true,
        alt: Some((0, branch)),
        key: 0,
    };
    let plain_edge = |pkg: &str| DepEdge {
        atom: format!("virtual/{pkg}"),
        evaluated: format!("virtual/{pkg}"),
        category: "virtual".to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let mut o = graph_entry("virtual", "O", "1");
    o.deps = vec![alt_edge("plain", 0), alt_edge("D", 1)];
    let plain = graph_entry("virtual", "plain", "1");
    let mut d = graph_entry("virtual", "D", "1");
    d.deps = vec![plain_edge("O")];
    let mut p = graph_entry("dev-libs", "P", "1");
    p.deps = vec![plain_edge("D")];
    let entries = vec![o, plain, d, p];
    assert!(find_virtual_cycles(&entries, Path::new("/nonexistent-root")).is_empty());
}

#[test]
fn circular_dep_solutions_suggests_disabling_the_gating_flag() {
    // USE-gated build cycle: exactly "disable x on usecyclea".
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let result = graph_result_real("dev-libs/usecyclea");
    assert_eq!(result.circular_deps.len(), 1);
    let sols = circular_dep_solutions(
        &result.circular_deps[0],
        &repos,
        &config,
        &result.autounmask_use_changes,
        &result.entries,
    );
    assert_eq!(
        sols,
        vec![CircularSuggestion {
            parent_cpv: "dev-libs/usecyclea-1.0".to_string(),
            changes: vec![("x".to_string(), false)],
            followup: false,
        }]
    );
}

#[test]
fn circular_dep_solutions_applies_the_autounmask_use_overlay() {
    // Portage evaluates the suggestion on the autounmasked USE while `bar` stays
    // untouchable, so the only fix is `foo` off.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let result = graph_result_autounmask("=dev-libs/cyc0w-3");
    assert_eq!(result.circular_deps.len(), 1);
    assert!(
        result
            .autounmask_use_changes
            .iter()
            .any(|ch| ch.token.split_whitespace().any(|t| t == "bar")),
        "expected an autounmask `bar` change, got {:?}",
        result.autounmask_use_changes
    );
    let sols = circular_dep_solutions(
        &result.circular_deps[0],
        &repos,
        &config,
        &result.autounmask_use_changes,
        &result.entries,
    );
    assert_eq!(
        sols,
        vec![CircularSuggestion {
            parent_cpv: "dev-libs/cyc0z-3".to_string(),
            changes: vec![("foo".to_string(), false)],
            followup: false,
        }]
    );
}

#[test]
fn circular_dep_solutions_grandparent_conflict_disqualifies_the_suggestion() {
    // Grandparent with a hard use-dep on the fix: the suggestion list is empty.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let result = graph_result_real("dev-libs/gpcyclec");
    assert_eq!(result.circular_deps.len(), 1);
    let sols = circular_dep_solutions(
        &result.circular_deps[0],
        &repos,
        &config,
        &result.autounmask_use_changes,
        &result.entries,
    );
    assert!(sols.is_empty(), "{:?}", sols);
}

#[test]
fn mixed_priority_cycle_reports_per_edge_labels() {
    // The `:=` edge is unbreakable at every rung, so Portage aborts; rotation
    // starts at the earliest-inserted node.
    let result = graph_result_real("dev-libs/slopcyca");
    assert_eq!(
        result.circular_deps,
        vec![vec![
            "dev-libs/slopcycb-1.0".to_string(),
            "dev-libs/slopcyca-1.0".to_string()
        ]]
    );
    assert_eq!(
        cycle_edge_labels(&result.entries, &result.circular_deps[0]),
        vec!["buildtime".to_string(), "runtime_slot_op".to_string()]
    );
    // All-buildtime fixtures keep the old rendering.
    let hard = graph_result_real("dev-libs/hardcyclea");
    assert_eq!(
        cycle_edge_labels(&hard.entries, &hard.circular_deps[0]),
        vec!["buildtime".to_string(), "buildtime".to_string()]
    );
}

#[test]
fn circular_dep_solutions_is_empty_for_a_cycle_with_no_conditional_edge() {
    // Unconditional DEPENDs, no IUSE: no suggestion (the generic advisory).
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let result = graph_result_real("dev-libs/hardcyclea");
    assert!(
        circular_dep_solutions(
            &result.circular_deps[0],
            &repos,
            &config,
            &result.autounmask_use_changes,
            &result.entries,
        )
        .is_empty()
    );
}
