use super::*;

#[test]
fn resolved_dep_targets_reports_an_installed_target_as_no_tree_edge() {
    // The one `merge_bound_only` difference: the tree view drops the
    // installed edge, the scheduling graph keeps it.
    let dep = DepEdge {
        atom: "dev-libs/onlyinstalled".to_string(),
        evaluated: "dev-libs/onlyinstalled".to_string().clone(),
        category: "dev-libs".to_string(),
        package: "onlyinstalled".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut installed = new_entry("dev-libs", "onlyinstalled", "1.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![new_entry("dev-libs", "user", "1.0", vec![dep]), installed];
    let root = Path::new("/nonexistent-root-for-unit-test");
    assert_eq!(
        resolved_dep_targets(&entries, root, &HashMap::new())[0][0],
        None
    );
    let g = build_digraph(
        &entries,
        &["dev-libs/user".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[0].iter().map(|&(c, _)| c).collect();
    assert_eq!(children, vec![1], "the scheduling graph keeps the edge");
}
