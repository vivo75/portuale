use super::*;

#[test]
fn locate_candidate_finds_the_real_fixture_ebuild_file() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let candidate = locate_candidate(&repos, "dev-libs", "packagepkg", "1.0").unwrap();
    let path = ebuild_path(&candidate, "dev-libs", "packagepkg", "1.0");
    assert!(path.ends_with("dev-libs/packagepkg/packagepkg-1.0.ebuild"));
    assert!(path.is_file(), "{path:?} should exist");
}

#[test]
fn locate_candidate_is_none_for_a_version_that_does_not_exist() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    assert!(locate_candidate(&repos, "dev-libs", "packagepkg", "99.0").is_none());
}

#[test]
fn entry_matches_any_checks_the_resolved_cpv_against_buildpkg_exclude_atoms() {
    let e = {
        let mut e = source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.2".into(),
            },
        );
        e.slot = Some("0".into());
        e.sub_slot = Some("0".into());
        e
    };
    assert!(!entry_matches_any(&e, &[]));
    assert!(entry_matches_any(&e, &["dev-libs/packagepkg".to_string()]));
    assert!(entry_matches_any(
        &e,
        &[">=dev-libs/packagepkg-1".to_string()]
    ));
    assert!(entry_matches_any(
        &e,
        &["dev-libs/packagepkg:0".to_string()]
    ));
    assert!(!entry_matches_any(&e, &["dev-libs/other".to_string()]));
    assert!(!entry_matches_any(
        &e,
        &["dev-libs/packagepkg:1".to_string()]
    ));
    assert!(!entry_matches_any(
        &e,
        &["<dev-libs/packagepkg-1".to_string()]
    ));
    // A non-mergeable outcome never matches.
    let ai = source_entry(
        "packagepkg",
        PretendOutcome::AlreadyInstalled {
            version: "1.2".into(),
        },
    );
    assert!(!entry_matches_any(
        &ai,
        &["dev-libs/packagepkg".to_string()]
    ));
}
