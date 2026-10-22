use super::*;

#[test]
fn topological_order_breaks_a_cycle_at_its_run_time_edge() {
    // a's unsatisfied DEPEND names b, b's RDEPEND names a: the cycle breaks at
    // the run-time edge, so b merges first (a must build against it).
    let dep = |pkg: &str, priority: DepPriority| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}").clone(),
        category: "dev-libs".to_string(),
        package: pkg.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: if priority.buildtime { 3 } else { 0 },
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
    let mut b = graph_entry("dev-libs", "cyc-b", "1.0");
    a.deps = vec![dep("cyc-b", buildtime)];
    b.deps = vec![dep("cyc-a", runtime)];
    a.required_by = vec![("dev-libs".to_string(), "cyc-b".to_string())];
    b.required_by = vec![("dev-libs".to_string(), "cyc-a".to_string())];
    let (ordered, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: vec![a, b],
        top_level_atoms: &["dev-libs/cyc-a".to_string()],
        config: &test_config(),
        root: Path::new("/nonexistent-root"),
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    let names: Vec<&str> = ordered.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["cyc-b", "cyc-a"]);

    // All-buildtime cycle: unbreakable, falls back to bias order.
    let mut a = graph_entry("dev-libs", "cyc-a", "1.0");
    let mut b = graph_entry("dev-libs", "cyc-b", "1.0");
    a.deps = vec![dep("cyc-b", buildtime)];
    b.deps = vec![dep("cyc-a", buildtime)];
    a.required_by = vec![("dev-libs".to_string(), "cyc-b".to_string())];
    b.required_by = vec![("dev-libs".to_string(), "cyc-a".to_string())];
    let (ordered, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: vec![a, b],
        top_level_atoms: &["dev-libs/cyc-a".to_string()],
        config: &test_config(),
        root: Path::new("/nonexistent-root"),
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    let names: Vec<&str> = ordered.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["cyc-a", "cyc-b"]);
}

#[test]
fn topological_merge_order_stamps_discovery_before_sorting() {
    // `discovery` is the pre-sort BFS position: ranks track input slots.
    let dep = |pkg: &str, priority: DepPriority| DepEdge {
        atom: format!("dev-libs/{pkg}"),
        evaluated: format!("dev-libs/{pkg}").clone(),
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
    let mut a = graph_entry("dev-libs", "a", "1.0");
    let b = graph_entry("dev-libs", "b", "1.0");
    a.deps = vec![dep("b", runtime)];
    let (ordered, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: vec![a, b],
        top_level_atoms: &["dev-libs/a".to_string()],
        config: &test_config(),
        root: Path::new("/nonexistent-root"),
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    let ranks: Vec<(&str, usize)> = ordered
        .iter()
        .map(|e| (e.package.as_str(), e.discovery))
        .collect();
    assert_eq!(ranks, vec![("b", 1), ("a", 0)]);
}

#[test]
fn implicit_system_deps_n_skips_the_merge_order_bias() {
    // Leaves a, b, c with c in @system: the bias promotes c first, while
    // `--implicit-system-deps=n` keeps discovery order.
    let mut config = test_config();
    config.system_packages = vec!["dev-libs/c".to_string()];
    let atoms = vec![
        "dev-libs/a".to_string(),
        "dev-libs/b".to_string(),
        "dev-libs/c".to_string(),
    ];
    let (biased, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: vec![
            graph_entry("dev-libs", "a", "1.0"),
            graph_entry("dev-libs", "b", "1.0"),
            graph_entry("dev-libs", "c", "1.0"),
        ],
        top_level_atoms: &atoms,
        config: &config,
        root: Path::new("/nonexistent-root"),
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    let names: Vec<&str> = biased.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["c", "a", "b"]);
    let (unbiased, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: vec![
            graph_entry("dev-libs", "a", "1.0"),
            graph_entry("dev-libs", "b", "1.0"),
            graph_entry("dev-libs", "c", "1.0"),
        ],
        top_level_atoms: &atoms,
        config: &config,
        root: Path::new("/nonexistent-root"),
        implicit_system_deps: false,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    let names: Vec<&str> = unbiased.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

/// `skipped_update_roots` attributes the miss to the root the rejecting edge
/// points at, and a merge parent to its entry's root.
#[test]
fn skipped_update_roots_attributes_miss_and_parent_per_edge() {
    fn entry(package: &str, version: &str, running: bool, deps: Vec<DepEdge>) -> GraphEntry {
        GraphEntry {
            discovery: 0,
            category: "dev-libs".to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: Some("testrepo".to_string()),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: CandidateSource::Ebuild,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: running,
            remote_binary: false,
            build_id: None,
            build_time: None,
            deps,
        }
    }
    fn edge(child: &str, atom: &str, key: u8) -> DepEdge {
        DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string(),
            category: "dev-libs".to_string(),
            package: child.to_string(),
            priority: DepPriority::default(),
            disjunctive: false,
            alt: None,
            key,
        }
    }
    // Cross-active: target parent plus a running entry elsewhere.
    let running_twin = entry("blk0x", "1", true, Vec::new());
    let parent = entry(
        "blk0b",
        "1",
        false,
        vec![edge("blk0x", "<dev-libs/blk0x-2", 3)],
    );
    let entries = vec![running_twin, parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "<dev-libs/blk0x-2",
        ),
        (true, false),
        "DEPEND-rejected miss lands running, target parent stays target",
    );
    // Same edges, single-rooted: everything attributes target.
    let parent = entry(
        "blk0b",
        "1",
        false,
        vec![edge("blk0x", "<dev-libs/blk0x-2", 3)],
    );
    let entries = vec![parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "<dev-libs/blk0x-2",
        ),
        (false, false),
        "single-root runs keep the target attribution",
    );
    // A runtime edge keeps the miss in the parent's (running) root.
    let parent = entry("blk0b", "1", true, vec![edge("blk0x", "dev-libs/blk0x", 0)]);
    let entries = vec![parent];
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "blk0b", "1"),
            "dev-libs/blk0x",
        ),
        (true, true),
        "RDEPEND keeps the miss in the parent's (running) root",
    );
    // Unknown parent: both target.
    assert_eq!(
        skipped_update_roots(
            &entries,
            ("dev-libs", "blk0x"),
            ("dev-libs", "nope", "1"),
            "x",
        ),
        (false, false),
    );
}
