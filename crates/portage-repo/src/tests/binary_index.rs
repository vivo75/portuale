use super::*;

#[test]
fn list_remote_binary_candidates_reads_each_binrepos_own_packages_index() {
    // Only in the binhost index: no ebuild, no local entry.
    let binrepos = vec![fixture_binhost()];
    let local = BinaryIndex::from_pkgdir(&fixtures_root().join("pkgdir"));
    let cands = list_remote_binary_candidates(
        &binrepos,
        Path::new("/"),
        &local,
        "dev-libs",
        "remotebinpkg",
    );
    assert_eq!(cands.len(), 1);
    let c = &cands[0];
    assert_eq!(c.version, "1.0");
    assert!(c.remote, "a binhost candidate is marked remote");
    assert_eq!(c.repo_name, "gentoo", "from the index's own REPO field");
    assert_eq!(c.source, CandidateSource::Binary);

    // Already-local entries are dropped (Portage `bintree.isremote`).
    let none = list_remote_binary_candidates(
        &binrepos,
        Path::new("/"),
        &BinaryIndex::from_pkgdir(&fixtures_root().join("binhost")),
        "dev-libs",
        "remotebinpkg",
    );
    assert!(none.is_empty());

    // No binrepos: no remote candidates.
    assert!(
        list_remote_binary_candidates(&[], Path::new("/"), &local, "dev-libs", "remotebinpkg",)
            .is_empty()
    );
}
