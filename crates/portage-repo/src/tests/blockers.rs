use super::*;

#[test]
fn resolve_blockers_matches_a_graph_resolved_package_with_no_installed_candidates() {
    let entries = vec![
        graph_entry("dev-libs", "owner", "1.0"),
        graph_entry("dev-libs", "target", "2.0"),
    ];
    let pending = vec![PendingBlocker {
        atom_str: "!!dev-libs/target".to_string(),
        strong: true,
        buildtime: true,
        target_category: "dev-libs".to_string(),
        target_package: "target".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(
        conflicts,
        vec![FiledBlocker {
            owner: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
            conflict: BlockerConflict {
                atom_str: "!!dev-libs/target".to_string(),
                strong: true,
                matched_category: "dev-libs".to_string(),
                matched_package: "target".to_string(),
                matched_version: "2.0".to_string(),
                // Merge-bound match with a merging parent: unresolved outright.
                unsolvable: true,
                satisfied_by: None,
                tree_scheduled_uninstall: false,
            },
        }]
    );
}

#[test]
fn resolve_blockers_skips_a_blocker_matching_its_own_owner() {
    let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
    let pending = vec![PendingBlocker {
        atom_str: "!dev-libs/owner".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "owner".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
}

#[test]
fn resolve_blockers_drops_a_use_dep_blocker_the_target_does_not_satisfy() {
    // Unsatisfied use-dep: the block is a satisfied no-op (Portage omits the line).
    let mut target = graph_entry("dev-libs", "target", "2.0");
    target.use_flags_display = vec![("wantblock".to_string(), false)];
    let entries = vec![graph_entry("dev-libs", "owner", "1.0"), target];
    let call = |atom: &str| {
        resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &[PendingBlocker {
                atom_str: atom.to_string(),
                strong: true,
                buildtime: true,
                target_category: "dev-libs".to_string(),
                target_package: "target".to_string(),
                owner_key: ("dev-libs".to_string(), "owner".to_string()),
                owner_version: "1.0".to_string(),
                owner_merging: true,
                owner_installed: false,
            }],
            &entries,
            &HashSet::new(),
            false,
        )
    };
    assert!(call("!!dev-libs/target[wantblock]").is_empty());
    // `(+)` default stands in as enabled, so `-gone` is unmet.
    assert!(call("!!dev-libs/target[-gone(+)]").is_empty());
    // A plain blocker still fires, as does a satisfied use-dep.
    assert_eq!(call("!!dev-libs/target").len(), 1);
    assert_eq!(call("!!dev-libs/target[-wantblock]").len(), 1);
}

#[test]
fn resolve_blockers_keeps_an_installed_match_replaced_in_its_slot_as_satisfied() {
    // Installed match replaced in-slot: the row is kept as `Replacement`.
    let dir = slotundo_temp_dir("blocker-s1-a");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "",
    });
    let owner = graph_entry("dev-libs", "bparent", "1.0");
    let mut upgrade = graph_entry("dev-libs", "blocked", "2.0");
    upgrade.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "2.0".into(),
    };
    let pending = PendingBlocker {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), "bparent".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    let conflicts = resolve_blockers(&dir, &[pending], &[owner, upgrade], &HashSet::new(), false);
    assert_eq!(
        conflicts.len(),
        1,
        "a replaced-in-slot installed match keeps its row"
    );
    assert!(!conflicts[0].conflict.unsolvable);
    assert_eq!(
        conflicts[0].conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Replacement {
            cp: ("dev-libs".to_string(), "blocked".to_string()),
            slot: "0".to_string(),
        })
    );

    // Cell a': different-slot replacement uninstalls the installed match instead.
    let owner = graph_entry("dev-libs", "bparent", "1.0");
    let mut other_slot = graph_entry("dev-libs", "blocked", "2.0");
    other_slot.slot = Some("1".to_string());
    other_slot.sub_slot = Some("1".to_string());
    let pending = PendingBlocker {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), "bparent".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    let conflicts = resolve_blockers(
        &dir,
        &[pending],
        &[owner, other_slot],
        &HashSet::new(),
        false,
    );
    assert_eq!(
        conflicts.len(),
        1,
        "a different-slot replacement keeps the row"
    );
    assert!(!conflicts[0].conflict.unsolvable);
    assert_eq!(
        conflicts[0].conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall {
            cpv: "dev-libs/blocked-1.0".to_string(),
            anchor: ("dev-libs".to_string(), "bparent".to_string()),
        }),
        "a different-slot installed match is removed (#72 B3)"
    );

    // Cell b: a same-version reinstall is merge-bound, never "replaced".
    let owner = graph_entry("dev-libs", "bparent", "1.0");
    let mut reinstall = graph_entry("dev-libs", "blocked", "1.0");
    reinstall.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let pending = PendingBlocker {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), "bparent".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    let conflicts = resolve_blockers(
        &dir,
        &[pending],
        &[owner, reinstall],
        &HashSet::new(),
        false,
    );
    assert_eq!(
        conflicts.len(),
        1,
        "a same-version reinstall is merge-bound"
    );
    assert!(conflicts[0].conflict.unsolvable);
    assert_eq!(conflicts[0].conflict.satisfied_by, None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unwalked_scan_skips_without_merge_bound_entries_and_blocker_free_owners() {
    // Two cheap guards: nothing merge-bound means a no-op scan, and a blocker-free
    // owner never reaches USE reduction.
    let dir = slotundo_temp_dir("blocker-83-scan");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "bparent",
        version: "1.0",
        slot: "0",
        rdepend: "!<dev-libs/blocked-2.0",
        installed_use: "",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "plain",
        version: "1.0",
        slot: "0",
        rdepend: "dev-libs/blocked",
        installed_use: "",
    });
    let config = portage_profile::Config::default();
    let constraints: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut memo: HashMap<(String, String, String, String), String> = HashMap::new();

    // (a): installed-only entry, so the scan returns before the vdb.
    let mut installed = graph_entry("dev-libs", "blocked", "1.0");
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let mut pending = Vec::new();
    collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
        repos: &[],
        root: &dir,
        config: &config,
        dynamic_deps: false,
        ignore_built_slot_operator_deps: false,
        disj_constraints: &constraints,
        root_deps_running_root: None,
        entries: &[installed],
        pending_blockers: &mut pending,
        installed_meta_memo: &mut memo,
    });
    assert!(
        pending.is_empty(),
        "nothing merge-bound means nothing to satisfy"
    );

    // (b): only the blocker-recorded owner is collected.
    let mut upgrade = graph_entry("dev-libs", "blocked", "1.5");
    upgrade.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "1.5".into(),
    };
    let mut pending = Vec::new();
    collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
        repos: &[],
        root: &dir,
        config: &config,
        dynamic_deps: false,
        ignore_built_slot_operator_deps: false,
        disj_constraints: &constraints,
        root_deps_running_root: None,
        entries: &[upgrade],
        pending_blockers: &mut pending,
        installed_meta_memo: &mut memo,
    });
    assert_eq!(
        pending.len(),
        1,
        "only the blocker-recorded owner is collected"
    );
    assert_eq!(
        pending[0].owner_key,
        ("dev-libs".to_string(), "bparent".to_string())
    );
    assert_eq!(pending[0].atom_str, "!<dev-libs/blocked-2.0");
    assert!(pending[0].owner_installed);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_blockers_returns_nothing_when_the_target_matches_nothing() {
    let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
    let pending = vec![PendingBlocker {
        atom_str: "!dev-libs/nonexistent".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "nonexistent".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
}
