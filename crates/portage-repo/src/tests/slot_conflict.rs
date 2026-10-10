use super::*;

#[test]
fn record_slot_conflict_merges_same_triple_and_appends_new() {
    // One collision handler per conflicting slot: a second atom on the
    // same triple re-renders the record instead of appending a duplicate.
    let mk = |atom: &str, current: &str| SlotConflict {
        category: "dev-libs".to_string(),
        package: "t".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: atom.to_string(),
        instances: vec![
            SlotConflictInstance {
                version: "2.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "r".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
            SlotConflictInstance {
                version: current.to_string(),
                sub_slot: "0".to_string(),
                repo_name: "r".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
        ],
    };
    let mut v = vec![mk("atom-1", "1.0")];
    record_slot_conflict(&mut v, mk("atom-2", "1.0"));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].conflicting_atom, "atom-2");
    record_slot_conflict(&mut v, mk("atom-3", "1.5"));
    assert_eq!(v.len(), 2);
}

/// The record forms with the merge version resolved, the installed
/// version preserved, the consumer at its recorded slot and repo.
#[test]
fn build_residual_forms_a_record_for_a_broken_pin() {
    let dir = slotundo_temp_dir("161-resid");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin()],
    });
    assert_eq!(out.len(), 1);
    let r = &out[0];
    assert_eq!(r.resolved_version, "2.0");
    assert_eq!(r.conflicting_atom, "=dev-libs/souprov-1.0:0/1");
    assert_eq!(r.instances.len(), 2);
    assert!(!r.instances[0].installed);
    assert_eq!(r.instances[0].version, "2.0");
    assert!(r.instances[1].installed);
    assert_eq!(r.instances[1].version, "1.0");
    let parents: Vec<&str> = r.instances[1]
        .parents
        .iter()
        .map(|p| p.parent_cpv.as_str())
        .collect();
    assert_eq!(parents, vec!["dev-libs/conpkg-1.0:3/0::testrepo"]);
    let _ = fs::remove_dir_all(&dir);
}

/// A `New` entry feeds the merge version like an `Upgrade` one.
#[test]
fn build_residual_reads_the_merge_version_from_a_new_entry() {
    let dir = slotundo_temp_dir("161-resid-new");
    let (repos, config, _) = residual_161_setup(&dir);
    let new_entry = graph_entry("dev-libs", "souprov", "2.0");
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[new_entry],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin()],
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].resolved_version, "2.0");
    let _ = fs::remove_dir_all(&dir);
}

/// A dropped pin whose consumer is itself merge-bound is replaced,
/// not reported.
#[test]
fn build_residual_skips_a_consumer_being_replaced() {
    let dir = slotundo_temp_dir("161-resid-repl");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let consumer_merge = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "conpkg".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        slot: Some("3".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "conpkg", "1.0")
    };
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade, consumer_merge],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin()],
    });
    assert!(out.is_empty(), "a replaced consumer is not reported");
    let _ = fs::remove_dir_all(&dir);
}

/// A pin the merged version satisfies after all reconciles silently.
#[test]
fn build_residual_reconciles_a_pin_the_merge_satisfies() {
    let dir = slotundo_temp_dir("161-resid-rec");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let mut pin = residual_161_pin();
    pin.atom = "=dev-libs/souprov-2.0:0/2".to_string();
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &HashMap::new(),
        dropped: &[pin],
    });
    assert!(out.is_empty(), "a satisfied pin reconciles silently");
    let _ = fs::remove_dir_all(&dir);
}

/// The merge version comes from the target cp's own entry, never a
/// neighbour's.
#[test]
fn build_residual_reads_the_merge_version_from_the_target_entry() {
    let dir = slotundo_temp_dir("161-resid-cp");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let neighbour = graph_entry("dev-libs", "otherpkg", "9.9");
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade, neighbour],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin()],
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].resolved_version, "2.0");
    let _ = fs::remove_dir_all(&dir);
}

/// Of the installed versions the pin matches, the greatest wins; a
/// non-matching one never displaces.
#[test]
fn build_residual_keeps_the_highest_matching_installed() {
    let dir = slotundo_temp_dir("161-resid-inst");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    // Installed 2.0 at the same sub-slot does not match
    // `=...-1.0:0/1` and must not displace installed 1.0.
    let d2 = dir.join("var/db/pkg/dev-libs/souprov-2.0");
    fs::create_dir_all(&d2).unwrap();
    fs::write(d2.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d2.join("SLOT"), "0/1\n").unwrap();
    fs::write(d2.join("repository"), "testrepo\n").unwrap();
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin()],
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].instances[1].version, "1.0");
    let _ = fs::remove_dir_all(&dir);
}

/// Between two matching installed versions the greater wins (Portage
/// `matches[-1]`).
#[test]
fn build_residual_prefers_the_greatest_matching_installed() {
    let dir = slotundo_temp_dir("161-resid-max");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let dr = dir.join("var/db/pkg/dev-libs/souprov-1.0-r1");
    fs::create_dir_all(&dr).unwrap();
    fs::write(dr.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(dr.join("SLOT"), "0/1\n").unwrap();
    fs::write(dr.join("repository"), "testrepo\n").unwrap();
    let mut pin = residual_161_pin();
    pin.atom = ">=dev-libs/souprov-1.0:0/1".to_string();
    pin.raw_atom = pin.atom.clone();
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &HashMap::new(),
        dropped: &[pin],
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].instances[1].version, "1.0-r1");
    let _ = fs::remove_dir_all(&dir);
}

/// Puller atoms file by version match and USE satisfaction,
/// deduplicated per side.
#[test]
fn build_residual_files_puller_parents_by_version_and_use() {
    let dir = slotundo_temp_dir("161-resid-pull");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let row = |atom: &str| {
        (
            "dev-libs".to_string(),
            "sounneed".to_string(),
            "1.0".to_string(),
            atom.to_string(),
        )
    };
    let pullers: SlotPullers = HashMap::from([(
        ("dev-libs".to_string(), "souprov".to_string()),
        vec![
            row("=dev-libs/souprov-2.0"),
            row("=dev-libs/souprov-2.0"),
            row("=dev-libs/souprov-1.0"),
            row("=dev-libs/souprov-1.0"),
            row("=dev-libs/souprov-2.0[no_such_flag_xyz]"),
            row("=dev-libs/souprov-1.0[no_such_flag_xyz]"),
            row("<dev-libs/souprov-0.1"),
        ],
    )]);
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &pullers,
        dropped: &[residual_161_pin()],
    });
    assert_eq!(out.len(), 1);
    let merge_atoms: Vec<&str> = out[0].instances[0]
        .parents
        .iter()
        .map(|p| p.atom.as_str())
        .collect();
    let inst_atoms: Vec<&str> = out[0].instances[1]
        .parents
        .iter()
        .map(|p| p.atom.as_str())
        .collect();
    assert_eq!(merge_atoms, vec!["=dev-libs/souprov-2.0"]);
    // The puller plus the dropped pin itself, folded in after the
    // pullers.
    assert_eq!(
        inst_atoms,
        vec!["=dev-libs/souprov-1.0", "=dev-libs/souprov-1.0:0/1"]
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Each pin folds into its own record only; a stray pin folds nowhere.
#[test]
fn build_residual_folds_each_pin_into_its_own_record() {
    let dir = slotundo_temp_dir("161-resid-two");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let d = dir.join("var/db/pkg/dev-libs/sounneed-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/1\n").unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    let sounneed_merge = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "sounneed".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "sounneed", "1.0")
    };
    let pin_b = RevDepPin {
        cp: ("dev-libs".to_string(), "sounneed".to_string()),
        atom: "=dev-libs/sounneed-1.0:0/1".to_string(),
        raw_atom: "=dev-libs/sounneed-1.0:0/1".to_string(),
        consumer: (
            "dev-libs".to_string(),
            "conpkg".to_string(),
            "1.0".to_string(),
        ),
    };
    let pin_stray = RevDepPin {
        cp: ("dev-libs".to_string(), "zzz".to_string()),
        atom: "=dev-libs/souprov-1.0:0/1".to_string(),
        raw_atom: "=dev-libs/souprov-1.0:0/1".to_string(),
        consumer: ("dev-libs".to_string(), "zzz".to_string(), "9.9".to_string()),
    };
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade, sounneed_merge],
        slot_pullers: &HashMap::new(),
        dropped: &[residual_161_pin(), pin_b, pin_stray],
    });
    assert_eq!(out.len(), 2);
    for r in &out {
        let atoms: Vec<&str> = r.instances[1]
            .parents
            .iter()
            .map(|p| p.atom.as_str())
            .collect();
        assert_eq!(atoms.len(), 1, "exactly its own pin: {atoms:?}");
        assert!(
            !r.instances[1]
                .parents
                .iter()
                .any(|p| p.parent_cpv.contains("zzz")),
            "the stray pin folds nowhere"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Two identical pins fold one parent line, not two.
#[test]
fn build_residual_dedups_identical_pins() {
    let dir = slotundo_temp_dir("161-resid-dup");
    let (repos, config, upgrade) = residual_161_setup(&dir);
    let pin = residual_161_pin();
    let out = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &repos,
        config: &config,
        root: &dir,
        entries: &[upgrade],
        slot_pullers: &HashMap::new(),
        dropped: &[pin.clone(), pin],
    });
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].instances[1].parents.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// A built `:S/SS=` puller re-files against the installed vdb
/// sub-slot, not the tree's metadata for that version.
#[test]
fn merge_same_slot_conflicts_refiles_a_built_puller_under_the_installed_sub_slot() {
    let base = slotundo_temp_dir("291-refile");
    let repo = base.join("repo");
    let dir = repo.join("dev-libs/foo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("foo-1.0.ebuild"),
        "EAPI=8\nDESCRIPTION=\"291\"\nSLOT=\"0/1\"\nKEYWORDS=\"amd64\"\n",
    )
    .unwrap();
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
    let inst = |version: &str, sub: &str, installed: bool| SlotConflictInstance {
        version: version.to_string(),
        sub_slot: sub.to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed,
    };
    // The installed foo-1.0 was built at 0/9; the tree's 1.0 says 0/1.
    let conflict = SlotConflict {
        category: "dev-libs".to_string(),
        package: "foo".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: "dev-libs/foo:0/9=".to_string(),
        instances: vec![inst("1.0", "9", true), inst("2.0", "2", false)],
    };
    let mut pullers: SlotPullers = HashMap::new();
    pullers.insert(
        ("dev-libs".to_string(), "foo".to_string()),
        vec![(
            "dev-libs".to_string(),
            "parent".to_string(),
            "1.0".to_string(),
            "dev-libs/foo:0/9=".to_string(),
        )],
    );
    let out = merge_same_slot_conflicts(MergeSameSlotConflictsArgs {
        conflicts: &[conflict],
        pullers: &pullers,
        repos: &repos,
        config: &test_config(),
        root: &base,
        refile_singletons: true,
    });
    let installed = out[0].instances.iter().find(|i| i.installed).unwrap();
    assert_eq!(
        installed.parents.len(),
        1,
        "the `:0/9=` puller files under the installed 0/9 instance"
    );
}
