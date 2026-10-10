use super::*;

/// Backlog #296: an installed holder's recorded `bdprov:0` is a seed
/// atom like a world atom -- it holds the slot-0 instance a slot-1
/// merge would otherwise uninstall; `:=` / bare edges hold nothing.
#[test]
fn set_seed_holds_the_instance_an_installed_holders_slot_edge_selects() {
    let merged = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("app-misc", "bdprov", "3")
    };
    let held = |atoms: &[&str]| {
        let atoms: Vec<String> = atoms.iter().map(|a| (*a).to_string()).collect();
        set_seed_holds_instance(SetSeedHoldsInstanceArgs {
            set_seed_atoms: &atoms,
            entries: std::slice::from_ref(&merged),
            category: "app-misc",
            package: "bdprov",
            version: "1",
            slot: "0",
            sub_slot: "1",
        })
    };
    assert!(held(&["app-misc/bdprov:0"]));
    assert!(
        !held(&["app-misc/bdprov:="]),
        "a bare `:=` edge rebinds to the merge"
    );
    assert!(!held(&["app-misc/bdprov"]));
}

/// Backlog #289: a set atom that selects the old instance holds it
/// (real `SetArg` parent), a bare atom that the merged new instance
/// also matches does not.
#[test]
fn set_seed_holds_the_instance_only_a_slot_atom_selects() {
    let merged = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("app-misc", "bdprov", "3")
    };
    let held = |atoms: &[&str]| {
        let atoms: Vec<String> = atoms.iter().map(|a| (*a).to_string()).collect();
        set_seed_holds_instance(SetSeedHoldsInstanceArgs {
            set_seed_atoms: &atoms,
            entries: std::slice::from_ref(&merged),
            category: "app-misc",
            package: "bdprov",
            version: "1",
            slot: "0",
            sub_slot: "1",
        })
    };
    assert!(held(&["app-misc/bdprov:0"]), "a slot atom selects slot 0");
    assert!(
        !held(&["app-misc/bdprov"]),
        "a bare atom selects the new slot"
    );
    assert!(!held(&["app-misc/bdprov:1"]), "another slot's atom");
    assert!(
        !held(&["@world", "!app-misc/bdprov:0"]),
        "sets and blockers"
    );
}

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
                // #68 S2 (cell e): a merge-bound match with a merging
                // parent is unresolved outright -- real prints `B`
                // and exits 1. The pre-S2 expectation was `false`.
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
    // `!!dev-libs/target[wantblock]` against a graphed target whose
    // `use_flags_display` has `wantblock` OFF -> the block is a
    // satisfied no-op, dropped (real omits the `[blocks]` line).
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
    // `(+)` default: `gone` is absent from the target's IUSE, so it
    // stands in as enabled -> `-gone` unmet -> block dropped.
    assert!(call("!!dev-libs/target[-gone(+)]").is_empty());
    // but a plain (no `[use]`) blocker still fires,
    assert_eq!(call("!!dev-libs/target").len(), 1);
    // and a use-dep the target DOES satisfy fires too.
    assert_eq!(call("!!dev-libs/target[-wantblock]").len(), 1);
}

#[test]
fn resolve_blockers_buildpkgonly_gate() {
    // #143 S1: real's `--buildpkgonly` blocker gate (upstream
    // `test_blocker.py::testBlockerBuildpkgonly`, oracle
    // `differential-test-bed/logs/fx-b1-blocker.json` cases 6-11):
    // `depgraph.py::_add_pkg_dep_string` blanks RDEPEND/PDEPEND/
    // IDEPEND for a non-built package, and `_validate_blockers`
    // clears `depends_on_order` for every blocker that is not a
    // hard (`!!`) buildtime one. Under `buildpkgonly` only a
    // strong buildtime blocker files (resolved by uninstall —
    // the abort's rows; the exit itself is
    // `buildpkgonly_deps_unsatisfied`'s); soft and runtime-keyed
    // blockers are satisfied silently.
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
    // Soft DEPEND blocker: files normally, dropped under buildpkgonly.
    assert_eq!(call("!dev-libs/blocked", false, true, false).len(), 1);
    assert!(call("!dev-libs/blocked", false, true, true).is_empty());
    // Hard RDEPEND blocker: files normally, dropped under buildpkgonly
    // (real never even walks it — the RDEPEND blanking).
    assert_eq!(call("!!dev-libs/blocked", true, false, false).len(), 1);
    assert!(call("!!dev-libs/blocked", true, false, true).is_empty());
    // Hard DEPEND blocker: kept under buildpkgonly, resolved by
    // uninstalling the installed match.
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
fn resolve_blockers_drops_a_blocker_from_a_superseded_walk_candidate() {
    // #142 S2 F2 (upstream `test_blocker.py::testBlocker`, oracle
    // `differential-test-bed/logs/fx-b1-blocker.json` cases 0-5):
    // every pg0 order walks several X candidates but merges one, and
    // real only ever sees blockers of graph nodes
    // (`_validate_blockers` iterates the digraph). A pending blocker
    // whose declaring instance is not the merged one is stale and
    // files nothing — this is what made the uninstall target follow
    // walk order (Y-2/Y-3) instead of the merged candidate (Y-1).
    let dir = slotundo_temp_dir("blocker-stale-owner");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blk0y",
        version: "3",
        slot: "3",
        rdepend: "",
        installed_use: "",
    });
    let entries = vec![graph_entry("dev-libs", "blk0x", "2")];
    let stale = PendingBlocker {
        atom_str: "!=dev-libs/blk0y-3".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blk0y".to_string(),
        owner_key: ("dev-libs".to_string(), "blk0x".to_string()),
        owner_version: "3".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    assert!(
        resolve_blockers(&dir, &[stale], &entries, &HashSet::new(), false).is_empty(),
        "a walked-but-unmerged candidate's blocker is stale"
    );
    // Control: the merged instance's own blocker still files, paired
    // against its match.
    let current = PendingBlocker {
        atom_str: "!=dev-libs/blk0y-3".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blk0y".to_string(),
        owner_key: ("dev-libs".to_string(), "blk0x".to_string()),
        owner_version: "2".to_string(),
        owner_merging: true,
        owner_installed: false,
    };
    let filed = resolve_blockers(&dir, &[current], &entries, &HashSet::new(), false);
    assert_eq!(filed.len(), 1);
    assert_eq!(
        filed[0].conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall {
            cpv: "dev-libs/blk0y-3".to_string(),
            anchor: ("dev-libs".to_string(), "blk0x".to_string()),
        })
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolve_blockers_keeps_an_installed_match_replaced_in_its_slot_as_satisfied() {
    // #68 S1 / #72 B1 (cell a): installed `blocked-1.0:0` is replaced
    // by the merge-bound `blocked-2.0:0`, so real's tracker no longer
    // holds it and the block resolves to the replacement merge. Since
    // B1 the row is kept, tagged `Replacement`; the renderer hides it
    // until B2 models real's scheduling predicate.
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

    // Cell a': the replacement targets a *different* slot, so the
    // installed match is not replaced (real's same-slot replacement
    // rule); the block is resolved by uninstalling it instead.
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

    // Cell b: a same-version reinstall is merge-bound in the tracker,
    // so it must not be treated as "replaced" -- the row survives
    // unresolved.
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
    // #284 (`app-misc/bdprov` shape): the provider upgrades 2 -> 3
    // into a new slot while its own `!blocked:0` soft-blocks the old
    // installed instance. Real uninstalls the old slot (`[uninstall]`
    // + `[blocks b]`, rc 0): the request parents the merge, not the
    // old instance, so the cp-keyed retry closure must not hold it
    // once the run re-resolves the cp elsewhere.
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
    // The requested cp is always a retry-closure member; without the
    // #284 per-instance rule that veto alone keeps the hard block.
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

    // Case 2: the bound consumer rebuilds (`:=`), so its live dep is
    // rebound to the new slot through its own walk -- its recorded
    // edge no longer parents the old instance either.
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

    // Control: a consumer pinned to the old slot (`:0`, no rebind)
    // still holds it -- the edge survives the #284 skip, so the row
    // stays unresolved even with the closure veto lifted (the cp
    // moves, but a remaining graph node still pulls the instance).
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
    // #68 S2/S3 oracle cells b-h.
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

    // Cell c: installed blocked-1.0 survives, a walked consumer
    // depends on it -> unresolved (digraph node with parents).
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
    // Cell d: the same state without the walked consumer -> the
    // uninstall is fine (real's `b`).
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
    // ...but in a complete-mode context real's digraph holds the
    // installed closure even though portuale's `entries` don't
    // (L0 `sys-apps/systemd-utils`): the vdb reverse scan is the
    // documented fallback and marks it unresolved again. The
    // consumer must be an installed vdb package for it to be found.
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

    // Cell e: a merge-bound match with a merging parent is unresolved
    // outright, installed or not.
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

    // Cell b: a same-version Reinstall is merge-bound, too.
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

    // Cell f: an installed (nomerge) parent's installed match is
    // ignored outright.
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
    // Cell g: same, but a merge-bound match survives; unresolved only
    // when the owner itself is a walked node with parents.
    //
    // #72 B1: the atom matches **two** instances here -- the installed
    // 1.0 (replaced in-slot by the 1.5, so it now carries a hidden
    // `Replacement` row) and the merge-bound 1.5 (the row real's
    // tracker still holds, whose state the owner's parents decide).
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
    // #77 A1: the satisfied nomerge arm removes the **owner** and is
    // ordered after the merge-bound blocked instance (`anchor`).
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
    // #73 cell g: the owner has no walked parent entry, but it is in
    // the `@world`/`@selected`/`@system` closure (real's SetArg
    // parent) -> unresolved.
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
    // #73 S0 cell f / T11: the same closure membership must NOT make
    // an *installed-only* match unresolved -- `merge_bound_match`
    // gates the whole nomerge arm.
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

    // Cell h: a soft same-slot self-block is skipped; a strong one is
    // not.
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
fn resolve_blockers_tags_a_scanned_nomerge_owner_with_its_own_removal() {
    // #77 A1 / A0 n1: the owner is the vardb `Package` the
    // all-installed scan registered (`owner_installed: true`). It was
    // never walked, so it has no entry -- even when the target cp's
    // merge entry exists -- and real removes the **owner**, ordered
    // after the merge-bound blocked instance (`inst_task`).
    let dir = slotundo_temp_dir("blocker-77-scan");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "bparent",
        version: "1.0",
        slot: "0",
        rdepend: "!<dev-libs/blocked-2.0",
        installed_use: "flip",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "blocked",
        version: "1.0",
        slot: "0",
        rdepend: "",
        installed_use: "flip",
    });
    let pb = || PendingBlocker {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "blocked".to_string(),
        owner_key: ("dev-libs".to_string(), "bparent".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: false,
        owner_installed: true,
    };
    let mut upgrade = graph_entry("dev-libs", "blocked", "1.5");
    upgrade.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "1.5".into(),
    };
    // The atom matches the installed 1.0 (replaced in-slot, a hidden
    // `Replacement` twin) and the merge-bound 1.5.
    let conflicts = resolve_blockers(&dir, &[pb()], &[upgrade.clone()], &HashSet::new(), false);
    assert_eq!(conflicts.len(), 2);
    let removal = conflicts
        .iter()
        .find(|f| f.conflict.matched_version == "1.5")
        .expect("merge-bound 1.5 row");
    assert!(!removal.conflict.unsolvable);
    assert_eq!(
        removal.conflict.satisfied_by,
        Some(BlockerSatisfiedBy::Uninstall {
            cpv: "dev-libs/bparent-1.0".to_string(),
            anchor: ("dev-libs".to_string(), "blocked".to_string()),
        })
    );
    // `file_blocker_conflicts`: the Replacement twin (no removal) is
    // dropped for the absent owner, the Uninstall twin creates the
    // removal entry and the row rides it.
    let mut entries = vec![upgrade];
    let mut orphans: Vec<OrphanBlocker> = Vec::new();
    file_blocker_conflicts(&mut entries, &dir, conflicts, &mut orphans);
    assert_eq!(entries.len(), 2);
    let removal_entry = entries
        .iter()
        .find(|e| e.package == "bparent")
        .expect("bparent-1.0 removal entry");
    assert!(matches!(
        &removal_entry.outcome,
        PretendOutcome::Uninstall { version } if version == "1.0"
    ));
    assert_eq!(
        removal_entry.required_by,
        vec![("dev-libs".to_string(), "blocked".to_string())],
        "ordered after the merge-bound blocked instance, not the owner"
    );
    assert_eq!(removal_entry.blockers.len(), 1);
    assert_eq!(removal_entry.blockers[0].matched_version, "1.5");

    // A required-set closure member (the owner is in @selected) makes
    // the block unresolved: no tag, no fabricated removal -- but
    // (backlog #80) the row is no longer dropped: it rides out as an
    // orphan for the trailing blocker group (A0 n1b).
    let closure: HashSet<(String, String)> =
        [("dev-libs".to_string(), "bparent".to_string())].into();
    let mut upgrade = graph_entry("dev-libs", "blocked", "1.5");
    upgrade.outcome = PretendOutcome::Upgrade {
        from: "1.0".into(),
        to: "1.5".into(),
    };
    let conflicts = resolve_blockers(&dir, &[pb()], &[upgrade.clone()], &closure, false);
    assert!(
        conflicts
            .iter()
            .find(|f| f.conflict.matched_version == "1.5")
            .expect("merge-bound 1.5 row")
            .conflict
            .unsolvable
    );
    let mut entries = vec![upgrade];
    let mut orphans: Vec<OrphanBlocker> = Vec::new();
    file_blocker_conflicts(&mut entries, &dir, conflicts, &mut orphans);
    assert!(
        entries.iter().all(|e| e.package != "bparent"),
        "an unresolved absent-owner row fabricates no removal"
    );
    assert_eq!(orphans.len(), 1, "the n1b row survives as an orphan");
    let orphan = &orphans[0];
    assert_eq!(
        (
            orphan.owner_category.as_str(),
            orphan.owner_package.as_str()
        ),
        ("dev-libs", "bparent")
    );
    assert!(orphan.conflict.unsolvable);
    assert_eq!(orphan.conflict.matched_version, "1.5");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unwalked_scan_skips_without_merge_bound_entries_and_blocker_free_owners() {
    // #83: the two cheap guards in front of the scan. (a) with
    // nothing merge-bound the whole scan is a no-op even when an
    // installed owner records a blocker (`resolve_blockers` would
    // drop every row it could produce); (b) an owner whose effective
    // runtime deps carry no `!` token cannot contribute a blocker,
    // so it never reaches the USE reduction.
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

    // (a): the only entry is installed-only -- the recorded blocker
    // stays invisible, and the scan returns before touching the vdb.
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

    // (b): a merge-bound upgrade of another cp collects the recorded
    // blocker but not the blocker-free owner.
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
