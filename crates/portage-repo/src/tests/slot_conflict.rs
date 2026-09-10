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
