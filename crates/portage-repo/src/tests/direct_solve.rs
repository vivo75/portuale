use super::*;

#[test]
fn direct_solve_removes_the_unforced_merge_and_reports_the_skip() {
    // Old/new shape: `<2.0` forces 1.0, so the bare tuple drops 2.0 with a skip
    // row (Portage's staged-oracle verdict).
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
        ),
        (
            "1.0",
            false,
            vec![(old_cpv, "<dev-libs/slotconflicttarget-2.0", false)],
        ),
    ])];
    let entries = s2_entries(vec!["2.0", "1.0"]);
    let top: HashSet<(String, String)> = HashSet::from([
        (
            "dev-libs".to_string(),
            "slotconflictnewconsumer".to_string(),
        ),
        (
            "dev-libs".to_string(),
            "slotconflictoldconsumer".to_string(),
        ),
    ]);
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert_eq!(out.surviving, vec![]);
    assert_eq!(out.skipped.len(), 1);
    let s = &out.skipped[0];
    assert_eq!(s.skipped_version, "2.0");
    assert_eq!(s.atom, "<dev-libs/slotconflicttarget-2.0");
    assert_eq!(s.consumer_cpv, old_cpv);
    assert!(!s.consumer_installed);
}

#[test]
fn direct_solve_reports_an_argument_parent_with_the_cli_text() {
    // Portage prints the CLI argument bare (no atom, no marker); the direct-solve
    // producer supplies it via an ownerless depth-0 puller.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // Versioned argument pinning the kept instance rejects 2.0 the same way.
    let arg = "=dev-libs/slotconflicttarget-1.0";
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
        ),
        (
            "1.0",
            false,
            vec![
                (old_cpv, "<dev-libs/slotconflicttarget-2.0", false),
                ("", arg, false),
            ],
        ),
    ])];
    let entries = s2_entries(vec!["2.0", "1.0"]);
    let top: HashSet<(String, String)> = HashSet::from([
        (
            "dev-libs".to_string(),
            "slotconflictnewconsumer".to_string(),
        ),
        (
            "dev-libs".to_string(),
            "slotconflictoldconsumer".to_string(),
        ),
    ]);
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert_eq!(out.skipped.len(), 2);
    let arg_row = out
        .skipped
        .iter()
        .find(|s| s.consumer_cpv.is_empty())
        .expect("the argument parent rides out as a bare row");
    assert_eq!(arg_row.skipped_version, "2.0");
    assert_eq!(arg_row.atom, arg);
    assert!(!arg_row.consumer_installed);
}

#[test]
fn direct_solve_keeps_an_unsolvable_conflict_untouched() {
    // Both instances forced: the conflict survives with no skip rows.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewpin-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldpin-1.0:0/0::testrepo";
    let conflicts = [s2_conflict(vec![
        (
            "1.0",
            false,
            vec![(old_cpv, "<dev-libs/slotconflicttarget-2.0", false)],
        ),
        (
            "2.0",
            false,
            vec![(new_cpv, ">=dev-libs/slotconflicttarget-2.0", false)],
        ),
    ])];
    let entries = s2_entries(vec!["1.0", "2.0"]);
    let top: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "slotconflictunsolvable".to_string())]);
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
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
    assert_eq!(out.surviving[0].instances.len(), 2);
}

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

#[test]
fn direct_solve_arg_parents_protect_the_triangle() {
    // Installed 1.0 excluded from argv parents' matches, so the conflict is
    // protected (staged oracle, both argv orders).
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [SlotConflict {
        category: "dev-libs".to_string(),
        package: "paired".to_string(),
        slot: "0".to_string(),
        resolved_version: "1.0".to_string(),
        conflicting_atom: "<dev-libs/paired-2.0".to_string(),
        instances: vec![
            SlotConflictInstance {
                version: "1.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: vec![SlotConflictParent {
                    parent_cpv: "dev-libs/othermod-1.0:0/0::testrepo".to_string(),
                    atom: "<dev-libs/paired-2.0".to_string(),
                    use_display: Vec::new(),
                    installed: false,
                }],
                installed: true,
            },
            SlotConflictInstance {
                version: "2.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: vec![SlotConflictParent {
                    parent_cpv: "dev-libs/needer-1.0:0/0::testrepo".to_string(),
                    atom: ">=dev-libs/paired-2.0".to_string(),
                    use_display: Vec::new(),
                    installed: false,
                }],
                installed: false,
            },
        ],
    }];
    let entries = vec![graph_entry("dev-libs", "paired", "2.0")];
    let top: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "needer".to_string()),
        ("dev-libs".to_string(), "othermod".to_string()),
    ]);
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
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// Installed instances rank after merge instances; an installed survivor is kept.
#[test]
fn direct_solve_orders_merge_instances_before_installed() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // 1.0 installed and no arg names the conflict: normal matching.
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
        ),
        (
            "1.0",
            true,
            vec![(old_cpv, "<dev-libs/slotconflicttarget-2.0", false)],
        ),
    ])];
    // Walk order names 1.0 first; merge must still outrank installed.
    let entries = s2_entries(vec!["1.0", "2.0"]);
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// Tuple order follows walk order.
#[test]
fn direct_solve_tuple_order_follows_walk_order() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // Both pull bare: walk order decides.
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
        ),
        (
            "1.0",
            false,
            vec![(old_cpv, "dev-libs/slotconflicttarget", false)],
        ),
    ])];
    let entries = s2_entries(vec!["1.0", "2.0"]);
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// Outside parents draw package edges, never instance edges.
#[test]
fn direct_solve_ignores_parents_outside_the_conflict() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, top) = ds_s0_161();
    let mut conflict = conflicts[0].clone();
    conflict.instances[0].parents.push(SlotConflictParent {
        parent_cpv: "dev-libs/otherpkg-1.0:0/0::testrepo".to_string(),
        atom: "dev-libs/slotconflicttarget".to_string(),
        use_display: Vec::new(),
        installed: false,
    });
    let conflicts = [conflict];
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
    // The outside parent changes no edge that matters.
    assert_eq!(
        out.removed,
        vec![(
            "dev-libs".to_string(),
            "slotconflicttarget".to_string(),
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// A non-built parent atom never arms the rebuild gate.
#[test]
fn direct_solve_solves_past_a_non_built_parent() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, top) = ds_s0_161();
    let mut conflict = conflicts[0].clone();
    conflict.instances[0].parents.push(SlotConflictParent {
        parent_cpv: "dev-libs/paired-1.0:0/0::testrepo".to_string(),
        atom: "dev-libs/slotconflicttarget:0".to_string(),
        use_display: Vec::new(),
        installed: false,
    });
    let conflicts = [conflict];
    let excluded = vec!["dev-libs/paired".to_string()];
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(ds_input_161(DsInput161Args {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        excluded: &excluded,
        selective: false,
        replace_cps: &empty_replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    // Slot-only atom: the gate stays shut.
    assert_eq!(
        out.removed,
        vec![(
            "dev-libs".to_string(),
            "slotconflicttarget".to_string(),
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// The rebuild gate needs the parent installed at the pulled version.
#[test]
fn direct_solve_solves_past_an_uninstalled_parent() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, top) = ds_s0_161();
    let mut conflict = conflicts[0].clone();
    // `paired-9.9` is not installed, so the gate stays shut either way.
    conflict.instances[0].parents.push(SlotConflictParent {
        parent_cpv: "dev-libs/paired-9.9:0/0::testrepo".to_string(),
        atom: "dev-libs/slotconflicttarget:0/0=".to_string(),
        use_display: Vec::new(),
        installed: false,
    });
    let conflicts = [conflict];
    let excluded = vec!["dev-libs/paired".to_string()];
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(ds_input_161(DsInput161Args {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        excluded: &excluded,
        selective: false,
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// `--exclude` alone arms the rebuild gate.
#[test]
fn direct_solve_exclude_alone_arms_the_rebuild_gate() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, top) = ds_s0_161();
    let mut conflict = conflicts[0].clone();
    conflict.instances[0].parents.push(SlotConflictParent {
        parent_cpv: "dev-libs/paired-1.0:0/0::testrepo".to_string(),
        atom: "dev-libs/slotconflicttarget:0/0=".to_string(),
        use_display: Vec::new(),
        installed: false,
    });
    let conflicts = [conflict];
    let excluded = vec!["dev-libs/paired".to_string()];
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(ds_input_161(DsInput161Args {
        conflicts: &conflicts,
        entries: &entries,
        top_level: &top,
        excluded: &excluded,
        selective: false,
        replace_cps: &empty_replace,
        root: &root,
        repos: &repos,
        config: &config,
    }));
    // Excluded built-`=` parent is rebuild residue: the conflict survives.
    assert!(out.removed.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// The replace set holds the conflict without solving.
#[test]
fn direct_solve_replace_set_holds_the_conflict() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, _) = ds_s0_161();
    // No args: only the replace-set gate holds it.
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
    assert_eq!(out.surviving.len(), 1);
}

/// A lone bare parent solves through the tuple path.
#[test]
fn direct_solve_lone_bare_parent_solves_through_the_tuple() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(new_cpv, "dev-libs/slotconflicttarget", false)],
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

/// A lone outside parent still solves through the tuple path.
#[test]
fn direct_solve_lone_outside_parent_solves_through_the_tuple() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(
                "dev-libs/otherpkg-1.0:0/0::testrepo",
                "dev-libs/slotconflicttarget",
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

/// Argless forcing still drops the unforced merge.
#[test]
fn direct_solve_argless_forcing_drops_the_unforced_merge() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, _) = ds_s0_161();
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
            "2.0".to_string(),
            "1.0".to_string()
        )]
    );
    assert!(out.surviving.is_empty());
}

/// An unresolvable parent falls back to the shared sink.
#[test]
fn direct_solve_unresolvable_parent_falls_back_to_the_sink() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let conflicts = [s2_conflict(vec![
        (
            "2.0",
            false,
            vec![(
                "dev-libs/slotconflicttarget-9.9:0/0::testrepo",
                "dev-libs/slotconflicttarget",
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
