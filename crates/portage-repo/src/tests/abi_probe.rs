use super::*;

/// The conflict-fired probe rebuilds the installed `:=` parent instead of
/// reporting, with one guard arm per gate.
#[test]
fn slot_conflict_abi_probe_rebuilds_the_built_parent() {
    let (base, repos) = abi_probe_harness(true);
    let cons_cpv = "app-misc/abicons-1:0/0::testrepo";
    let force_cpv = "app-misc/abiforce-1:0/0::testrepo";
    let record = abi_conflict_record(
        vec![
            (force_cpv, ">=app-misc/abiprov-2"),
            (cons_cpv, "app-misc/abiprov:="),
        ],
        vec![(cons_cpv, "app-misc/abiprov:0/1=")],
    );
    let installed_consumer = PretendOutcome::AlreadyInstalled {
        version: "1".to_string(),
    };
    let probe = |entries: &[GraphEntry],
                 conflicts: &[SlotConflict],
                 already: &BTreeSet<(String, String)>,
                 excluded: &[String]|
     -> BTreeSet<(String, String)> {
        let walked: HashSet<(String, String)> = entries
            .iter()
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let mut in_graph: HashSet<(String, String)> = HashSet::new();
        for e in entries {
            if !matches!(
                e.outcome,
                PretendOutcome::AlreadyInstalled { .. }
                    | PretendOutcome::NoVisibleCandidate
                    | PretendOutcome::Uninstall { .. }
            ) {
                in_graph.insert((e.category.clone(), e.package.clone()));
            }
        }
        let empty_reach: HashSet<(String, String)> = HashSet::new();
        let parents = collect_probe_parents(CollectProbeParentsArgs {
            root: &base,
            entries,
            reachable: &empty_reach,
            walked: &walked,
            in_graph: &in_graph,
            already,
            with_bdeps: true,
            excluded,
        });
        slot_conflict_abi_probe(SlotConflictAbiProbeArgs {
            root: &base,
            repos: &repos,
            entries,
            conflicts,
            already,
            undone: &BTreeSet::new(),
            with_bdeps: true,
            walked: &walked,
            excluded,
            probe_parents: &parents,
            config: &test_config(),
        })
    };
    let abicons = ("app-misc".to_string(), "abicons".to_string());
    let entries = abi_probe_entries(installed_consumer.clone());
    assert_eq!(
        probe(
            &entries,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &[]
        ),
        BTreeSet::from([abicons.clone()])
    );
    let merging = abi_probe_entries(PretendOutcome::New {
        version: "1".to_string(),
    });
    assert!(
        probe(
            &merging,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &[]
        )
        .is_empty(),
        "a merging parent needs no reinstall"
    );
    // Already scheduled: no growth.
    assert!(
        probe(
            &entries,
            std::slice::from_ref(&record),
            &BTreeSet::from([abicons.clone()]),
            &[],
        )
        .is_empty(),
        "an in-flight rebuild is not scheduled twice"
    );
    // Excluded parent is never force-rebuilt (Portage's `excluded_pkgs` skip).
    assert!(
        probe(
            &entries,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &["app-misc/abicons".to_string()]
        )
        .is_empty(),
        "an excluded parent is never force-rebuilt"
    );
    // Plain version pin is a conflict, not a rebuild (Portage's `slot_operator_built` gate).
    let plain = abi_conflict_record(
        vec![(force_cpv, ">=app-misc/abiprov-2")],
        vec![(cons_cpv, "=app-misc/abiprov-1")],
    );
    assert!(
        probe(&entries, &[plain], &BTreeSet::new(), &[]).is_empty(),
        "a non-slot-operator pin reports instead of rebuilding"
    );
    // `(Argument)` parent is not a package; Portage skips it.
    let arg_only = abi_conflict_record(vec![], vec![("", "app-misc/abiprov:0/1=")]);
    assert!(
        probe(&entries, &[arg_only], &BTreeSet::new(), &[]).is_empty(),
        "a top-level atom schedules no rebuild"
    );
    // Same sub-slot on both instances: no ABI shift.
    let mut same_sub = record.clone();
    same_sub.instances[1].sub_slot = "2".to_string();
    same_sub.instances[1].parents = vec![SlotConflictParent {
        parent_cpv: cons_cpv.to_string(),
        atom: "app-misc/abiprov:0/9=".to_string(),
        use_display: Vec::new(),
        installed: false,
    }];
    assert!(
        probe(&entries, &[same_sub], &BTreeSet::new(), &[]).is_empty(),
        "identical sub-slots are not an ABI update"
    );
    // Downgrade direction: never the update target.
    let mut older_other = record.clone();
    older_other.instances[0].version = "0".to_string();
    assert!(
        probe(&entries, &[older_other], &BTreeSet::new(), &[]).is_empty(),
        "an older other-instance is never the update target"
    );
    // No same-version ebuild: a forced reinstall would dead-end.
    let (bare_base, bare_repos) = abi_probe_harness(false);
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in &entries {
        if !matches!(e.outcome, PretendOutcome::AlreadyInstalled { .. }) {
            in_graph.insert((e.category.clone(), e.package.clone()));
        }
    }
    let bare_parents = collect_probe_parents(CollectProbeParentsArgs {
        root: &bare_base,
        entries: &entries,
        reachable: &HashSet::new(),
        walked: &walked,
        in_graph: &in_graph,
        already: &BTreeSet::new(),
        with_bdeps: true,
        excluded: &[],
    });
    assert!(
        slot_conflict_abi_probe(SlotConflictAbiProbeArgs {
            root: &bare_base,
            repos: &bare_repos,
            entries: &entries,
            conflicts: std::slice::from_ref(&record),
            already: &BTreeSet::new(),
            undone: &BTreeSet::new(),
            with_bdeps: true,
            walked: &walked,
            excluded: &[],
            probe_parents: &bare_parents,
            config: &test_config()
        })
        .is_empty(),
        "without a visible replacement the probe does nothing"
    );
    // Veto: another installed parent rejects the candidate.
    let veto_dir = base.join("var/db/pkg/app-misc").join("abiveto-1");
    fs::create_dir_all(&veto_dir).unwrap();
    fs::write(veto_dir.join("CATEGORY"), "app-misc\n").unwrap();
    fs::write(veto_dir.join("SLOT"), "0\n").unwrap();
    fs::write(veto_dir.join("repository"), "testrepo\n").unwrap();
    fs::write(veto_dir.join("RDEPEND"), "<app-misc/abiprov-2\n").unwrap();
    let mut vetoed = entries.clone();
    vetoed.push(GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1".to_string(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-misc", "abiveto", "1")
    });
    assert!(
        probe(
            &vetoed,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &[]
        )
        .is_empty(),
        "a vetoed candidate refuses the whole replacement"
    );
    // Portage evaluates the `:=` against the replacement's USE, so an off-flag
    // conditional refuses where a flat token match would accept.
    {
        use md5::Digest as _;
        let (cond_base, cond_repos) = abi_probe_harness(true);
        let body = "EAPI=8\nDESCRIPTION=\"214 abi\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nIUSE=\"cflag\"\nRDEPEND=\"cflag? ( app-misc/abiprov:= )\"\n";
        let dir = cond_base.join("repo/app-misc/abicons");
        fs::write(dir.join("abicons-1.ebuild"), body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        fs::write(
            cond_base.join("repo/metadata/md5-cache/app-misc/abicons-1"),
            format!(
                "DEFINED_PHASES=-\nDESCRIPTION=214 abi\nEAPI=8\nIUSE=cflag\nKEYWORDS=amd64\nRDEPEND=cflag? ( app-misc/abiprov:= )\nSLOT=0\n_md5_={md5}\n"
            ),
        )
        .unwrap();
        let cond_entries = abi_probe_entries(installed_consumer.clone());
        let walked: HashSet<(String, String)> = cond_entries
            .iter()
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let parents = collect_probe_parents(CollectProbeParentsArgs {
            root: &cond_base,
            entries: &cond_entries,
            reachable: &HashSet::new(),
            walked: &walked,
            in_graph: &HashSet::new(),
            already: &BTreeSet::new(),
            with_bdeps: true,
            excluded: &[],
        });
        assert!(
            slot_conflict_abi_probe(SlotConflictAbiProbeArgs {
                root: &cond_base,
                repos: &cond_repos,
                entries: &cond_entries,
                conflicts: std::slice::from_ref(&record),
                already: &BTreeSet::new(),
                undone: &BTreeSet::new(),
                with_bdeps: true,
                walked: &walked,
                excluded: &[],
                probe_parents: &parents,
                config: &test_config()
            })
            .is_empty(),
            "a conditional `:=` whose flag is off is not selected"
        );
        let _ = fs::remove_dir_all(&cond_base);
    }
    let _ = fs::remove_dir_all(&base);
    let _ = fs::remove_dir_all(&bare_base);
}

/// Settle-time display continuity: a `slot_operator_rebuild` reinstall re-derives
/// its pair from the built atom and the merged provider, so the block renders
/// even though the firing pass restarted.
#[test]
fn slot_conflict_abi_display_pairs_follow_the_rebuild() {
    let (base, _repos) = abi_probe_harness(true);
    let reinstall = |slot_operator_rebuild: bool| GraphEntry {
        outcome: PretendOutcome::Reinstall {
            version: "1".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-misc", "abicons", "1")
    };
    let provider = GraphEntry {
        outcome: PretendOutcome::New {
            version: "2".to_string(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        // Only a child of the replacement pairs (Portage).
        required_by: vec![("app-misc".to_string(), "abicons".to_string())],
        ..graph_entry("app-misc", "abiprov", "2")
    };
    let set: BTreeSet<(String, String)> =
        BTreeSet::from([("app-misc".to_string(), "abicons".to_string())]);
    // Rebuilt consumer re-derives its pair.
    assert_eq!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), provider.clone()], &set, true),
        vec![(
            "app-misc/abiprov-2".to_string(),
            "app-misc/abicons-1".to_string()
        )]
    );
    // Provider no longer pulled (USE conditional now off): no pair.
    let unpulled = GraphEntry {
        required_by: Vec::new(),
        ..provider.clone()
    };
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), unpulled], &set, true).is_empty(),
        "a provider the rebuilt consumer no longer pulls is not paired"
    );
    // Not a slot-operator rebuild: no pair.
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(false), provider.clone()], &set, true)
            .is_empty(),
        "an ordinary reinstall renders no ABI pair"
    );
    // Consumer not in the replace set: no pair.
    assert!(
        slot_conflict_abi_display_pairs(
            &base,
            &[reinstall(true), provider.clone()],
            &BTreeSet::new(),
            true
        )
        .is_empty(),
        "only replace-set rebuilds explain themselves"
    );
    // Provider at the bound sub-slot: no shift, no pair.
    let same_sub = GraphEntry {
        sub_slot: Some("1".into()),
        ..provider.clone()
    };
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), same_sub], &set, true).is_empty(),
        "an unshifted provider is not an ABI cause"
    );
    let _ = fs::remove_dir_all(&base);
}
