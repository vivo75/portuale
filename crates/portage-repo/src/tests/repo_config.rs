use super::*;

#[test]
fn find_repos_defaults_masters_to_the_main_repo_alone_when_unset() {
    // Portage: config.py RepoConfigLoader (no "masters" key: every
    // non-main repo masters the main repo alone).
    let root = masters_test_root("default");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n\n[overlay]\nlocation = overlay\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main", "overlay"]);

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let main = repos.iter().find(|r| r.name == "main").unwrap();
    let overlay = repos.iter().find(|r| r.name == "overlay").unwrap();
    assert_eq!(main.masters, Vec::<PathBuf>::new());
    assert_eq!(overlay.masters, vec![main.location.clone()]);
}

#[test]
fn find_repos_reads_sync_fields_volatile_and_git_module_options() {
    let root = masters_test_root("sync-fields");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\nsync-type = git\n\
         sync-uri = https://example.invalid/main.git\n\
         sync-git-verify-commit-signature = true\n\
         sync-rsync-verify-jobs = 9\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main"]);

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let main = repos.iter().find(|r| r.name == "main").unwrap();
    assert_eq!(main.sync_type.as_deref(), Some("git"));
    assert_eq!(
        main.sync_uri.as_deref(),
        Some("https://example.invalid/main.git")
    );
    // Not under /var/db/repos -> volatile heuristic yields true.
    assert!(main.volatile);
    // Only git-module keys apply for sync-type = git.
    assert_eq!(
        main.module_specific_options,
        vec![(
            "sync-git-verify-commit-signature".to_string(),
            "true".to_string()
        )]
    );
}

#[test]
fn find_repos_explicit_volatile_key_wins_over_the_heuristic() {
    let root = masters_test_root("volatile-explicit");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\nvolatile = no\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main"]);
    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    assert!(!repos.iter().find(|r| r.name == "main").unwrap().volatile);
}

#[test]
fn find_repos_resolves_an_explicit_masters_key_to_the_named_repos_own_locations() {
    // Explicit "masters" overrides the default outright, in declared
    // order; unknown names drop (Portage config.py only warns).
    let root = masters_test_root("explicit");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [overlay]\nlocation = overlay\n\n\
         [downstream]\nlocation = downstream\nmasters = overlay unknownrepo\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main", "overlay", "downstream"]);

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let overlay = repos.iter().find(|r| r.name == "overlay").unwrap();
    let downstream = repos.iter().find(|r| r.name == "downstream").unwrap();
    // An explicit key fully replaces the default (no implicit main).
    assert_eq!(downstream.masters, vec![overlay.location.clone()]);
}

#[test]
fn find_repos_treats_an_explicit_empty_masters_key_as_no_masters_at_all() {
    // Explicit-but-empty "masters =" means no masters (Portage).
    let root = masters_test_root("empty");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [overlay]\nlocation = overlay\nmasters = \n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main", "overlay"]);

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let overlay = repos.iter().find(|r| r.name == "overlay").unwrap();
    assert_eq!(overlay.masters, Vec::<PathBuf>::new());
}

#[test]
fn find_repos_reads_layout_conf_masters_repo_name_and_profile_formats() {
    // Portage three-tier masters: layout.conf `masters =` beats the
    // implicit default; `repo-name` overrides the section name.
    let root = masters_test_root("layout-conf");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [overlay]\nlocation = overlay\n\n\
         [downstream]\nlocation = downstream\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("main/metadata")).unwrap();
    std::fs::create_dir_all(root.join("overlay/metadata")).unwrap();
    std::fs::create_dir_all(root.join("downstream/metadata")).unwrap();
    // layout.conf `masters = overlay` wins over the implicit main.
    std::fs::write(
        root.join("downstream/metadata/layout.conf"),
        "masters = overlay\nrepo-name = renamed-downstream\n\
         aliases = downstream\nprofile-formats = portage-2\n",
    )
    .unwrap();

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let overlay = repos.iter().find(|r| r.name == "overlay").unwrap();
    // The section was `[downstream]`; the layout.conf `repo-name`
    // renamed it.
    assert!(repos.iter().all(|r| r.name != "downstream"));
    let renamed = repos
        .iter()
        .find(|r| r.name == "renamed-downstream")
        .expect("layout.conf repo-name override applied");
    assert_eq!(renamed.masters, vec![overlay.location.clone()]);
    assert_eq!(renamed.profile_formats, vec!["portage-2".to_string()]);
    // A repos.conf `masters` key still wins over layout.conf.
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [overlay]\nlocation = overlay\n\n\
         [downstream]\nlocation = downstream\nmasters = \n",
    )
    .unwrap();
    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    let renamed = repos
        .iter()
        .find(|r| r.name == "renamed-downstream")
        .unwrap();
    assert_eq!(
        renamed.masters,
        Vec::<PathBuf>::new(),
        "an explicit (even empty) repos.conf masters wins over layout.conf's"
    );
}

#[test]
fn find_repos_reads_profiles_repo_name_and_drops_a_mismatched_section() {
    // `profiles/repo_name` is canonical; a mismatched section without
    // an alias is dropped (Portage `_read_repo_name`, config.py).
    let root = masters_test_root("repo-name-file");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [aliased-section]\nlocation = aliased\n\n\
         [dropped-section]\nlocation = dropped\n",
    )
    .unwrap();
    for r in ["main", "aliased", "dropped"] {
        std::fs::create_dir_all(root.join(r).join("profiles")).unwrap();
        std::fs::create_dir_all(root.join(r).join("metadata")).unwrap();
    }
    std::fs::write(root.join("main/profiles/repo_name"), "main\n").unwrap();
    // `aliased` mismatches but is listed as an alias: kept.
    std::fs::write(root.join("aliased/profiles/repo_name"), "aliased-repo\n").unwrap();
    std::fs::write(
        root.join("aliased/metadata/layout.conf"),
        "aliases = aliased-section\n",
    )
    .unwrap();
    // `dropped`: repo_name file mismatches the section, no alias.
    std::fs::write(root.join("dropped/profiles/repo_name"), "dropped-repo\n").unwrap();

    let repos = hermetic_find_repos(&root).expect("repos.conf resolves");
    assert!(repos.iter().any(|r| r.name == "aliased-repo"));
    assert!(
        !repos.iter().any(|r| r.name == "dropped-repo"),
        "a section whose name mismatches the repo_name file with no alias is dropped"
    );
    assert!(repos.iter().all(|r| r.name != "dropped-section"));
}

#[test]
fn reverse_dependency_constraints_enforces_a_runtime_pin_but_ignores_a_build_time_pin() {
    // The runtime-key pin enforces as a slot constraint; the
    // build-time-key pin is ignored entirely.
    let root = fixtures_root();
    let repos = hermetic_find_repos(&root).expect("fixture repos.conf must resolve");
    let upgrade = |pkg: &str| GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: pkg.into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        ..graph_entry("dev-libs", pkg, "2.0")
    };
    let entries = [upgrade("whblocker"), upgrade("rdcblocker")];
    let hard_want: HashMap<(String, String), Vec<String>> = HashMap::from([
        (
            ("dev-libs".to_string(), "whblocker".to_string()),
            vec![">=dev-libs/whblocker-1".to_string()],
        ),
        (
            ("dev-libs".to_string(), "rdcblocker".to_string()),
            vec![">=dev-libs/rdcblocker-1".to_string()],
        ),
    ]);
    let reachable: HashSet<(String, String)> = HashSet::from([
        ("dev-libs".to_string(), "whtarget".to_string()),
        ("dev-libs".to_string(), "rdctarget".to_string()),
    ]);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        root: &root,
        entries: &entries,
        with_bdeps: true,
        excluded: &[],
        hard_want: &hard_want,
        reachable: &reachable,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        top_level_cps: &HashSet::new(),
        world_reachable: &HashSet::new(),
        config: &portage_profile::Config::default(),
        complete: false,
        portage_complete: false,
        installed_walked: &HashMap::new(),
        update: false,
        selective: true,
        backtrack_allowed: true,
        arg_atoms: &[],
        root_deps_running_root: None,
        gate: crate::test_gate(&repos),
    });
    assert_eq!(
        enforced.len(),
        1,
        "the reachable runtime-key pin must be enforced, got {enforced:?}"
    );
    assert_eq!(enforced[0].consumer.1, "whtarget");
    assert!(
        enforced.iter().all(|p| p.consumer.1 != "rdctarget")
            && dropped.iter().all(|p| p.consumer.1 != "rdctarget"),
        "the build-time-key pin must be ignored entirely, got enforced={enforced:?} dropped={dropped:?}"
    );
}

#[test]
fn repo_usepkg_atoms_append_the_repo_name_and_drop_bad_and_conflicting_atoms() {
    // Portage: repository/config.py:253-282 (validation, conflict removal),
    // depgraph.py:384-392 (`atom::reponame`); Portage 3.0.82.2 evidence in
    // LLM/evidence/2026-10-18-s8/out373/.
    let root = masters_test_root("usepkg-lists");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n\
         usepkg-exclude = dev-libs/a dev-libs/b:2 >=dev-libs/bad-1 dev-libs/both\n\
         usepkg-include = dev-libs/both dev-libs/c\n",
    )
    .unwrap();
    let (exclude, include) = repo_usepkg_atoms_impl(&root, &root.join("no-such-global.conf"), None);
    assert_eq!(exclude, vec!["dev-libs/a::main", "dev-libs/b:2::main"]);
    assert_eq!(include, vec!["dev-libs/c::main"]);
}

#[test]
fn global_and_user_repos_conf_merge_with_user_keys_winning() {
    // Portage: `load_repository_config` reads the global file before the
    // user's, so a user `[section]` key overrides the global one and
    // unset keys fall through (`repository/config.py:1490-1505`).
    let root = masters_test_root("global-merge");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    let global = root.join("global.conf");
    std::fs::write(
        &global,
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\nsync-uri = https://example.invalid/global\npriority = 7\n",
    )
    .unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\nsync-uri = https://example.invalid/user\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main"]);
    let repos = find_repos_impl_with(&root, true, &global, None).expect("merge resolves");
    let main = repos.iter().find(|r| r.name == "main").unwrap();
    assert_eq!(
        main.sync_uri.as_deref(),
        Some("https://example.invalid/user")
    );
    assert_eq!(main.priority, 7);
}

#[test]
fn absent_global_file_contributes_no_sections() {
    // Portage's installed path: an absent global file is simply skipped;
    // the `cnf/repos.conf` fallback is only for a non-installed Portage.
    let root = masters_test_root("global-absent");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main"]);
    let repos = find_repos_impl_with(&root, true, &root.join("no-such-global.conf"), None)
        .expect("user file alone resolves");
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "main");
}

#[test]
fn portage_repositories_replaces_both_files() {
    // Portage: when `PORTAGE_REPOSITORIES` is set, exactly that text is
    // parsed and neither file is read (`repository/config.py:1490-1505`).
    let root = masters_test_root("repositories-var");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    let global = root.join("global.conf");
    std::fs::write(&global, "[DEFAULT]\nmain-repo = global-only\n").unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = user-only\n\n[user-only]\nlocation = user-only\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["var-only"]);
    let var_text =
        "[DEFAULT]\nmain-repo = var-only\n\n[var-only]\nlocation = var-only\n".to_string();
    let repos =
        find_repos_impl_with(&root, true, &global, Some(var_text)).expect("variable text resolves");
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "var-only");
}

#[test]
fn missing_repo_directory_drops_the_repo_but_keeps_gentoo() {
    // Portage: a `location` that is not a directory drops the repo with
    // a `!!!` line, except `gentoo`, which is kept so a first sync is
    // possible (`repository/config.py:1088-1106`). A section without a
    // `location` attribute is dropped too.
    let root = masters_test_root("missing-dir");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n\
         [main]\nlocation = main\n\n\
         [gone]\nlocation = gone\n\n\
         [gentoo]\nlocation = vanished\n\n\
         [locless]\npriority = 5\n",
    )
    .unwrap();
    make_repo_dirs(&root, &["main"]);
    let repos = hermetic_find_repos(&root).expect("resolves with drops");
    assert!(repos.iter().any(|r| r.name == "main"));
    assert!(repos.iter().all(|r| r.name != "gone"));
    assert!(repos.iter().all(|r| r.name != "locless"));
    let gentoo = repos
        .iter()
        .find(|r| r.name == "gentoo")
        .expect("gentoo is kept despite its missing directory");
    assert!(gentoo.location.ends_with("vanished"));
}

#[test]
fn bad_sections_warn_once_in_file_order() {
    // Portage `RepoConfigLoader.__init__`
    // (`lib/portage/repository/config.py:1072-1106`): one pass over the
    // sections in file order, warning as it goes; it runs once per
    // process, so one `emerge` prints each line once. `zeta-noloc` sorts
    // after `alpha-badloc`: file order -- not sorted or `HashMap` order --
    // puts its line first. `eprintln!` goes through libtest's capture
    // hook, so an fd redirect cannot observe it; the parent below re-runs
    // this test in a subprocess and asserts on the real stderr bytes.
    if let Ok(root) = std::env::var("PORTUALE_REPOS_WARN_PROBE") {
        let root = std::path::PathBuf::from(root);
        let global = root.join("no-such-global.conf");
        find_repos_impl_with(&root, true, &global, None).expect("resolves with drops");
        find_repos_impl_with(&root, true, &global, None).expect("resolves with drops");
        std::process::exit(0);
    }
    let tmp = TempDir::new("repos-conf-warnings");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    let bad_dir = root.join("does-not-exist");
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n\
             [zeta-noloc]\npriority = 5\n\n\
             [alpha-badloc]\nlocation = {}\n\n\
             [main]\nlocation = main\n",
            bad_dir.display()
        ),
    )
    .unwrap();
    make_repo_dirs(root, &["main"]);

    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "tests::repo_config::bad_sections_warn_once_in_file_order",
            "--nocapture",
        ])
        .env("PORTUALE_REPOS_WARN_PROBE", root)
        .output()
        .expect("probe subprocess runs");
    assert!(
        out.status.success(),
        "probe child failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|line| line.contains("zeta-noloc") || line.contains("alpha-badloc"))
        .map(str::to_string)
        .collect();
    assert_eq!(
        lines,
        [
            "!!! Section 'zeta-noloc' in repos.conf is missing location attribute".to_string(),
            format!(
                "!!! Section 'alpha-badloc' in repos.conf has location attribute set \
                 to nonexistent directory: '{}'",
                bad_dir.display()
            ),
        ]
    );
}
