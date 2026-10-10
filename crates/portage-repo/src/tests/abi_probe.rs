use super::*;

/// Backlog #214 (v2 `#24e`): the conflict-fired probe schedules the
/// installed `:=` parent for rebuild instead of reporting, with one
/// guard arm per coded gate.
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
        let parents = collect_probe_parents(
            &base,
            entries,
            &empty_reach,
            &walked,
            &in_graph,
            already,
            true,
            excluded,
        );
        slot_conflict_abi_probe(
            &base,
            &repos,
            entries,
            conflicts,
            already,
            &BTreeSet::new(),
            true,
            &walked,
            excluded,
            &parents,
            &test_config(),
        )
    };
    let abicons = ("app-misc".to_string(), "abicons".to_string());
    let entries = abi_probe_entries(installed_consumer.clone());
    // Positive: the installed `:=` parent rebuilds.
    assert_eq!(
        probe(
            &entries,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &[]
        ),
        BTreeSet::from([abicons.clone()])
    );
    // Merge-bound parent rebinds through its own walk: no probe.
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
    // Excluded parent: real's `excluded_pkgs` skip.
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
    // Non-built atom: a plain version pin is a real conflict, not an
    // ABI rebuild (real's `slot_operator_built` gate).
    let plain = abi_conflict_record(
        vec![(force_cpv, ">=app-misc/abiprov-2")],
        vec![(cons_cpv, "=app-misc/abiprov-1")],
    );
    assert!(
        probe(&entries, &[plain], &BTreeSet::new(), &[]).is_empty(),
        "a non-slot-operator pin reports instead of rebuilding"
    );
    // `(Argument)` parent: not a package, real skips it.
    let arg_only = abi_conflict_record(vec![], vec![("", "app-misc/abiprov:0/1=")]);
    assert!(
        probe(&entries, &[arg_only], &BTreeSet::new(), &[]).is_empty(),
        "a top-level atom schedules no rebuild"
    );
    // Same sub-slot on both instances: no ABI shift to chase.
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
    // Downgrade direction: the other instance is older.
    let mut older_other = record.clone();
    older_other.instances[0].version = "0".to_string();
    assert!(
        probe(&entries, &[older_other], &BTreeSet::new(), &[]).is_empty(),
        "an older other-instance is never the update target"
    );
    // No same-version ebuild for the parent: the forced reinstall
    // would dead-end.
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
    let bare_parents = collect_probe_parents(
        &bare_base,
        &entries,
        &HashSet::new(),
        &walked,
        &in_graph,
        &BTreeSet::new(),
        true,
        &[],
    );
    assert!(
        slot_conflict_abi_probe(
            &bare_base,
            &bare_repos,
            &entries,
            std::slice::from_ref(&record),
            &BTreeSet::new(),
            &BTreeSet::new(),
            true,
            &walked,
            &[],
            &bare_parents,
            &test_config(),
        )
        .is_empty(),
        "without a visible replacement the probe does nothing"
    );
    // Veto: another installed parent rejects the fresh candidate
    // (the R2 refusal, same gate as the scan).
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
    // #254: the replacement's `:=` under a USE conditional that is off
    // in the tree ebuild. Real's `_select_atoms_probe`
    // (`_emerge/depgraph.py:2667-2713`) evaluates it against the
    // replacement's USE, so the atom is not selected and the probe
    // refuses; a flat token match would accept it.
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
        let parents = collect_probe_parents(
            &cond_base,
            &cond_entries,
            &HashSet::new(),
            &walked,
            &HashSet::new(),
            &BTreeSet::new(),
            true,
            &[],
        );
        assert!(
            slot_conflict_abi_probe(
                &cond_base,
                &cond_repos,
                &cond_entries,
                std::slice::from_ref(&record),
                &BTreeSet::new(),
                &BTreeSet::new(),
                true,
                &walked,
                &[],
                &parents,
                &test_config(),
            )
            .is_empty(),
            "a conditional `:=` whose flag is off is not selected"
        );
        let _ = fs::remove_dir_all(&cond_base);
    }
    let _ = fs::remove_dir_all(&base);
    let _ = fs::remove_dir_all(&bare_base);
}

/// Backlog #214 (v2 `#24e`): the settle-time display continuity --
/// a `slot_operator_rebuild` reinstall re-derives its
/// `(provider-cpv, consumer-cpv)` pair from the recorded built atom
/// and the merged provider, so the `causing rebuilds` block renders
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
        // #254: the rebuilt consumer's own walk pulls the provider
        // (real pairs only a child of the replacement).
        required_by: vec![("app-misc".to_string(), "abicons".to_string())],
        ..graph_entry("app-misc", "abiprov", "2")
    };
    let set: BTreeSet<(String, String)> =
        BTreeSet::from([("app-misc".to_string(), "abicons".to_string())]);
    // Positive: the rebuilt consumer re-derives its provider pair.
    assert_eq!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), provider.clone()], &set, true),
        vec![(
            "app-misc/abiprov-2".to_string(),
            "app-misc/abicons-1".to_string()
        )]
    );
    // #254: the rebuilt consumer does not pull the provider (its new
    // build dropped the `:=`, e.g. a USE conditional now off): real's
    // `_compute_abi_rebuild_info` finds no child, so no pair.
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

/// Backlog #288: an argument atom is a probe parent of its own cp and
/// vetoes a provider candidate it does not match (real
/// `_slot_operator_check_reverse_dependencies` over `_parent_atoms`).
#[test]
fn argument_probe_parents_veto_a_candidate_the_argument_does_not_match() {
    let mut parents: HashMap<(String, String), Vec<ProbeParent>> = HashMap::new();
    let atoms: HashSet<String> = ["=app-misc/mmprov-1", "!app-misc/blk", "app-misc/mmcons"]
        .iter()
        .map(|a| (*a).to_string())
        .collect();
    add_argument_probe_parents(&mut parents, &atoms);
    let provider = ("app-misc".to_string(), "mmprov".to_string());
    let consumer = ("app-misc".to_string(), "mmcons".to_string());
    assert!(
        !parents.contains_key(&("app-misc".to_string(), "blk".to_string())),
        "a blocker argument is not a parent"
    );
    let scheduled = BTreeSet::new();
    assert!(probe_refused(
        &parents,
        &scheduled,
        &provider,
        "app-misc/mmprov-3:1/2",
        &consumer
    ));
    assert!(!probe_refused(
        &parents,
        &scheduled,
        &provider,
        "app-misc/mmprov-1:0/1",
        &consumer
    ));
}
