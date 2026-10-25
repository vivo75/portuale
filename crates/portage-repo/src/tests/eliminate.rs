use super::*;

/// The bridge fixpoint runs the new-slot update arm, so both solvers agree (--update
/// schedules the walked consumer; without it the arm stays off).
#[test]
fn slot_operator_bridge_entries_run_the_new_slot_arm() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let dir = TempDir::new("portage-repo-slotop-bridge-newslot").keep();
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
    ] {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = dir.join("repo");
    for (pkg, pv, slot) in [("massb", "2.0", "2/2"), ("massc", "1.0", "0")] {
        let d = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&d).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"211 bridge\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(d.join(format!("{pkg}-{pv}.ebuild")), body.as_bytes()).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 bridge\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
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
    let entries = [fresh, consumer];
    // With `--update` the bridge synthesises the consumer rebuild.
    let (out, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &repos,
        entries: &entries,
        reachable: &HashSet::new(),
        with_bdeps: true,
        excluded: &[],
        update: true,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    let names: Vec<&str> = out.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["massc"]);
    assert!(matches!(
        out[0].outcome,
        PretendOutcome::Reinstall {
            slot_operator_rebuild: true,
            ..
        }
    ));
    // Without `--update` the arm stays off.
    let (off, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &repos,
        entries: &entries,
        reachable: &HashSet::new(),
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    assert!(off.is_empty(), "no want_update, no schedule");
    let _ = fs::remove_dir_all(&dir);
}

/// The replace-cp lookup matches the cp being decided: a decoy and a foreign pin
/// never satisfy it.
#[test]
fn slot_operator_eliminate_rebuilds_lookup_matches_the_replace_cp() {
    let dir = slotundo_temp_dir("161-elim");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "sounneed",
        version: "1.0",
        slot: "0/1",
        rdepend: "soflag? ( dev-libs/souprov:0/1= )",
        installed_use: "",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souprov",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    // Same-category decoy sorting first: the exact lookup walks past it.
    let decoy = dir.join("var/db/pkg/dev-libs/aadecoy-9.9");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(decoy.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(decoy.join("SLOT"), "0\n").unwrap();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let provider = slotundo_provider_entry();
    let entry = slotundo_rebuild_entry("sounneed", "1.0", "0", "1", &[("soflag", false)]);
    let sounneed = ("dev-libs".to_string(), "sounneed".to_string());
    let other_pin = RevDepPin {
        cp: ("app-arch".to_string(), "aadecoy".to_string()),
        atom: "=app-arch/aadecoy-1.0".to_string(),
        raw_atom: "=app-arch/aadecoy-1.0".to_string(),
        built_render: Vec::new(),
        consumer: (
            "app-arch".to_string(),
            "aadecoy".to_string(),
            "9.9".to_string(),
        ),
    };
    let demoted = slot_operator_eliminate_rebuilds(SlotOperatorEliminateRebuildsArgs {
        root: &dir,
        repos: &repos,
        entries: &[provider, entry],
        replace: &BTreeSet::from([sounneed.clone()]),
        slot_want: &HashMap::from([(sounneed.clone(), vec!["dev-libs/sounneed".to_string()])]),
        reverse_pins: &[other_pin],
        selective: false,
        changed_slot: false,
        top_level_cps: &HashSet::new(),
        empty: false,
    });
    assert_eq!(
        demoted,
        BTreeSet::from([sounneed]),
        "rule 8 demotes with a decoy installed and a foreign pin present"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The synthesiser entry lands at the tree ebuild's slot, with no `[oldver]`
/// bracket on agreement, and records the vdb repository.
#[test]
fn slot_operator_rebuild_entries_use_the_consumer_tree_slot_and_repo() {
    let dir = slotundo_temp_dir("161-entries");
    let d = dir.join("var/db/pkg/dev-libs/souprov-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
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
    let reach: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "souprov".to_string())]);
    let (out, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &repos,
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].slot.as_deref(), Some("0"));
    assert_eq!(out[0].sub_slot.as_deref(), Some("1"));
    assert!(out[0].oldbest.is_empty(), "same slot: no [oldver] bracket");
    assert_eq!(out[0].repo_name.as_deref(), Some("testrepo"));
    let _ = fs::remove_dir_all(&dir);
}

/// A repo-drifted rebuild still gets the `[oldver]` bracket while the entry keeps
/// the tree repo.
#[test]
fn slot_operator_rebuild_entries_mark_repo_drifted_rebuilds_oldbest() {
    let dir = slotundo_temp_dir("274-entries");
    let d = dir.join("var/db/pkg/dev-libs/souprov-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    fs::write(d.join("repository"), "oldrepo\n").unwrap();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
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
    let reach: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "souprov".to_string())]);
    let (out, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &repos,
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].slot.as_deref(), Some("0"));
    assert_eq!(out[0].sub_slot.as_deref(), Some("1"));
    assert_eq!(out[0].repo_name.as_deref(), Some("testrepo"));
    assert_eq!(
        out[0].oldbest,
        vec![InstalledRef {
            version: "1.0".into(),
            slot: "0".into(),
            sub_slot: "1".into(),
            repo: "oldrepo".into(),
        }],
        "same slot/sub-slot, drifted repo: the installed instance is oldbest"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order() {
    // Flag off, so both rule-8 sides use-reduce the conditional away.
    let dir = slotundo_temp_dir("undo");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "sounneed",
        version: "1.0",
        slot: "0/1",
        rdepend: "soflag? ( dev-libs/souprov:0/1= )",
        installed_use: "",
    });
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souprov",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let provider = slotundo_provider_entry();
    let sounneed = ("dev-libs".to_string(), "sounneed".to_string());
    let replace = BTreeSet::from([sounneed.clone()]);
    let slot_want = HashMap::from([(sounneed.clone(), vec!["dev-libs/sounneed".to_string()])]);
    let no_pins: Vec<RevDepPin> = Vec::new();
    let no_top: HashSet<(String, String)> = HashSet::new();

    let undo = |entries: &[GraphEntry],
                selective: bool,
                changed_slot: bool,
                top: &HashSet<(String, String)>,
                empty: bool| {
        slot_operator_eliminate_rebuilds(SlotOperatorEliminateRebuildsArgs {
            root: &dir,
            repos: &repos,
            entries,
            replace: &replace,
            slot_want: &slot_want,
            reverse_pins: &no_pins,
            selective,
            changed_slot,
            top_level_cps: top,
            empty,
        })
    };

    // Rule 8 flip: both sides use-reduce the `soflag?` group away.
    let entry = slotundo_rebuild_entry("sounneed", "1.0", "0", "1", &[("soflag", false)]);
    let entries = [provider.clone(), entry.clone()];
    assert_eq!(undo(&entries, false, false, &no_top, false), replace);
    // Flag on: the tree `:=` binds to the graph's 0/2, so the rebuild is kept.
    let entry_on = slotundo_rebuild_entry("sounneed", "1.0", "0", "1", &[("soflag", true)]);
    assert!(undo(&[provider.clone(), entry_on], false, false, &no_top, false).is_empty());

    // Rule 6: an own-slot move keeps the rebuild.
    let entry_slot = slotundo_rebuild_entry("sounneed", "1.0", "0", "2", &[("soflag", false)]);
    assert!(
        undo(
            &[provider.clone(), entry_slot],
            false,
            false,
            &no_top,
            false
        )
        .is_empty()
    );

    // Rule 3: `--changed-slot` only fires when the tree ebuild moved; here it
    // did not, so the rule-8 demotion stands.
    assert_eq!(
        undo(&entries, false, true, &no_top, false),
        replace,
        "rule 3 must not keep a rebuild whose tree ebuild did not move"
    );
    // Positive shape (vdb 0/1, tree 0/2): kept with and without the flag.
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souneedslot",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    let entry_slotmove = slotundo_rebuild_entry("souneedslot", "1.0", "0", "2", &[]);
    let want_slotmove = HashMap::from([(
        ("dev-libs".to_string(), "souneedslot".to_string()),
        vec!["dev-libs/souneedslot".to_string()],
    )]);
    let replace_slotmove = BTreeSet::from([("dev-libs".to_string(), "souneedslot".to_string())]);
    let rule3 = |changed_slot: bool| {
        slot_operator_eliminate_rebuilds(SlotOperatorEliminateRebuildsArgs {
            root: &dir,
            repos: &repos,
            entries: &[provider.clone(), entry_slotmove.clone()],
            replace: &replace_slotmove,
            slot_want: &want_slotmove,
            reverse_pins: &no_pins,
            selective: false,
            changed_slot,
            top_level_cps: &no_top,
            empty: false,
        })
    };
    assert!(
        rule3(true).is_empty() && rule3(false).is_empty(),
        "a slot-moved ebuild consumer is kept with and without --changed-slot"
    );

    // Rule 1: a different installed version is an upgrade.
    let entry_v2 = slotundo_rebuild_entry("sounneed", "2.0", "0", "1", &[("soflag", false)]);
    assert!(undo(&[provider.clone(), entry_v2], false, false, &no_top, false).is_empty());

    // Rule 2: --newuse/--changed-use wins.
    let mut entry_flags = entry.clone();
    entry_flags.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: vec!["soflag".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: true,
    };
    assert!(
        undo(
            &[provider.clone(), entry_flags],
            false,
            false,
            &no_top,
            false
        )
        .is_empty()
    );

    // Rule 4: a parent atom missing the installed instance.
    let bad_want = HashMap::from([(sounneed.clone(), vec!["=dev-libs/sounneed-2.0".to_string()])]);
    let demoted = slot_operator_eliminate_rebuilds(SlotOperatorEliminateRebuildsArgs {
        root: &dir,
        repos: &repos,
        entries: &[provider.clone(), entry.clone()],
        replace: &replace,
        slot_want: &bad_want,
        reverse_pins: &no_pins,
        selective: false,
        changed_slot: false,
        top_level_cps: &no_top,
        empty: false,
    });
    assert!(demoted.is_empty());

    // Rule 5: non-selective keeps a user-requested package.
    let top = HashSet::from([sounneed.clone()]);
    assert!(
        undo(
            &[provider.clone(), entry.clone()],
            false,
            false,
            &top,
            false
        )
        .is_empty()
    );
    // Selective ignores the argument (Portage's `selective` arm).
    assert_eq!(
        undo(&[provider.clone(), entry.clone()], true, false, &top, false),
        replace
    );

    // Rule 7: a binary entry keeps its rebuild.
    let mut entry_binary = entry.clone();
    entry_binary.source = CandidateSource::Binary;
    assert!(
        undo(
            &[provider.clone(), entry_binary],
            false,
            false,
            &no_top,
            false
        )
        .is_empty()
    );

    // Rule 0: --emptytree skips the whole undo.
    assert!(
        undo(
            &[provider.clone(), entry.clone()],
            false,
            false,
            &no_top,
            true
        )
        .is_empty()
    );

    // Non-slot-operator entries are never touched.
    let mut entry_plain = entry.clone();
    entry_plain.outcome = PretendOutcome::Reinstall {
        version: "1.0".into(),
        changed_flags: vec!["soflag".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    assert!(
        undo(
            &[provider.clone(), entry_plain],
            false,
            false,
            &no_top,
            false
        )
        .is_empty()
    );

    // The latch: demoted cps are never re-added by the scan.
    let reach: HashSet<(String, String)> = HashSet::from([sounneed.clone()]);
    let latched = undo(
        &[provider.clone(), entry.clone()],
        false,
        false,
        &no_top,
        false,
    );
    assert_eq!(latched, replace);
    let (rescheduled, ..) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: &dir,
        repos: &[],
        entries: &[provider, entry],
        reachable: &reach,
        scope: None,
        already: &BTreeSet::new(),
        undone: &latched,
        with_bdeps: true,
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        excluded: &[],
        config: &test_config(),
    });
    assert!(
        rescheduled.is_empty(),
        "the undo latch suppresses the re-add"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn required_set_reachable_cps_follows_the_forward_installed_dep_graph() {
    // Throwaway vdb: tail -> mid -> target chain, an island, and a @system half.
    let dir = TempDir::new("portage-repo-reach").keep();
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    };
    mk("tail-1.0", "dev-libs/mid:0/1=");
    mk("mid-1.0", "dev-libs/target:0/1=");
    mk("target-1.0", "");
    mk("island-1.0", "");
    mk("sysmember-1.0", "dev-libs/sysdep");
    mk("sysdep-1.0", "");

    let cp = |p: &str| ("dev-libs".to_string(), p.to_string());

    // Only `tail` in @world: the chain is reachable, the rest is not.
    let r = required_set_reachable_cps(&dir, &["dev-libs/tail".to_string()], &[]);
    assert!(r.contains(&cp("tail")) && r.contains(&cp("mid")) && r.contains(&cp("target")));
    assert!(!r.contains(&cp("island")));
    assert!(!r.contains(&cp("sysmember")) && !r.contains(&cp("sysdep")));

    // `sysmember` in @system pulls `sysdep`.
    let r = required_set_reachable_cps(&dir, &[], &["dev-libs/sysmember".to_string()]);
    assert!(r.contains(&cp("sysmember")) && r.contains(&cp("sysdep")));
    assert!(!r.contains(&cp("mid")));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn installed_reachability_walks_match_independent_calls() {
    let dir = TempDir::new("portage-repo-reach-shared").keep();
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    };
    mk("tail-1.0", "dev-libs/mid");
    mk("mid-1.0", "dev-libs/target");
    mk("target-1.0", "");
    mk("sysmember-1.0", "dev-libs/sysdep");
    mk("sysdep-1.0", "");
    let tail = ["dev-libs/tail".to_string()];
    let sys = ["dev-libs/sysmember".to_string()];

    let graph = InstalledReachability::new(&dir);
    assert_eq!(
        graph.reachable(&tail, &[]),
        required_set_reachable_cps(&dir, &tail, &[])
    );
    assert_eq!(
        graph.reachable(&tail, &sys),
        required_set_reachable_cps(&dir, &tail, &sys)
    );
    // A second walk with other seeds on the same graph still agrees.
    assert_eq!(
        graph.reachable(&[], &sys),
        required_set_reachable_cps(&dir, &[], &sys)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reachable_without_does_not_follow_a_replaced_package() {
    // Portage: depgraph.py, _complete_graph -- only graph nodes pull deps in.
    let dir = TempDir::new("portage-repo-reach-without").keep();
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
    };
    mk("top-1.0", "dev-libs/mid");
    mk("mid-1.0", "dev-libs/leaf");
    mk("leaf-1.0", "");
    let seeds = ["dev-libs/top".to_string()];
    let graph = InstalledReachability::new(&dir);
    let cp = |p: &str| ("dev-libs".to_string(), p.to_string());

    let all = graph.reachable(&seeds, &[]);
    assert!(all.contains(&cp("leaf")));
    let skip: HashSet<_> = [cp("mid")].into_iter().collect();
    let live = graph.reachable_without(&seeds, &[], &skip);
    assert!(live.contains(&cp("mid")), "the replaced package is listed");
    assert!(!live.contains(&cp("leaf")), "but its deps are not followed");
    let _ = fs::remove_dir_all(&dir);
}
