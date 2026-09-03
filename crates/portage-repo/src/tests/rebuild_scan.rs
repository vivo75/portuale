use super::*;

/// Under `--update` a provider the pass downgraded is a missed
/// update: the downgrade's `from` slot/sub-slot feeds the scan.
/// Portage: depgraph.py, _slot_operator_update_probe
#[test]
fn slot_operator_rebuild_scan_treats_a_pin_forced_downgrade_as_a_missed_update() {
    let dir = TempDir::new("portage-repo-slotop-303").keep();
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    // `bar` 2.0 is installed at sub-slot 2; `holder` was built
    // against the older sub-slot 1 and pins it.
    mk("bar-2.0", "0/2", "");
    mk("holder-1.0", "0", "dev-libs/bar:0/1=");
    let bar_downgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Downgrade {
            from: "2.0".into(),
            to: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("1".into()),
        ..graph_entry("dev-libs", "bar", "1.0")
    };
    let reach: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "holder".to_string())]);
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let scan = |update: bool, top: &HashSet<(String, String)>| {
        slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
            root: &dir,
            repos: &[],
            entries: std::slice::from_ref(&bar_downgrade),
            reachable: &reach,
            already: &empty,
            undone: &empty,
            with_bdeps: true,
            update,
            top_level_cps: top,
            top_level_atoms: &HashSet::new(),
            excluded: &[],
            config: &test_config(),
        })
        .0
    };
    assert_eq!(
        scan(true, &HashSet::new()),
        BTreeSet::from([("dev-libs".to_string(), "holder".to_string())]),
        "the pin-forced downgrade schedules the stale consumer's rebuild"
    );
    assert!(
        scan(false, &HashSet::new()).is_empty(),
        "without --update a downgrade is not a missed update"
    );
    let requested: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "bar".to_string())]);
    assert!(
        scan(true, &requested).is_empty(),
        "an explicitly requested downgrade is not a missed update"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_operator_rebuild_scan_flags_only_the_stale_bindings() {
    // A throwaway vdb: `foo` (unused here), a `stale` consumer bound
    // `bar:2/2=`, a `fresh` consumer bound `bar:2/9=`, an
    // `otherslot` consumer bound `bar:1/1=`, and a `nonop` consumer
    // with a plain (no `=`) `bar:2` dep.
    let dir = TempDir::new("portage-repo-slotop").keep();
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("stale-1.0", "0", "dev-libs/bar:2/2=");
    mk("fresh-1.0", "0", "dev-libs/bar:2/9=");
    mk("otherslot-1.0", "0", "dev-libs/bar:1/1=");
    mk("nonop-1.0", "0", "dev-libs/bar:2");

    // `bar` is being upgraded to slot 2 sub-slot 9 this run.
    let bar_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    // Every consumer is "reachable" here (the gating is tested
    // separately in `required_set_reachable_cps_*`); empty repos ->
    // the rebuild slot falls back to the vdb `SLOT` (no cascade).
    let reach: HashSet<(String, String)> = ["stale", "fresh", "otherslot", "nonop"]
        .iter()
        .map(|p| ("dev-libs".to_string(), (*p).to_string()))
        .collect();
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([("dev-libs".to_string(), "stale".to_string())]),
        "only the stale :2/2= binding is scheduled for a rebuild"
    );
    assert_eq!(
        abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )]
    );

    // Nothing changing `bar` -> no rebuilds.
    let (empty_sched, empty_abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: &[],
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert!(empty_sched.is_empty() && empty_abi.is_empty());

    // Not reachable -> the post-walk vdb scan is suppressed entirely
    // (Portage: no slot-op rebuild for a consumer outside the required
    // sets). The slot-operator-dep probe is not complete-mode gated, but `bar`'s
    // upgrade carries no unbuilt `:=` dep, so nothing fires here.
    let (none_sched, none_abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &HashSet::new(),
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert!(none_sched.is_empty() && none_abi.is_empty());

    // A cp already in `already` stays reported once in the graph as
    // the asked-for rebuild (`abi_rebuilds` survives); a different
    // in-graph cp is still skipped.
    let stale_rebuild = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "stale".into(),
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: true,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "stale", "1.0")
    };
    let already = BTreeSet::from([("dev-libs".to_string(), "stale".to_string())]);
    let (again, again_abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: &[bar_upgrade.clone(), stale_rebuild],
        reachable: &reach,
        already: &already,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(again, already, "the set is stable -- no second restart");
    assert_eq!(
        again_abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )],
        "the causing-rebuilds pair survives into the settling pass"
    );

    // The `--solver=` bridges must still build the one flagged entry.
    let (out, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    let names: Vec<&str> = out.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["stale"]);
    assert!(matches!(
        out[0].outcome,
        PretendOutcome::Reinstall {
            slot_operator_rebuild: true,
            ..
        }
    ));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_operator_rebuild_scan_honours_with_bdeps() {
    // DEPEND/BDEPEND read only with `with_bdeps` (Portage empties them
    // otherwise); RDEPEND/PDEPEND/IDEPEND stay unconditional.
    // Portage: lib/_emerge/depgraph.py
    let dir = TempDir::new("portage-repo-slotop-bdeps").keep();
    let mk = |name: &str, key: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join(key), "dev-libs/bar:2/2=\n").unwrap();
    };
    mk("rdep-1.0", "RDEPEND");
    mk("dep-1.0", "DEPEND");
    mk("bdep-1.0", "BDEPEND");
    mk("pdep-1.0", "PDEPEND");
    mk("idep-1.0", "IDEPEND");

    // `bar` moves to slot 2 sub-slot 9 this run.
    let bar_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    let reach: HashSet<(String, String)> = ["rdep", "dep", "bdep", "pdep", "idep"]
        .iter()
        .map(|p| ("dev-libs".to_string(), (*p).to_string()))
        .collect();
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let cp = |name: &str| ("dev-libs".to_string(), name.to_string());

    let runtime_only = BTreeSet::from([cp("rdep"), cp("pdep"), cp("idep")]);
    let all_five = BTreeSet::from([cp("rdep"), cp("dep"), cp("bdep"), cp("pdep"), cp("idep")]);
    // `walked` entries: what the pass actually visited (an
    // `AlreadyInstalled` node per consumer, the @world shape).
    let walked_entry = |name: &str| GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", name, "1.0")
    };

    // bdeps off, consumers only reachable: runtime/install-time keys.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: false,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(
        scheduled, runtime_only,
        "DEPEND/BDEPEND are skipped without with_bdeps"
    );
    assert!(
        !abi.iter()
            .any(|(_, c)| c == "dev-libs/dep-1.0" || c == "dev-libs/bdep-1.0")
    );

    // Reachable-but-unwalked consumers keep build-time keys off.
    // Portage: _emerge/depgraph.py
    let (scheduled, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(
        scheduled, runtime_only,
        "build-time keys need a walked consumer even with bdeps on"
    );

    // bdeps on and every consumer walked (@world complete-mode
    // seeds): every key is scanned, as before this slice.
    let mut walked: Vec<GraphEntry> = vec![bar_upgrade.clone()];
    for name in ["rdep", "dep", "bdep", "pdep", "idep"] {
        walked.push(walked_entry(name));
    }
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: &walked,
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(scheduled, all_five);
    assert_eq!(
        abi,
        vec![
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/bdep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/dep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/idep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/pdep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/rdep-1.0".to_string()
            ),
        ]
    );

    // The reachability gate beats the walk test: even a walked consumer's
    // build-time keys stay out when bdeps is off.
    let (scheduled, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: &walked,
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: false,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(scheduled, runtime_only);

    // The slot-change probe (`slot_operator_slot_change_probe`) reads
    // merge-bound parents' tree metadata, not the vdb, and must not
    // gain a with_bdeps gate -- covered by its own test above.
    let _ = fs::remove_dir_all(&dir);
}

/// A `Reinstall` parent counts toward the `new_slot` map; the skip
/// clauses (`undone`, unreachable-while-gated) suppress independently.
#[test]
fn slot_operator_rebuild_scan_reinstall_parent_and_skip_clauses() {
    let dir = slotundo_temp_dir("161-scan");
    let d = dir.join("var/db/pkg/dev-libs/stale-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    let bar_reinstall = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Reinstall {
            version: "2.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    let stale = ("dev-libs".to_string(), "stale".to_string());
    let reach: HashSet<(String, String)> = HashSet::from([stale.clone()]);
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    // The Reinstall parent feeds `new_slot`, so the stale `:2/2=`
    // binding schedules a rebuild.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &reach,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert_eq!(scheduled, BTreeSet::from([stale.clone()]));
    assert_eq!(
        abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )]
    );
    // An `undone` consumer is skipped even though it is reachable.
    let undone = BTreeSet::from([stale.clone()]);
    let (done_sched, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &reach,
        already: &empty,
        undone: &undone,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert!(done_sched.is_empty(), "`undone` suppresses the rebuild");
    // A reachable set that omits the consumer suppresses the scan --
    // the gate itself is non-empty, so the loop runs and skips.
    let elsewhere: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "other".to_string())]);
    let (far_sched, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &elsewhere,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert!(far_sched.is_empty(), "unreachable consumers never schedule");
    let _ = fs::remove_dir_all(&dir);
}
