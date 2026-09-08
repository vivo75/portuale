use super::*;

#[test]
fn direct_solve_skips_slot_operator_rebuild_conflicts() {
    // Rebuild conflicts never reach the direct solve (Portage).
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [s2_conflict(vec![
        (
            "1.0",
            false,
            vec![(
                "dev-libs/slotconflictoldpin-1.0:0/0::testrepo",
                "<dev-libs/slotconflicttarget-2.0",
                false,
            )],
        ),
        (
            "2.0",
            false,
            vec![(
                "dev-libs/slotconflictnewpin-1.0:0/0::testrepo",
                ">=dev-libs/slotconflicttarget-2.0",
                false,
            )],
        ),
    ])];
    let entries = s2_entries(vec!["1.0", "2.0"]);
    let top: HashSet<(String, String)> = HashSet::new();
    let replace: BTreeSet<(String, String)> =
        BTreeSet::from([("dev-libs".to_string(), "slotconflicttarget".to_string())]);
    let out = direct_solve_slot_conflicts(s2_input(S2InputArgs {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        replace_cps: &replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// Installed ranks last over three versions with only one walked.
#[test]
fn direct_solve_ambiguous_conflict_prefers_the_walked_instance() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let parent = |cpv: &str| (cpv.to_string(), "dev-libs/mgfc".to_string(), false);
    let inst = |version: &str, sub: &str, installed: bool, parents: Vec<(String, String, bool)>| {
        SlotConflictInstance {
            version: version.to_string(),
            sub_slot: sub.to_string(),
            repo_name: "testrepo".to_string(),
            use_display: Vec::new(),
            parents: parents
                .into_iter()
                .map(|(cpv, atom, pinst)| SlotConflictParent {
                    parent_cpv: cpv,
                    atom,
                    use_display: Vec::new(),
                    installed: pinst,
                })
                .collect(),
            installed,
        }
    };
    let conflicts = [SlotConflict {
        category: "dev-libs".to_string(),
        package: "mgfc".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: "dev-libs/mgfc".to_string(),
        instances: vec![
            inst(
                "1",
                "0",
                true,
                vec![parent("dev-libs/otherpkg-1.0:0/0::testrepo")],
            ),
            inst(
                "2",
                "0",
                false,
                vec![parent("dev-libs/otherpkg-1.0:0/0::testrepo")],
            ),
            inst(
                "3.0",
                "0",
                false,
                vec![parent("dev-libs/otherpkg-1.0:0/0::testrepo")],
            ),
        ],
    }];
    // Only 2 walked: 2 first, installed 1 second, unwalked 3.0 last.
    let entries = vec![GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "mgfc".into(),
        outcome: PretendOutcome::New {
            version: "2".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "mgfc", "2")
    }];
    let top: HashSet<(String, String)> = HashSet::new();
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(s2_input(S2InputArgs {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        replace_cps: &empty_replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    // 3.0 drops against the installed keeper; the 1-vs-2 residual survives.
    assert_eq!(
        out.removed,
        vec![(
            "dev-libs".to_string(),
            "mgfc".to_string(),
            "3.0".to_string(),
            "1".to_string()
        )]
    );
    assert_eq!(out.surviving.len(), 1);
}

/// A mismatched installed flag falls back to the sink too.
#[test]
fn direct_solve_mismatched_parent_installedness_falls_back_to_the_sink() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(
                "dev-libs/slotconflicttarget-2.0:0/0::testrepo",
                "dev-libs/slotconflicttarget",
                true,
            )],
        ),
        ("1.0", false, vec![]),
    ])];
    let entries = s2_entries(vec!["2.0", "1.0"]);
    let top: HashSet<(String, String)> = HashSet::new();
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(s2_input(S2InputArgs {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        replace_cps: &empty_replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    assert_eq!(
        out.removed,
        vec![(
            "dev-libs".to_string(),
            "slotconflicttarget".to_string(),
            "1.0".to_string(),
            "2.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// An unreached parent edge deletes quietly.
#[test]
fn direct_solve_unreached_parent_edge_deletes_quietly() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(
                "dev-libs/slotconflicttarget-1.0:0/0::testrepo",
                "<dev-libs/slotconflicttarget-2.0",
                false,
            )],
        ),
        ("1.0", false, vec![]),
    ])];
    let entries = s2_entries(vec!["2.0", "1.0"]);
    let top: HashSet<(String, String)> = HashSet::new();
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(s2_input(S2InputArgs {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        replace_cps: &empty_replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    // Both drop with no keeper: the edge hangs off an unreached node.
    assert_eq!(
        out.removed,
        vec![
            (
                "dev-libs".to_string(),
                "slotconflicttarget".to_string(),
                "2.0".to_string(),
                String::new(),
            ),
            (
                "dev-libs".to_string(),
                "slotconflicttarget".to_string(),
                "1.0".to_string(),
                String::new(),
            ),
        ]
    );
    assert!(out.surviving.is_empty());
}
