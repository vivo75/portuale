use super::*;

#[test]
fn direct_solve_removes_the_unforced_merge_and_reports_the_skip() {
    // The 2-atom old/new shape (S0 matrix): `oldconsumer`'s `<2.0`
    // singles out 1.0 while `newconsumer`'s bare atom matches both
    // (an `or_tuple`); 1.0 is forced through the old edge, so the
    // tuple is satisfied without 2.0 and 2.0 drops with a skip row
    // against oldconsumer's pin -- real's `remove: 2.0 /
    // keep: 1.0` verdict and `WARNING` text (staged oracle).
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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
    // Backlog #230 fix round 1 (review Important 3): real's
    // `PackageArg`/`AtomArg` arm (`depgraph.py:1681-1686`) prints
    // `str(parent)` -- the CLI argument -- with no atom and no `^`
    // marker. The direct-solve producer is the one path that can
    // supply it: a top-level argument puller is recorded with
    // empty parent fields and the argument text as its atom (see
    // `slot_pullers`' depth-0 ownerless push), so when that atom
    // rejects the removed instance the skip row carries the CLI
    // text in `atom` with an empty `consumer_cpv`, and the
    // renderer printing `s.atom` bare reproduces real's arm
    // exactly. (The reverse-pin producer always builds a non-empty
    // consumer cpv; the backtrack-mask rows skip empty-category
    // pullers outright.) No contract or bed cell grounds an
    // Argument parent end to end (blk0 parents are all Packages),
    // so this producer-level pin is the only coverage -- the
    // renderer arm beyond it stays unpinned.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // A versioned CLI argument pinning the kept instance: it
    // rejects the removed 2.0 exactly like oldconsumer's pin.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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
    // Both instances singly forced (newpin's `>=2.0`, oldpin's
    // `<2.0`): nothing is unforced, the conflict survives with no
    // skip rows -- rc 1, like real.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
    assert_eq!(out.surviving[0].instances.len(), 2);
}

#[test]
fn direct_solve_skips_slot_operator_rebuild_conflicts() {
    // Same shape as the unsolvable cell, but keyed in
    // `slot_operator_replace_installed`: real never feeds those to
    // the direct solve (`:1784-1788`) -- #24 territory, not #90.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts, &entries, &top, &replace, &root, &repos, &config,
    ));
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

#[test]
fn direct_solve_arg_parents_protect_the_triangle() {
    // The #57 triangle: the installed 1.0 is excluded from every
    // argv parent's match list (`is_arg_parent`: top-level pull
    // with an update wanted), so `othermod`'s `<2.0` matches
    // nothing and protects the whole conflict -- rc 1, like real
    // (staged oracle both argv orders).
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    assert!(out.removed.is_empty());
    assert!(out.skipped.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// Backlog #161 S5: an installed instance ranks after every merge
/// instance for the `or_tuple` first-pulled rule, and an installed
/// survivor is always kept.
#[test]
fn direct_solve_orders_merge_instances_before_installed() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // Same pulls as the S0 shape, but 1.0 is installed and no
    // top-level arg names the conflict (so `arg_mode` stays off and
    // the installed instance matches normally).
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
    // Walk order names 1.0 first: the merge instance must still
    // outrank the installed one.
    let entries = s2_entries(vec!["1.0", "2.0"]);
    let top: HashSet<(String, String)> = HashSet::new();
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: tuple order follows walk order -- with 1.0
/// walked first, an ambiguous bare/bare conflict drops 2.0.
#[test]
fn direct_solve_tuple_order_follows_walk_order() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let new_cpv = "dev-libs/slotconflictnewconsumer-1.0:0/0::testrepo";
    let old_cpv = "dev-libs/slotconflictoldconsumer-1.0:0/0::testrepo";
    // Both parents pull bare: nothing forces either version, so the
    // walk order decides.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: a parent naming a package outside the conflict
/// draws a package-to-package edge, never an instance edge.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    // The S0 verdict stands: the outside parent changes no edge that
    // matters.
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

/// Backlog #161 S5: a non-built parent atom never arms the
/// slot-operator-rebuild gate, so the solve proceeds past it.
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
    let out = direct_solve_slot_conflicts(ds_input_161(
        &conflicts,
        &entries,
        &top,
        &excluded,
        false,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    // A slot-only atom is not a built op: the gate stays shut and
    // the S0 verdict stands.
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

/// Backlog #161 S5: the rebuild gate needs the parent installed at
/// the pulled version -- and the pulled version installed, not just
/// some other version of the same cp.
#[test]
fn direct_solve_solves_past_an_uninstalled_parent() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, top) = ds_s0_161();
    let mut conflict = conflicts[0].clone();
    // `paired-9.9` is not installed (the fixture vdb holds
    // `paired-1.0`), so the gate stays shut either way.
    conflict.instances[0].parents.push(SlotConflictParent {
        parent_cpv: "dev-libs/paired-9.9:0/0::testrepo".to_string(),
        atom: "dev-libs/slotconflicttarget:0/0=".to_string(),
        use_display: Vec::new(),
        installed: false,
    });
    let conflicts = [conflict];
    let excluded = vec!["dev-libs/paired".to_string()];
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(ds_input_161(
        &conflicts,
        &entries,
        &top,
        &excluded,
        false,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: `--exclude` alone arms the rebuild gate -- the
/// `useoldpkg` alternative need not match too.
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
    let out = direct_solve_slot_conflicts(ds_input_161(
        &conflicts,
        &entries,
        &top,
        &excluded,
        false,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    // The installed `paired-1.0` parent with a built `=` atom is
    // `--exclude`d: real's rebuild-path residue, not remove-path
    // material -- the conflict survives.
    assert!(out.removed.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// Backlog #161 S5: naming the conflict in the replace set holds it
/// without solving, even when no rebuild parent exists.
#[test]
fn direct_solve_replace_set_holds_the_conflict() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, _) = ds_s0_161();
    // No top-level args: nothing else in the solve can protect the
    // conflict, so only the replace-set gate holds it.
    let top: HashSet<(String, String)> = HashSet::new();
    let replace: BTreeSet<(String, String)> =
        BTreeSet::from([("dev-libs".to_string(), "slotconflicttarget".to_string())]);
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts, &entries, &top, &replace, &root, &repos, &config,
    ));
    assert!(out.removed.is_empty());
    assert_eq!(out.surviving.len(), 1);
}

/// Backlog #161 S5: a lone bare parent over two instances solves
/// through the tuple path, dropping the later-walked instance.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: installed instances rank after every merge
/// instance -- over three `mgfc` versions with only 2.0 walked,
/// the two unforced merges drop in rank order behind it.
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
    // Only 2 is walked: 2 ranks first, the installed 1
    // second, the unwalked 3.0 last.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    // 2 wins the tuple; 3.0 drops against the installed keeper.
    // The 1-vs-2 residual survives for the mask trial.
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

/// Backlog #161 S5: a lone outside parent still solves through the
/// tuple path -- misattributing it to an instance must change the
/// verdict, not pass silently.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: without top-level args the S0 forcing still
/// drops 2.0 -- the verdict must not depend on which path (tuple
/// or direct edge) each parent takes.
#[test]
fn direct_solve_argless_forcing_drops_the_unforced_merge() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = test_config();
    let (conflicts, entries, _) = ds_s0_161();
    let top: HashSet<(String, String)> = HashSet::new();
    let empty_replace: BTreeSet<(String, String)> = BTreeSet::new();
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: a parent triple naming no pulled version --
/// here a version that does not exist -- falls back to the shared
/// sink instead of hijacking an instance edge.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: a parent triple whose installed flag matches no
/// instance -- a merge instance pulled "as installed" -- falls back
/// to the shared sink just the same.
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
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

/// Backlog #161 S5: a single-match edge from an instance node no
/// traversal reaches deletes quietly -- the degenerate shape the
/// take-over documents (no edge reached anything, no keeper owed).
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
    let out = direct_solve_slot_conflicts(s2_input(
        &conflicts,
        &entries,
        &top,
        &empty_replace,
        &root,
        &repos,
        &config,
    ));
    // Both instances drop with no keeper: the only edge hangs off
    // an instance node the walk never reaches.
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
