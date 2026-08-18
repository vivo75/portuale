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
