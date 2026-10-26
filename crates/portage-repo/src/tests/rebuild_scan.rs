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
            entries: std::slice::from_ref(&bar_downgrade),
            reachable: &reach,
            scope: None,
            already: &empty,
            undone: &empty,
            with_bdeps: true,
            update,
            top_level_cps: top,
            top_level_atoms: &HashSet::new(),
            excluded: &[],
            config: &test_config(),
            gate: crate::test_gate(&[]),
        })
        .0
    };
    assert_eq!(
        scan(true, &HashSet::new()),
        BTreeSet::from([
            ("dev-libs".to_string(), "bar".to_string()),
            ("dev-libs".to_string(), "holder".to_string())
        ]),
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
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([
            ("dev-libs".to_string(), "bar".to_string()),
            ("dev-libs".to_string(), "stale".to_string())
        ]),
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
        entries: &[],
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert!(empty_sched.is_empty() && empty_abi.is_empty());

    // Not reachable -> the post-walk vdb scan is suppressed entirely
    // (Portage: no slot-op rebuild for a consumer outside the required
    // sets). The slot-operator-dep probe is not complete-mode gated, but `bar`'s
    // upgrade carries no unbuilt `:=` dep, so nothing fires here.
    let (none_sched, none_abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
    let already = BTreeSet::from([
        ("dev-libs".to_string(), "bar".to_string()),
        ("dev-libs".to_string(), "stale".to_string()),
    ]);
    let (again, again_abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        entries: &[bar_upgrade.clone(), stale_rebuild],
        reachable: &reach,
        scope: None,
        already: &already,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
        gate: crate::test_gate(&[]),
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

/// A provider merged into a fresh slot schedules the installed
/// consumers bound to the old slot, even with an empty `reachable`
/// set, as long as they were walked this pass.
/// Portage: depgraph.py, _slot_operator_update_probe (bug 486580)
#[test]
fn slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-newslot").keep();
    // vdb: provider `massb-1` in slot 1, consumer `massc-1` bound
    // `massb:1/1=`.
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    // Tree: `massb-2.0` at slot 2/2, plus a `massc-1.0` ebuild (the
    // replacement parent must exist).
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"211 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "2.0", "2/2");
    write_pkg("massc", "1.0", "0");
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    // `massb-2.0` merged into the fresh slot 2; the consumer is walked
    // (AlreadyInstalled) but `reachable` is empty -- the empty-world
    // conflict-mass shape.
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "massc", "1.0")
    };
    let entries = vec![fresh.clone(), consumer.clone()];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([massc.clone()]),
        "the consumer bound to the abandoned slot rebuilds"
    );
    // The new-slot arm records the consumer alone, so no display pair.
    // Portage: depgraph.py, _slot_operator_update_backtrack
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    // No `--update` and provider not directly requested: withheld,
    // mirroring Portage's `want_update` gate.
    let (withheld, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(withheld.is_empty(), "without --update the probe withholds");
    // ...unless the provider is directly requested (Portage arg-chain).
    let top = HashSet::from([("dev-libs".to_string(), "massb".to_string())]);
    let top_atoms = HashSet::from(["dev-libs/massb".to_string()]);
    let (via_arg, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &top,
        top_level_atoms: &top_atoms,
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(via_arg, BTreeSet::from([massc.clone()]));
    // With `--update` and the provider requested, the greedy pin holds
    // the installed slot and the replace set stays empty.
    let top_atoms = HashSet::from(["dev-libs/massb".to_string()]);
    let (greedy_pinned, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &top,
        top_level_atoms: &top_atoms,
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(
        greedy_pinned.is_empty(),
        "a requested provider below the highest slot probes nothing"
    );
    // A lower-version fresh slot is a downgrade never probed -- unless
    // nothing higher is available; a graph downgrade never vetoes that.
    let older = GraphEntry {
        outcome: PretendOutcome::New {
            version: "0.9".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "0.9")
    };
    let (downgraded, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[older.clone(), consumer.clone()],
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        downgraded,
        BTreeSet::from([massc.clone()]),
        "a graph downgrade does not veto the available upgrade"
    );
    // Pure downgrade: no higher version anywhere, so nothing probes.
    let old_base = TempDir::new("portage-repo-slotop-downgrade").keep();
    let old_repo = old_base.join("repo");
    let write_old = |pkg: &str, pv: &str, slot: &str| {
        let dir = old_repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"211 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = old_repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_old("massb", "0.9", "2/2");
    write_old("massc", "1.0", "0");
    let old_repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: old_repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    // The vdb half lives on `base` (installed `massb-1` + `massc-1`);
    // only the tree is downgrade-only here.
    let (pure_downgrade, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[older, consumer.clone()],
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&old_repos),
    });
    assert!(pure_downgrade.is_empty(), "downgrades never probe");
    let _ = fs::remove_dir_all(&old_base);
    // No tree candidate for the consumer: no replacement parent, no
    // schedule (Portage finds nothing and does nothing).
    let (nocand, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert!(nocand.is_empty(), "no candidate means no schedule");
    // Neither walked nor reachable: outside the probe's population.
    let (unwalked, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: std::slice::from_ref(&fresh),
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(unwalked.is_empty(), "unwalked consumers never schedule");
    let _ = fs::remove_dir_all(&base);
}

/// The two cuts of the pin gate: a blocker-discarded pin refuses
/// nothing, and an explicit single-slot atom pins nothing.
#[test]
fn slot_operator_rebuild_scan_pin_gate_cuts_probe() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-pingate").keep();
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str, rdepend: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            format!("EAPI=8\nDESCRIPTION=\"268 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !rdepend.is_empty() {
            body.push_str(&format!("RDEPEND=\"{rdepend}\"\n"));
        }
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=268 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64").unwrap();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "SLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "1.0", "1/1", "");
    write_pkg("massb", "2.0", "2/2", "!dev-libs/massb:1");
    write_pkg("massc", "1.0", "0", "");
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "massc", "1.0")
    };
    let entries = vec![fresh, consumer];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massb = ("dev-libs".to_string(), "massb".to_string());
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let top = HashSet::from([massb.clone()]);
    let scan = |atoms: &HashSet<String>| {
        slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
            root: &base,
            entries: &entries,
            reachable: &HashSet::new(),
            scope: None,
            already: &empty,
            undone: &empty,
            with_bdeps: true,
            update: true,
            top_level_cps: &top,
            top_level_atoms: atoms,
            excluded: &[],
            config: &test_config(),
            gate: crate::test_gate(&repos),
        })
    };
    // Cut (b): the bare request's pin is discarded by the blocker
    // lookahead, so the probe fires.
    let bare = HashSet::from(["dev-libs/massb".to_string()]);
    let (discarded, abi, ..) = scan(&bare);
    assert_eq!(
        discarded,
        BTreeSet::from([massc.clone()]),
        "a blocker-discarded pin refuses nothing"
    );
    assert!(abi.is_empty(), "new-slot arm: consumer alone");
    // Cut (a): an explicit slot atom spans one slot and pins nothing.
    let slot_atom = HashSet::from(["dev-libs/massb:2".to_string()]);
    let (slotted, ..) = scan(&slot_atom);
    assert_eq!(
        slotted,
        BTreeSet::from([massc.clone()]),
        "an explicit slot atom pins nothing"
    );
    // Cut (a): an exact-version atom spans one slot, but as an argument
    // parent it vetoes the new-slot candidate, so nothing probes.
    let ver_atom = HashSet::from(["=dev-libs/massb-1.0".to_string()]);
    let (versioned, ..) = scan(&ver_atom);
    assert!(
        versioned.is_empty(),
        "the exact-version argument vetoes the new-slot candidate: {versioned:?}"
    );
    let _ = fs::remove_dir_all(&base);
}

/// A replacement whose fresh candidate violates another parent's atom
/// is refused on both arms; satisfied, self, out-of-scope, replaced
/// or excluded parents never veto.
/// Portage: depgraph.py, _slot_operator_check_reverse_dependencies
#[test]
fn slot_operator_update_probe_refusal_vetoes_both_arms() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-refusal").keep();
    // vdb: provider `massb-1` in slot 1, consumer `massc-1` bound
    // `massb:1/1=`, veto parent `massv-1` pinning `<massb-2`.
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
        ("massv-1", "0", "<dev-libs/massb-2"),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!("EAPI=8\nDESCRIPTION=\"211 R2\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 R2\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "2.0", "2/2");
    write_pkg("massc", "1.0", "0");
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let walked_entry = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let consumer = walked_entry("massc");
    let veto_parent = walked_entry("massv");
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let scan = |entries: &[GraphEntry], excluded: &[String]| {
        slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
            root: &base,
            entries,
            reachable: &HashSet::new(),
            scope: None,
            already: &empty,
            undone: &empty,
            with_bdeps: true,
            update: true,
            top_level_cps: &HashSet::new(),
            top_level_atoms: &HashSet::new(),
            excluded,
            config: &test_config(),
            gate: crate::test_gate(&repos),
        })
    };
    // The veto parent walked: the fresh candidate violates its pin,
    // so the whole replacement is refused -- nothing schedules.
    let (refused, ..) = scan(&[fresh.clone(), consumer.clone(), veto_parent.clone()], &[]);
    assert!(refused.is_empty(), "a vetoed replacement schedules nothing");
    // Out of scope: Portage never walks the parent, so no veto.
    let (unscoped, ..) = scan(&[fresh.clone(), consumer.clone()], &[]);
    assert_eq!(unscoped, BTreeSet::from([massc.clone()]));
    // Excluded: Portage's `excluded_pkgs` skip.
    let (excluded_out, ..) = scan(
        &[fresh.clone(), consumer.clone(), veto_parent.clone()],
        &["dev-libs/massv".to_string()],
    );
    assert_eq!(excluded_out, BTreeSet::from([massc.clone()]));
    // Same-slot arm: an in-slot upgrade the veto parent rejects is
    // refused the same way.
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let reach_massc: HashSet<(String, String)> =
        HashSet::from([massc.clone(), ("dev-libs".to_string(), "massv".to_string())]);
    let (same_refused, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[upgrade.clone(), consumer.clone(), veto_parent.clone()],
        reachable: &reach_massc,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(same_refused.is_empty(), "the same-slot arm refuses too");
    let (same_kept, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[upgrade, consumer, veto_parent],
        reachable: &reach_massc,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &["dev-libs/massv".to_string()],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        same_kept,
        BTreeSet::from([massc, ("dev-libs".to_string(), "massb".to_string())])
    );
    let _ = fs::remove_dir_all(&base);
}

/// The new-child-slot arm also probes entries in a slot the consumer
/// is not bound to; bound-slot entries stay with the same-slot arm.
/// Portage: depgraph.py, _iter_similar_available
#[test]
fn slot_operator_new_slot_arm_probes_bound_slot_mismatched_entries() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-mismatch").keep();
    // vdb: provider `massm` installed in slots 0 (`1`) and 1 (`2`),
    // consumer `massn-1` bound `massm:0/1=`, consumer `masso-1`
    // bound `massm:1/1=`.
    for (name, slot, rdepend) in [
        ("massm-1", "0", ""),
        ("massm-2", "1", ""),
        ("massn-1", "0", "dev-libs/massm:0/1="),
        ("masso-1", "0", "dev-libs/massm:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    for pkg in ["massn", "masso"] {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = "EAPI=8\nDESCRIPTION=\"211 mismatch\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n";
        std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 mismatch\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT=0\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-1.0")), entry).unwrap();
    }
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    // `massm-3.0` upgrades the slot-1 instance; the consumer is bound
    // to slot 0 -- a bound-slot mismatch on an `Upgrade` entry, with
    // no `New` entry anywhere.
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massm", "3.0")
    };
    let walked_consumer = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massn = ("dev-libs".to_string(), "massn".to_string());
    // Both consumers walked, neither reachable: the same-slot arm is
    // out of scope. `massn` (bound to slot 0) probes the slot-1
    // upgrade; `masso` (bound to the entry's own slot 1) must not
    // leak that entry into the new arm.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[upgrade, walked_consumer("massn"), walked_consumer("masso")],
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(scheduled, BTreeSet::from([massn]));
    // The new-slot arm records the consumer alone: no display pair.
    // Portage: depgraph.py, _slot_operator_update_backtrack
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    let _ = fs::remove_dir_all(&base);
}

/// The new-slot arm needs a replacement parent whose own atom accepts
/// the new slot: `pinned` (`:0=`) never rebuilds, `open` (`:=`) does.
/// Portage: depgraph.py, _slot_operator_update_probe
#[test]
fn slot_operator_new_slot_arm_needs_a_replacement_atom_that_accepts_the_new_slot() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-304").keep();
    for (name, slot, rdepend) in [
        ("massm-1", "0", ""),
        ("massm-2", "1", ""),
        ("pinned-1", "0", "dev-libs/massm:0/1="),
        ("open-1", "0", "dev-libs/massm:0/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    for (pkg, rdepend) in [
        ("pinned", "dev-libs/massm:0="),
        ("open", "dev-libs/massm:="),
    ] {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "EAPI=8\nDESCRIPTION=\"304\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"{rdepend}\"\n"
        );
        std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=304\nEAPI=8\n".to_string();
        writeln!(
            entry,
            "KEYWORDS=amd64\nRDEPEND={rdepend}\nSLOT=0\n_md5_={md5}"
        )
        .unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-1.0")), entry).unwrap();
    }
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massm", "3.0")
    };
    let walked_consumer = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let (scheduled, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &[upgrade, walked_consumer("pinned"), walked_consumer("open")],
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([("dev-libs".to_string(), "open".to_string())]),
        "only the consumer whose replacement atom accepts slot 1 rebuilds"
    );
    let _ = fs::remove_dir_all(&base);
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

    // The upgraded provider joins its consumers in the replace set.
    let runtime_only = BTreeSet::from([cp("bar"), cp("rdep"), cp("pdep"), cp("idep")]);
    let all_five = BTreeSet::from([
        cp("bar"),
        cp("rdep"),
        cp("dep"),
        cp("bdep"),
        cp("pdep"),
        cp("idep"),
    ]);
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
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: false,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
        entries: &walked,
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
        entries: &walked,
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: false,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
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
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([stale.clone(), ("dev-libs".to_string(), "bar".to_string())])
    );
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
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &reach,
        scope: None,
        already: &empty,
        undone: &undone,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert!(done_sched.is_empty(), "`undone` suppresses the rebuild");
    // A reachable set that omits the consumer suppresses the scan --
    // the gate itself is non-empty, so the loop runs and skips.
    let elsewhere: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "other".to_string())]);
    let (far_sched, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        entries: std::slice::from_ref(&bar_reinstall),
        reachable: &elsewhere,
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&[]),
    });
    assert!(far_sched.is_empty(), "unreachable consumers never schedule");
    let _ = fs::remove_dir_all(&dir);
}

/// The new-slot arm ranges over every available package, not just the
/// scheduled entries: with no provider entry it still finds the move.
/// Portage: depgraph.py, _slot_operator_update_probe/_backtrack
#[test]
fn slot_operator_rebuild_scan_newslot_arm_fires_without_a_scheduled_provider() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-269").keep();
    // vdb: provider instances in both slots, consumer bound to slot 0.
    for (name, slot, rdepend) in [
        ("mmprov-1", "0/1", ""),
        ("mmprov-2", "1/1", ""),
        ("mmcons-1", "0", "app-misc/mmprov:0/1="),
    ] {
        let d = base.join("var/db/pkg/app-misc").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-misc\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    // Tree: `mmprov-3` at slot 1/2, plus the consumer's own ebuild
    // (the replacement parent must exist).
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("app-misc").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"269 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=269 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/app-misc");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("mmprov", "3", "1/2");
    write_pkg("mmcons", "1", "0");
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo.clone(),
        priority: 0,
        is_main: true,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    // No provider entry at all (Portage's pass 0 schedules none either):
    // the consumer sits in the graph as walked/AlreadyInstalled.
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-misc", "mmcons", "1")
    };
    let entries = vec![consumer.clone()];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let mmcons = ("app-misc".to_string(), "mmcons".to_string());
    let mmprov = ("app-misc".to_string(), "mmprov".to_string());
    // Only `mmcons` requested: the greedy pin stays out.
    let top = HashSet::from([mmcons.clone()]);
    let top_atoms = HashSet::from(["app-misc/mmcons".to_string()]);
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &top,
        top_level_atoms: &top_atoms,
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([mmcons.clone()]),
        "the available slot move rebuilds the consumer"
    );
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    // Provider requested too: still probes nothing.
    let mut both = top.clone();
    both.insert(mmprov.clone());
    let both_atoms = HashSet::from(["app-misc/mmcons".to_string(), "app-misc/mmprov".to_string()]);
    let (greedy_pinned, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &base,
        entries: &entries,
        reachable: &HashSet::new(),
        scope: None,
        already: &empty,
        undone: &empty,
        with_bdeps: true,
        update: true,
        top_level_cps: &both,
        top_level_atoms: &both_atoms,
        excluded: &[],
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(
        greedy_pinned.is_empty(),
        "a requested provider below the highest slot probes nothing"
    );
    let _ = fs::remove_dir_all(&base);
}

// ---- `--rebuild-if-*` restart scan (backlog #394): over the reached set
// (merges and kept-installed nodes) name the installed parents whose
// walked buildtime edge lands on a dep this run merges. Portage records
// those edges in `_rebuild_config.add` and restarts the walk with the
// parents force-reinstalled (`depgraph.py:424-442`, `:531-576`,
// `:5692-5698`, outer loop `:12235`); the re-walk prints the `rR` row
// (oracle `LLM/evidence/2026-10-19-394/out/portage-a-unbuilt-deep.txt`:
// `U rebuildtrigger-2.0 [1.0]` + `rR rebuildconsumer-1.0` + `N r394top-1`,
// backtrack 1/20). ----

/// vdb with installed `rebuildtrigger-1.0` + `rebuildconsumer-1.0` (the
/// `#394` oracle shape; the consumer's walked edge lives on its entry's
/// `deps`, like the walk records it).
fn rebuild_if_394_vdb(tag: &str) -> PathBuf {
    let dir = TempDir::new(&format!("portage-repo-rebuild-if-{tag}")).keep();
    for name in ["rebuildtrigger-1.0", "rebuildconsumer-1.0"] {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
    }
    dir
}

/// One walked edge from the consumer to the trigger; `buildtime` picks the
/// `DEPEND`/`BDEPEND` key (Portage `DepPriority.buildtime`).
fn rebuild_if_394_edge(buildtime: bool) -> DepEdge {
    DepEdge {
        atom: "dev-libs/rebuildtrigger".to_string(),
        evaluated: "dev-libs/rebuildtrigger".to_string(),
        category: "dev-libs".to_string(),
        package: "rebuildtrigger".to_string(),
        priority: DepPriority {
            buildtime,
            runtime: !buildtime,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: if buildtime { 3 } else { 0 },
    }
}

/// The kept-installed consumer node, carrying `edge` as its walked dep.
fn rebuild_if_394_consumer(edge: DepEdge) -> GraphEntry {
    GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        },
        deps: vec![edge],
        ..graph_entry("dev-libs", "rebuildconsumer", "1.0")
    }
}

/// The trigger merged this run (2.0 over installed 1.0), from `source`.
fn rebuild_if_394_trigger(source: CandidateSource) -> GraphEntry {
    GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "2.0".to_string(),
        },
        source,
        ..graph_entry("dev-libs", "rebuildtrigger", "2.0")
    }
}

/// Which `--rebuild-if-*` flag the scan runs under.
#[derive(Clone, Copy)]
enum RebuildIf394Flag {
    Unbuilt,
    NewRev,
    NewVer,
    None,
}

fn rebuild_if_394_scan(
    root: &Path,
    entries: &[GraphEntry],
    flag: RebuildIf394Flag,
    exclude: &[String],
    ignore: &[String],
) -> BTreeSet<(String, String)> {
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    rebuild_if_restart_parents(RebuildIfRestartParentsArgs {
        root,
        entries,
        already: &empty,
        unbuilt: matches!(flag, RebuildIf394Flag::Unbuilt),
        new_rev: matches!(flag, RebuildIf394Flag::NewRev),
        new_ver: matches!(flag, RebuildIf394Flag::NewVer),
        rebuild_exclude: exclude,
        rebuild_ignore: ignore,
    })
}

#[test]
fn rebuild_if_restart_names_a_reached_parent_behind_a_merged_build_dep() {
    // The #394 oracle shape: the trigger upgrades, the consumer is kept but
    // walked with a buildtime edge to it. Portage restarts and prints the
    // consumer `rR`.
    let dir = rebuild_if_394_vdb("reached");
    let entries = vec![
        rebuild_if_394_trigger(CandidateSource::Ebuild),
        rebuild_if_394_consumer(rebuild_if_394_edge(true)),
    ];
    for flag in [
        RebuildIf394Flag::Unbuilt,
        RebuildIf394Flag::NewRev,
        RebuildIf394Flag::NewVer,
    ] {
        assert_eq!(
            rebuild_if_394_scan(&dir, &entries, flag, &[], &[]),
            BTreeSet::from([("dev-libs".to_string(), "rebuildconsumer".to_string())]),
            "all three flags name the reached consumer"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rebuild_if_restart_ignores_an_unreached_parent() {
    // S2 shape: the consumer is installed but nothing reached it, so it has
    // no entry. Portage records no edge and prints no row.
    let dir = rebuild_if_394_vdb("unreached");
    let entries = vec![rebuild_if_394_trigger(CandidateSource::Ebuild)];
    assert!(rebuild_if_394_scan(&dir, &entries, RebuildIf394Flag::Unbuilt, &[], &[]).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rebuild_if_restart_ignores_a_runtime_only_edge() {
    // `RDEPEND`/`PDEPEND` never qualify (Portage `add`'s
    // `priority.buildtime` gate).
    let dir = rebuild_if_394_vdb("runtime");
    let entries = vec![
        rebuild_if_394_trigger(CandidateSource::Ebuild),
        rebuild_if_394_consumer(rebuild_if_394_edge(false)),
    ];
    assert!(
        rebuild_if_394_scan(&dir, &entries, RebuildIf394Flag::Unbuilt, &[], &[]).is_empty(),
        "a runtime-only edge never rebuilds"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rebuild_if_restart_splits_new_rev_from_new_ver() {
    // Merged `2.0-r1` over installed `2.0`: new-rev fires (cpv not
    // installed), new-ver does not (norev equal). Merged `2.0` over
    // installed `1.0`: both fire.
    let dir = TempDir::new("portage-repo-rebuild-if-rev").keep();
    for name in ["rebuildtrigger-2.0", "rebuildconsumer-1.0"] {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
    }
    let rev_bump = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2.0".to_string(),
            to: "2.0-r1".to_string(),
        },
        ..graph_entry("dev-libs", "rebuildtrigger", "2.0-r1")
    };
    let entries = vec![rev_bump, rebuild_if_394_consumer(rebuild_if_394_edge(true))];
    let consumer = BTreeSet::from([("dev-libs".to_string(), "rebuildconsumer".to_string())]);
    assert_eq!(
        rebuild_if_394_scan(&dir, &entries, RebuildIf394Flag::NewRev, &[], &[]),
        consumer,
        "new-rev fires for a revision bump"
    );
    assert!(
        rebuild_if_394_scan(&dir, &entries, RebuildIf394Flag::NewVer, &[], &[]).is_empty(),
        "new-ver ignores a revision-only bump"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rebuild_if_restart_honours_rebuild_exclude_and_ignore() {
    // `--rebuild-exclude` matches the parent, `--rebuild-ignore` the dep.
    let dir = rebuild_if_394_vdb("gates");
    let entries = vec![
        rebuild_if_394_trigger(CandidateSource::Ebuild),
        rebuild_if_394_consumer(rebuild_if_394_edge(true)),
    ];
    assert!(
        rebuild_if_394_scan(
            &dir,
            &entries,
            RebuildIf394Flag::Unbuilt,
            &["dev-libs/rebuildconsumer".to_string()],
            &[]
        )
        .is_empty(),
        "excluded parents never restart"
    );
    assert!(
        rebuild_if_394_scan(
            &dir,
            &entries,
            RebuildIf394Flag::Unbuilt,
            &[],
            &["dev-libs/rebuildtrigger".to_string()]
        )
        .is_empty(),
        "ignored deps never trigger"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rebuild_if_restart_ignores_a_binary_merge_and_a_merging_parent() {
    // A dep merged from a binary is `built` (Portage `built=(type !=
    // "ebuild")`), so it never triggers. A parent that already merges needs
    // no rebuild either (Portage only records edges for built parents).
    let dir = rebuild_if_394_vdb("source");
    let binary_entries = vec![
        rebuild_if_394_trigger(CandidateSource::Binary),
        rebuild_if_394_consumer(rebuild_if_394_edge(true)),
    ];
    assert!(
        rebuild_if_394_scan(&dir, &binary_entries, RebuildIf394Flag::Unbuilt, &[], &[]).is_empty(),
        "a binary dep never triggers"
    );
    let merging_consumer = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".to_string(),
            to: "1.1".to_string(),
        },
        deps: vec![rebuild_if_394_edge(true)],
        ..graph_entry("dev-libs", "rebuildconsumer", "1.1")
    };
    let merging_entries = vec![
        rebuild_if_394_trigger(CandidateSource::Ebuild),
        merging_consumer,
    ];
    assert!(
        rebuild_if_394_scan(&dir, &merging_entries, RebuildIf394Flag::Unbuilt, &[], &[]).is_empty(),
        "an already-merging parent is never named"
    );
    // No flag at all: the scan stays off.
    let plain_entries = vec![
        rebuild_if_394_trigger(CandidateSource::Ebuild),
        rebuild_if_394_consumer(rebuild_if_394_edge(true)),
    ];
    assert!(rebuild_if_394_scan(&dir, &plain_entries, RebuildIf394Flag::None, &[], &[]).is_empty());
    // An already-seeded parent is stable: no second restart.
    let seeded: BTreeSet<(String, String)> =
        BTreeSet::from([("dev-libs".to_string(), "rebuildconsumer".to_string())]);
    assert!(
        rebuild_if_restart_parents(RebuildIfRestartParentsArgs {
            root: &dir,
            entries: &plain_entries,
            already: &seeded,
            unbuilt: true,
            new_rev: false,
            new_ver: false,
            rebuild_exclude: &[],
            rebuild_ignore: &[],
        })
        .is_empty(),
        "a seeded parent never re-fires"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// `--deep` scans only what the pass walked; Portage probes its graph, never
/// an installed closure the new versions no longer pull in.
#[test]
fn deep_scan_scope_is_the_walked_set_narrowed_by_the_closure() {
    let cp = |n: &str| ("dev-libs".to_string(), n.to_string());
    let entries = vec![graph_entry("dev-libs", "walked", "1.0")];
    let closure = HashSet::from([cp("walked"), cp("orphan")]);
    assert_eq!(
        crate::resolve::scan_scope(Deep::Unlimited, &closure, &entries),
        HashSet::from([cp("walked")])
    );
    assert_eq!(
        crate::resolve::scan_scope(Deep::Unlimited, &HashSet::new(), &entries),
        HashSet::from([cp("walked")])
    );
    assert_eq!(
        crate::resolve::scan_scope(Deep::NotRequested, &closure, &entries),
        closure
    );
}
