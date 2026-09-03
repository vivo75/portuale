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
