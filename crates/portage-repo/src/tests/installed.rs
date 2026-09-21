use super::*;

/// `read_vdb_string` collapses multi-line values to one space-separated line
/// (Portage's `_aux_get` normalisation).
#[test]
fn read_vdb_string_collapses_multiline_values_to_one_space_separated_line() {
    let root = tmp_vdb(
        "dev-libs",
        "multiline-1.0",
        &[("RDEPEND", b"dev-libs/a\n\n  dev-libs/b\tdev-libs/c  \n")],
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multiline", "1.0", "RDEPEND"),
        "dev-libs/a dev-libs/b dev-libs/c"
    );
}

/// The trailing newline Portage's writer leaves must not survive the read.
#[test]
fn read_vdb_string_drops_the_trailing_newline() {
    let root = tmp_vdb("dev-libs", "newline-1.0", &[("repository", b"gentoo\n")]);
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "newline", "1.0", "repository"),
        "gentoo"
    );
}

/// An absent field file reads as `""`, not a panic.
#[test]
fn read_vdb_string_missing_file_is_still_empty() {
    let root = tmp_vdb("dev-libs", "absent-1.0", &[]);
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "absent", "1.0", "RDEPEND"),
        ""
    );
}

/// The committed multilinevdb fixture is the integration half of the same
/// contract: embedded newlines served exactly like Portage's `_aux_get`.
#[test]
fn read_vdb_string_normalises_the_committed_multiline_fixture() {
    let root = fixtures_root();
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "IUSE"),
        "alpha beta"
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "RDEPEND"),
        "dev-libs/samepkg virtual/libc"
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "DEFINED_PHASES"),
        "install info"
    );
}

/// `installed_candidates` serves repeats from its per-cp cache, invalidated
/// by the category dir mtime.
#[test]
fn installed_candidates_caches_per_cp_and_invalidates_on_vdb_change() {
    let root = tmp_vdb("dev-libs", "cachetest-1.0", &[]);
    let one = vec![("1.0".to_string(), "0".to_string(), "0".to_string())];
    // First call scans; the repeat is a cache hit.
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
    // A negative result is cached too.
    assert!(installed_candidates(&root, "dev-libs", "nocachetest").is_empty());
    // Adding a version bumps the category mtime and invalidates.
    let dir = root
        .join("var/db/pkg")
        .join("dev-libs")
        .join("cachetest-2.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SLOT"), "0\n").unwrap();
    let mut got = installed_candidates(&root, "dev-libs", "cachetest");
    got.sort();
    assert_eq!(
        got,
        vec![
            ("1.0".to_string(), "0".to_string(), "0".to_string()),
            ("2.0".to_string(), "0".to_string(), "0".to_string()),
        ]
    );
    // Removing it invalidates back.
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
}

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
