use super::*;

#[test]
fn slot_operator_slot_change_probe_schedules_a_moved_installed_child() {
    // Bug 456208 shape: tree `RDEPEND=app-arch/libarchive:=` (unbuilt) against
    // installed `libarchive-3.1.1` (vdb `0`, tree `0/13`): Portage schedules it.
    let dir = slotundo_temp_dir("probe");
    let mk = |name: &str, slot: &str| {
        let d = dir.join("var/db/pkg/app-arch").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-arch\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
    };
    mk("libarchive-3.1.1", "0");
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let parent = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Reinstall {
            version: "4.10.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("kde-base", "ark", "4.10.0")
    };
    let child = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "3.1.1".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-arch", "libarchive", "3.1.1")
    };
    let child_cp = ("app-arch".to_string(), "libarchive".to_string());
    let empty = BTreeSet::new();

    let mut scheduled = BTreeSet::new();
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        entries: &[parent.clone(), child.clone()],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut masked,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([child_cp.clone()]),
        "the moved installed child joins the replace set (bug 456208)"
    );
    assert!(
        masked.is_empty(),
        "no binary entry is merged, so the #212 arm reports nothing"
    );

    // Resolved to a merge-bound entry (Portage's built dep): no probe.
    let mut merged_child = child.clone();
    merged_child.outcome = PretendOutcome::Reinstall {
        version: "3.1.1".into(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        entries: &[parent.clone(), merged_child],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(scheduled.is_empty());

    // The undo latch suppresses a re-add.
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        entries: &[parent, child],
        undone: &BTreeSet::from([child_cp]),
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(scheduled.is_empty());

    // A child whose tree ebuild did not move is not scheduled.
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souprov",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    let parent_souprov = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "souneedslot", "1.0")
    };
    let child_souprov = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("1".into()),
        ..graph_entry("dev-libs", "souprov", "1.0")
    };
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        entries: &[parent_souprov, child_souprov],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(scheduled.is_empty());

    let _ = fs::remove_dir_all(&dir);
}

/// The probe fires for an `Upgrade` parent too; a slot-only `:0` dep never
/// probes (not a slot-operator atom).
#[test]
fn slot_operator_slot_change_probe_fires_for_upgrade_and_ignores_plain_slot_deps() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = slotundo_temp_dir("161-probe");
    let repo = base.join("repo");
    let write_pkg = |cp: &str, pv: &str, slot: &str, rdepend: &str| {
        let (cat, pkg) = cp.split_once('/').expect("category/package");
        let dir = repo.join(cat).join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            format!("EAPI=8\nDESCRIPTION=\"161 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !rdepend.is_empty() {
            writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
        }
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=161 probe\nEAPI=8\n".to_string();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache").join(cat);
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("dev-libs/probeparentA", "1.0", "0", "dev-libs/probechild:=");
    write_pkg("dev-libs/probeparentB", "1.0", "0", "dev-libs/probechild:0");
    write_pkg("dev-libs/probechild", "2.0", "0/1", "");
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
    // Installed child moved: vdb 0/9 vs tree 0/1.
    let vd = base.join("var/db/pkg/dev-libs/probechild-2.0");
    std::fs::create_dir_all(&vd).unwrap();
    std::fs::write(vd.join("CATEGORY"), "dev-libs\n").unwrap();
    std::fs::write(vd.join("SLOT"), "0/9\n").unwrap();
    std::fs::write(vd.join("repository"), "testrepo\n").unwrap();
    let child = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "probechild", "2.0")
    };
    let child_cp = ("dev-libs".to_string(), "probechild".to_string());
    let empty = BTreeSet::new();

    // An `Upgrade` parent probes like a `Reinstall` one.
    let upgrade_a = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Upgrade {
            from: "0.9".into(),
            to: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "probeparentA", "1.0")
    };
    let mut scheduled = BTreeSet::new();
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &[upgrade_a, child.clone()],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut masked,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([child_cp.clone()]),
        "an Upgrade parent probes the moved child"
    );

    // A plain `:0` dep is not a slot-operator atom: no probe.
    let new_b = graph_entry("dev-libs", "probeparentB", "1.0");
    let mut scheduled_b = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &[new_b, child],
        undone: &empty,
        scheduled: &mut scheduled_b,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(
        scheduled_b.is_empty(),
        "a slot-only dep without `=` never probes"
    );
    let _ = fs::remove_dir_all(&base);
}

/// The binary arm masks only when the same-version ebuild is visible (Portage's
/// `_slot_change_probe`).
#[test]
fn slot_operator_slot_change_probe_keeps_a_binary_whose_ebuild_is_invisible() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = slotundo_temp_dir("243-probe");
    let repo = base.join("repo");
    let write_pkg = |cp: &str, slot: &str, keywords: &str, rdepend: &str| {
        let (cat, pkg) = cp.split_once('/').expect("category/package");
        let dir = repo.join(cat).join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body = format!(
            "EAPI=8\nDESCRIPTION=\"243 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"{keywords}\"\n"
        );
        if !rdepend.is_empty() {
            writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
        }
        std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=243 probe\nEAPI=8\n".to_string();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "KEYWORDS={keywords}\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache").join(cat);
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-1.0")), entry).unwrap();
    };
    write_pkg(
        "dev-libs/visparent",
        "0",
        "amd64",
        "dev-libs/stalechild:= dev-libs/hiddenchild:=",
    );
    // Both trees moved 0/1 -> 0/2; only `hiddenchild`'s ebuild is masked.
    write_pkg("dev-libs/stalechild", "0/2", "amd64", "");
    write_pkg("dev-libs/hiddenchild", "0/2", "~amd64", "");
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
    let parent = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", "visparent", "1.0")
    };
    let binary_child = |package: &str| GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("1".into()),
        source: CandidateSource::Binary,
        ..graph_entry("dev-libs", package, "1.0")
    };
    let entries = vec![
        parent,
        binary_child("stalechild"),
        binary_child("hiddenchild"),
    ];
    let empty = BTreeSet::new();
    let mut scheduled = BTreeSet::new();
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &entries,
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut masked,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        masked,
        BTreeSet::from([(
            "dev-libs".to_string(),
            "stalechild".to_string(),
            "1.0".to_string()
        )]),
        "the visible stale binary is masked, the one behind a hidden ebuild is kept"
    );
}

/// The backtrack arm masks a merge-bound stale binary so the restart resolves the
/// dep to the tree ebuild instead.
#[test]
fn slot_operator_slot_change_probe_masks_a_stale_binary() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = slotundo_temp_dir("212-probe");
    let repo = base.join("repo");
    let write_pkg = |cp: &str, pv: &str, slot: &str, rdepend: &str| {
        let (cat, pkg) = cp.split_once('/').expect("category/package");
        let dir = repo.join(cat).join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            format!("EAPI=8\nDESCRIPTION=\"212 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !rdepend.is_empty() {
            writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
        }
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=212 probe\nEAPI=8\n".to_string();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache").join(cat);
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg(
        "dev-libs/maskparent",
        "1.0",
        "0",
        "dev-libs/maskchild:= dev-libs/samechild:= dev-libs/notreechild:=",
    );
    // Stale: tree moved 0/1 -> 0/2, the binary below was built at 0/1.
    write_pkg("dev-libs/maskchild", "1.0", "0/2", "");
    // Unmoved: tree and binary agree on 0/1.
    write_pkg("dev-libs/samechild", "1.0", "0/1", "");
    // `notreechild` has no tree ebuild at all (no write_pkg call).
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
    let parent = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", "maskparent", "1.0")
    };
    let binary_child = |package: &str, slot: &str, sub: &str| GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        slot: Some(slot.into()),
        sub_slot: Some(sub.into()),
        source: CandidateSource::Binary,
        ..graph_entry("dev-libs", package, "1.0")
    };
    let entries = vec![
        parent.clone(),
        binary_child("maskchild", "0", "1"),
        binary_child("samechild", "0", "1"),
        binary_child("notreechild", "0", "1"),
    ];
    let empty = BTreeSet::new();

    // Only the slot-moved binary reports; nothing schedules here.
    let mut scheduled = BTreeSet::new();
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &entries,
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut masked,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(scheduled.is_empty());
    assert_eq!(
        masked,
        BTreeSet::from([(
            "dev-libs".to_string(),
            "maskchild".to_string(),
            "1.0".to_string()
        )]),
        "the stale binary is masked; the unmoved one, the ebuild-less \
         one and the scheduled set are untouched"
    );

    // `--exclude` on the tree ebuild vetoes the mask (Portage).
    let mut masked_excl: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut scheduled_excl = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &entries,
        undone: &empty,
        scheduled: &mut scheduled_excl,
        excluded: &["dev-libs/maskchild".to_string()],
        masked: &mut masked_excl,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(
        masked_excl.is_empty(),
        "an excluded tree ebuild is no replacement"
    );

    // A binary parent never probes (Portage's `not dep.parent.built` gate).
    let binary_parent = GraphEntry {
        source: CandidateSource::Binary,
        ..parent.clone()
    };
    let mut masked_bpar: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut scheduled_bpar = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &base,
        entries: &[binary_parent, binary_child("maskchild", "0", "1")],
        undone: &empty,
        scheduled: &mut scheduled_bpar,
        excluded: &[],
        masked: &mut masked_bpar,
        config: &test_config(),
        gate: crate::test_gate(&repos),
    });
    assert!(masked_bpar.is_empty(), "a binary parent never probes");
    let _ = fs::remove_dir_all(&base);
}

/// The update force scan forces the installed child's update when the tree has a
/// newer slot the replacement's `:=` accepts; a second parent's pin or a missing
/// `--update` refuses it.
#[test]
fn slot_operator_update_force_scan_forces_the_installed_childs_update() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-236-force").keep();
    for (name, slot, rdepend) in [
        ("forceprov-1.0", "0/1", ""),
        ("forcecons-1.0", "0", ">=dev-libs/forceprov-1.0:0/1="),
        ("forcecons2-1.0", "0", ">=dev-libs/forceprov-1.0:0/1="),
        ("forceveto-1.0", "0", "<dev-libs/forceprov-2.0:0/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("EAPI"), "8\n").unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str, rdepend: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "EAPI=8\nDESCRIPTION=\"236 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"{rdepend}\"\n"
        );
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=236 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64").unwrap();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "SLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("forceprov", "1.0", "0/1", "");
    write_pkg("forceprov", "2.0", "0/2", "");
    write_pkg("forcecons", "1.0", "0", "dev-libs/forceprov:=");
    write_pkg("forcecons2", "1.0", "0", "dev-libs/forceprov:=");
    write_pkg("forceveto", "1.0", "0", "<dev-libs/forceprov-2.0:=");
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
    let installed = |pkg: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", pkg, "1.0")
    };
    let entries = vec![
        installed("forcecons"),
        installed("forcecons2"),
        installed("forceprov"),
    ];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let cons = ("dev-libs".to_string(), "forcecons".to_string());
    let veto = ("dev-libs".to_string(), "forceveto".to_string());
    let config = test_config();
    let scan = |entries: &[GraphEntry], reachable: &HashSet<(String, String)>, update: bool| {
        slot_operator_update_force_scan(SlotOperatorUpdateForceScanArgs {
            root: &base,
            config: &config,
            entries,
            reachable,
            already: &empty,
            undone: &empty,
            with_bdeps: false,
            update,
            top_level_cps: &HashSet::new(),
            excluded: &[],
            gate: crate::test_gate(&repos),
        })
    };
    let (consumers, forced) = scan(&entries, &HashSet::new(), true);
    // The forced child joins its consumers in the replace set; a second
    // consumer of the same child schedules too (Portage probes every parent).
    assert_eq!(
        consumers,
        BTreeSet::from([
            cons.clone(),
            ("dev-libs".to_string(), "forcecons2".to_string()),
            ("dev-libs".to_string(), "forceprov".to_string())
        ])
    );
    assert_eq!(
        forced,
        BTreeSet::from([(
            "dev-libs".to_string(),
            "forceprov".to_string(),
            "2.0".to_string()
        )])
    );
    // Without `--update` the probe does not run.
    assert_eq!(
        scan(&entries, &HashSet::new(), false),
        (empty.clone(), BTreeSet::new())
    );
    // A reachable installed parent pinning `<2.0` refuses the update.
    let mut with_veto = entries.clone();
    with_veto.push(installed("forceveto"));
    assert_eq!(
        scan(&with_veto, &HashSet::from([veto.clone()]), true),
        (empty.clone(), BTreeSet::new())
    );
    // Already-upgraded child: the walk's own selection owns that case.
    let upgraded = vec![
        installed("forcecons"),
        installed("forcecons2"),
        GraphEntry {
            outcome: PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "2.0".into(),
            },
            ..graph_entry("dev-libs", "forceprov", "2.0")
        },
    ];
    assert_eq!(
        scan(&upgraded, &HashSet::new(), true),
        (empty, BTreeSet::new())
    );
    let _ = fs::remove_dir_all(&base);
}

/// The consumer's replacement ebuild refuses a candidate its own atoms exclude
/// (weston's `<libdisplay-info-0.4.0:=`), so Portage's update probe finds no
/// new child for it; a candidate inside the range is accepted.
#[test]
fn replacement_accepts_follows_the_consumers_tree_atoms() {
    use md5::Digest as _;
    let base = TempDir::new("portage-repo-482-accepts").keep();
    let d = base.join("var/db/pkg/dev-libs/accons-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("EAPI"), "8\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    fs::write(d.join("RDEPEND"), "<dev-libs/accprov-2.0:0/1=\n").unwrap();
    let repo = base.join("repo");
    let dir = repo.join("dev-libs/accons");
    fs::create_dir_all(&dir).unwrap();
    let rdepend = "<dev-libs/accprov-2.0:=";
    let body = format!(
        "EAPI=8\nDESCRIPTION=\"482 accepts\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"{rdepend}\"\n"
    );
    fs::write(dir.join("accons-1.0.ebuild"), &body).unwrap();
    let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
    let cachedir = repo.join("metadata/md5-cache/dev-libs");
    fs::create_dir_all(&cachedir).unwrap();
    fs::write(
        cachedir.join("accons-1.0"),
        format!(
            "DEFINED_PHASES=-\nDESCRIPTION=482 accepts\nEAPI=8\nKEYWORDS=amd64\nRDEPEND={rdepend}\nSLOT=0\n_md5_={md5}\n"
        ),
    )
    .unwrap();
    let repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: repo,
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
    let pkg = all_installed_packages(&base)
        .iter()
        .find(|p| p.package == "accons")
        .cloned()
        .expect("accons is installed");
    let config = test_config();
    let provider = ("dev-libs".to_string(), "accprov".to_string());
    let accepts = |cand: &str| {
        replacement_accepts(ReplacementAcceptsArgs {
            root: &base,
            config: &config,
            pkg: &pkg,
            dep_keys: &["RDEPEND"],
            provider_cp: &provider,
            cand,
            gate: crate::test_gate(&repos),
        })
    };
    assert!(!accepts("dev-libs/accprov-2.0:0/2"));
    assert!(accepts("dev-libs/accprov-1.5:0/2"));
    let _ = fs::remove_dir_all(&base);
}

/// An `|| ( ... )` alternative never vetoes a replacement candidate (Portage
/// `_slot_operator_update_probe` only reads the parent's mandatory atoms).
#[test]
fn replacement_accepts_ignores_or_groups() {
    assert_eq!(
        without_or_groups(">=a/b-1:= || ( c/d:= <a/b-2 ) e/f || ( ( g/h ) i/j ) k/l"),
        ">=a/b-1:= e/f k/l"
    );
    assert_eq!(without_or_groups("a/b"), "a/b");
}
