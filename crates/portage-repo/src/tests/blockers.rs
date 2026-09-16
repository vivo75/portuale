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
fn resolve_blockers_buildpkgonly_gate() {
    // Portage blanks non-built RDEPEND/PDEPEND/IDEPEND and clears non-hard
    // blockers: under `buildpkgonly` only a strong buildtime blocker files.
    let dir = slotundo_temp_dir("blocker-buildpkgonly-gate");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "",
    });
    let owner = graph_entry("dev-libs", "owner", "1.0");
    let entries = vec![owner];
    let call = |atom: &str, strong: bool, buildtime: bool, buildpkgonly: bool| {
        resolve_blockers(
            &dir,
            &[PendingBlocker {
                atom_str: atom.to_string(),
                strong,
                buildtime,
                target_category: "dev-libs".to_string(),
                target_package: "blocked".to_string(),
                owner_key: ("dev-libs".to_string(), "owner".to_string()),
                owner_version: "1.0".to_string(),
                owner_merging: true,
                owner_installed: false,
            }],
            &entries,
            &HashSet::new(),
            buildpkgonly,
        )
    };
    // Soft DEPEND: files normally, dropped under buildpkgonly.
    assert_eq!(call("!dev-libs/blocked", false, true, false).len(), 1);
    assert!(call("!dev-libs/blocked", false, true, true).is_empty());
    // Hard RDEPEND: files normally, dropped under buildpkgonly (never walked).
    assert_eq!(call("!!dev-libs/blocked", true, false, false).len(), 1);
    assert!(call("!!dev-libs/blocked", true, false, true).is_empty());
    // Hard DEPEND: kept under buildpkgonly, resolved by uninstall.
    let kept = call("!!dev-libs/blocked", true, true, true);
    assert_eq!(kept.len(), 1);
    assert!(!kept[0].conflict.unsolvable);
    assert!(matches!(
        kept[0].conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall { .. })
    ));
    let _ = fs::remove_dir_all(&dir);
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
fn resolve_blockers_uninstalls_a_settled_block_when_the_cp_moves_slot() {
    // The provider moves slot while soft-blocking the old instance: Portage
    // uninstalls the old slot, so the cp-keyed retry closure must not hold it.
    let dir = slotundo_temp_dir("blocker-284-moves-slot");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "2",
        slot: "1/1",
        rdepend: "",
        installed_use: "",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "bcons",
        version: "1",
        slot: "0",
        rdepend: "dev-libs/blocked:0/1=",
        installed_use: "",
    });
    let mut upgrade = graph_entry("dev-libs", "blocked", "3");
    upgrade.outcome = PretendOutcome::Upgrade {
        from: "2".into(),
        to: "3".into(),
    };
    upgrade.slot = Some("1".to_string());
    upgrade.sub_slot = Some("2".to_string());
    let pending = || PendingBlocker {
        atom_str: "!dev-libs/blocked:0".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), "blocked".to_string()),
        owner_version: "3".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    // The requested cp is always a closure member; only the per-instance rule
    // below lifts the veto.
    let closure: HashSet<(String, String)> =
        [("dev-libs".to_string(), "blocked".to_string())].into();
    let conflicts = resolve_blockers(&dir, &[pending()], &[upgrade.clone()], &closure, false);
    assert_eq!(conflicts.len(), 1);
    assert!(
        !conflicts[0].conflict.unsolvable,
        "a parentless old slot uninstalls even though its cp was requested"
    );
    assert_eq!(
        conflicts[0].conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall {
            cpv: "dev-libs/blocked-1".to_string(),
            anchor: ("dev-libs".to_string(), "blocked".to_string()),
        })
    );

    // Case 2: the bound consumer rebuilds (`:=`), so its live dep rebinds to the
    // new slot and no longer parents the old instance.
    let edge = DepEdge {
        atom: "dev-libs/blocked:=".to_string(),
        evaluated: "dev-libs/blocked:=".to_string(),
        category: "dev-libs".to_string(),
        package: "blocked".to_string(),
        priority: DepPriority {
            runtime: true,
            runtime_slot_op: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let mut rebuild = graph_entry("dev-libs", "bcons", "1");
    rebuild.outcome = PretendOutcome::Reinstall {
        version: "1".into(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: true,
    };
    rebuild.slot = Some("0".to_string());
    rebuild.sub_slot = Some("0".to_string());
    rebuild.deps = vec![edge];
    let conflicts = resolve_blockers(
        &dir,
        &[pending()],
        &[upgrade.clone(), rebuild],
        &closure,
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(
        !conflicts[0].conflict.unsolvable,
        "a rebound `:=` consumer does not hold the old slot"
    );

    // Control: a consumer pinned to the old slot still holds it.
    let pinned = DepEdge {
        atom: "dev-libs/blocked:0".to_string(),
        evaluated: "dev-libs/blocked:0".to_string(),
        category: "dev-libs".to_string(),
        package: "blocked".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let mut staying = graph_entry("dev-libs", "bcons", "1");
    staying.outcome = PretendOutcome::AlreadyInstalled {
        version: "1".into(),
    };
    staying.slot = None;
    staying.sub_slot = None;
    staying.deps = vec![pinned];
    let conflicts = resolve_blockers(&dir, &[pending()], &[upgrade, staying], &closure, false);
    assert_eq!(conflicts.len(), 1);
    assert!(
        conflicts[0].conflict.unsolvable,
        "a slot-pinned consumer still holds the old instance"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_blockers_classifies_final_graph_matches_by_parent_operation() {
    // Oracle cells b-h: final graph matches classified by parent operation.
    let edge = |cat: &str, pkg: &str, atom: &str| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: cat.to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let pb = |atom: &str, owner: &str| PendingBlocker {
        atom_str: atom.to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), owner.to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    let installed = |pkg: &str| GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", pkg, "1.0")
    };
    let blocked_upgrade = |to: &str| {
        let mut e = graph_entry("dev-libs", "blocked", to);
        e.outcome = PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: to.into(),
        };
        e
    };

    // Cell c: a walked consumer holds the surviving installed match.
    let dir = slotundo_temp_dir("blocker-s2-c");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "",
    });
    let mut walker = installed("bconsumer");
    walker.deps = vec![edge("dev-libs", "blocked", "dev-libs/blocked")];
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[graph_entry("dev-libs", "bparent", "1.0"), walker],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(
        conflicts[0].conflict.unsolvable,
        "a walked consumer blocks the unmerge"
    );
    // Cell d: without the walked consumer the uninstall is fine.
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[graph_entry("dev-libs", "bparent", "1.0")],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(
        !conflicts[0].conflict.unsolvable,
        "an unwalked consumer does not"
    );
    // In a complete-mode context Portage's digraph holds the installed closure
    // even though portuale's `entries` don't: the vdb reverse scan is the
    // fallback and marks it unresolved again.
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "bconsumer",
        version: "1.0",
        slot: "0",
        rdepend: "dev-libs/blocked",
        installed_use: "",
    });
    let closure: HashSet<(String, String)> =
        [("dev-libs".to_string(), "blocked".to_string())].into();
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[graph_entry("dev-libs", "bparent", "1.0")],
        &closure,
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(
        conflicts[0].conflict.unsolvable,
        "a required-set closure member is a graph node with parents"
    );
    let _ = fs::remove_dir_all(&dir);

    // Cell e: merge-bound match with a merging parent is unresolved outright.
    let mut bparent3 = graph_entry("dev-libs", "bparent3", "1.0");
    bparent3.blockers = Vec::new();
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[PendingBlocker {
            atom_str: "!<dev-libs/blocked-3".to_string(),
            strong: false,
            buildtime: false,
            target_category: "dev-libs".to_string(),
            target_package: "blocked".to_string(),
            owner_key: ("dev-libs".to_string(), "bparent3".to_string()),
            owner_version: "1.0".to_string(),
            owner_merging: true,
            owner_installed: false,
        }],
        &[bparent3, graph_entry("dev-libs", "blocked", "2.0")],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(
        conflicts[0].conflict.unsolvable,
        "merge-bound match, merging parent"
    );

    // Cell b: a same-version Reinstall is merge-bound too.
    let dir = slotundo_temp_dir("blocker-s2-b");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "",
    });
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
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[graph_entry("dev-libs", "bparent", "1.0"), reinstall],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].conflict.unsolvable);
    let _ = fs::remove_dir_all(&dir);

    // Cell f: an installed (nomerge) parent's installed match is ignored.
    let dir = slotundo_temp_dir("blocker-s2-f");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "",
    });
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[installed("bparent")],
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty(), "nomerge parent, installed match");
    // Cell g: a merge-bound match survives; unresolved only when the owner is a
    // walked node with parents. The atom matches two instances: installed 1.0
    // (replaced in-slot, carrying a hidden `Replacement` row) and merge-bound
    // 1.5 (whose state the owner's parents decide).
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[installed("bparent"), blocked_upgrade("1.5")],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 2, "installed 1.0 + merge-bound 1.5");
    assert_eq!(
        conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.0")
            .expect("installed 1.0 row")
            .conflict
            .satisfied_by,
        Some(BlockerSatisfiedBy::Replacement {
            cp: ("dev-libs".to_string(), "blocked".to_string()),
            slot: "0".to_string(),
        })
    );
    assert!(
        !conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.5")
            .expect("merge-bound 1.5 row")
            .conflict
            .unsolvable,
        "nomerge parent, no graph parents"
    );
    // The satisfied nomerge arm removes the owner, ordered after the blocked instance.
    assert_eq!(
        conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.5")
            .expect("merge-bound 1.5 row")
            .conflict
            .satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall {
            cpv: "dev-libs/bparent-1.0".to_string(),
            anchor: ("dev-libs".to_string(), "blocked".to_string()),
        })
    );
    let mut parent_walker = installed("consumer");
    parent_walker.deps = vec![edge("dev-libs", "bparent", "dev-libs/bparent")];
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[installed("bparent"), blocked_upgrade("1.5"), parent_walker],
        &HashSet::new(),
        false,
    );
    assert_eq!(conflicts.len(), 2);
    assert!(
        conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.5")
            .expect("merge-bound 1.5 row")
            .conflict
            .unsolvable,
        "nomerge parent with graph parents"
    );
    // Owner in the world/selected/system closure (a SetArg parent): unresolved.
    let closure: HashSet<(String, String)> =
        [("dev-libs".to_string(), "bparent".to_string())].into();
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[installed("bparent"), blocked_upgrade("1.5")],
        &closure,
        false,
    );
    assert_eq!(conflicts.len(), 2);
    assert!(
        conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.5")
            .expect("merge-bound 1.5 row")
            .conflict
            .unsolvable,
        "#73 cell g: the required-set closure is a parent"
    );
    // Closure membership alone never unresolves an installed-only match.
    let conflicts = resolve_blockers(
        &dir,
        &[pb("!<dev-libs/blocked-2.0", "bparent")],
        &[installed("bparent")],
        &closure,
        false,
    );
    assert!(
        conflicts.is_empty(),
        "#73 cell f: no merge-bound match, closure membership is not enough"
    );
    let _ = fs::remove_dir_all(&dir);

    // Cell h: a soft same-slot self-block is skipped; a strong one is not.
    let soft = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[PendingBlocker {
            atom_str: "!dev-libs/blocked".to_string(),
            strong: false,
            buildtime: false,
            target_category: "dev-libs".to_string(),
            target_package: "blocked".to_string(),
            owner_key: ("dev-libs".to_string(), "blocked".to_string()),
            owner_version: "1.0".to_string(),
            owner_merging: true,
            owner_installed: false,
        }],
        &[graph_entry("dev-libs", "blocked", "1.0")],
        &HashSet::new(),
        false,
    );
    assert!(soft.is_empty(), "soft same-slot skip");
    let strong = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &[PendingBlocker {
            atom_str: "!!dev-libs/blocked".to_string(),
            strong: true,
            buildtime: true,
            target_category: "dev-libs".to_string(),
            target_package: "blocked".to_string(),
            owner_key: ("dev-libs".to_string(), "blocked".to_string()),
            owner_version: "1.0".to_string(),
            owner_merging: true,
            owner_installed: false,
        }],
        &[graph_entry("dev-libs", "blocked", "1.0")],
        &HashSet::new(),
        false,
    );
    assert_eq!(strong.len(), 1, "a strong blocker is not skipped");
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
