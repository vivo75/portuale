use super::*;

#[test]
fn candidate_use_flags_display_resolves_use_expand_flags_for_a_resume_entry() {
    // `emerge --resume` rebuilds each entry from a bare `cat/pkg-ver`; this
    // re-derives the `USE=` display for the fixture's USE_EXPAND setup.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let display = candidate_use_flags_display(&repos, &config, "dev-libs", "useexpandpkg", "1.0");
    assert_eq!(
        display,
        vec![
            ("video_cards_amdgpu".to_string(), false),
            ("video_cards_nvidia".to_string(), true),
        ]
    );
    // An unlocatable candidate is an empty list, never a panic.
    assert!(
        candidate_use_flags_display(&repos, &config, "dev-libs", "nope-does-not-exist", "9")
            .is_empty()
    );
}

#[test]
fn candidate_effective_use_flags_treats_a_missing_iuse_key_as_empty_iuse() {
    // Portage's md5-cache writer omits empty fields, but a package with an
    // empty IUSE still has the full effective USE.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    // `aubreaktop`'s cache has no `IUSE=` line.
    let flags = candidate_effective_use_flags(&repos, &config, "dev-libs", "aubreaktop", "1.0");
    assert!(!flags.is_empty(), "missing IUSE must not blank the USE set");
    assert!(
        flags
            .iter()
            .any(|f| f == "baz" || f == "confflag" || f == "foo"),
        "expected a profile USE flag, got {flags:?}"
    );
    assert!(candidate_effective_use_flags(&repos, &config, "dev-libs", "nope", "9").is_empty());
}

#[test]
fn running_root_satisfies_atom_checks_the_given_roots_own_real_vdb() {
    // The provider exists only as a hand-seeded vdb entry (no ebuild anywhere).
    let root = fixtures_root();
    let config = test_config();
    assert!(running_root_satisfies_atom(
        "dev-libs/rootdepsprovider",
        &root,
        &config
    ));
    assert!(running_root_satisfies_atom(
        "dev-libs/rootdepsprovider:0",
        &root,
        &config
    ));
    assert!(!running_root_satisfies_atom(
        "dev-libs/rootdepsprovider:1",
        &root,
        &config
    ));
    assert!(!running_root_satisfies_atom(
        "dev-libs/nonexistentprovider",
        &root,
        &config
    ));
}
