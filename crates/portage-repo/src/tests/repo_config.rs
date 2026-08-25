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

    let repos = find_repos(&root).expect("repos.conf resolves");
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

    let repos = find_repos(&root).expect("repos.conf resolves");
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
    let repos = find_repos(&root).expect("repos.conf resolves");
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

    let repos = find_repos(&root).expect("repos.conf resolves");
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

    let repos = find_repos(&root).expect("repos.conf resolves");
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

    let repos = find_repos(&root).expect("repos.conf resolves");
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
    let repos = find_repos(&root).expect("repos.conf resolves");
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

    let repos = find_repos(&root).expect("repos.conf resolves");
    assert!(repos.iter().any(|r| r.name == "aliased-repo"));
    assert!(
        !repos.iter().any(|r| r.name == "dropped-repo"),
        "a section whose name mismatches the repo_name file with no alias is dropped"
    );
    assert!(repos.iter().all(|r| r.name != "dropped-section"));
}
