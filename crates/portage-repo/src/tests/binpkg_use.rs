use super::*;

#[test]
fn binary_index_from_entries_feeds_list_binary_candidates_and_read_binary_metadata() {
    // Scanned entries are consumed like a parsed `Packages` file.
    let index = BinaryIndex::from_entries(vec![HashMap::from([
        ("CPV".to_string(), "dev-libs/scanned-1.0".to_string()),
        ("SLOT".to_string(), "0".to_string()),
        ("KEYWORDS".to_string(), "amd64".to_string()),
        ("REPO".to_string(), "gentoo".to_string()),
        ("RDEPEND".to_string(), "dev-libs/dep".to_string()),
    ])]);
    let cands = list_binary_candidates(&index, "dev-libs", "scanned");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].version, "1.0");
    assert_eq!(cands[0].repo_name, "gentoo");
    assert_eq!(cands[0].source, CandidateSource::Binary);

    let m = read_binary_metadata(&index, "dev-libs", "scanned", "1.0").unwrap();
    assert_eq!(m.get("RDEPEND").map(String::as_str), Some("dev-libs/dep"));
    assert!(read_binary_metadata(&index, "dev-libs", "scanned", "9.9").is_none());

    // A missing pkgdir is an empty index.
    assert!(BinaryIndex::from_pkgdir(Path::new("/nonexistent")).is_empty());
}

#[test]
fn read_binary_metadata_any_falls_back_to_a_binrepo_index() {
    let config = portage_profile::Config {
        binrepos: vec![fixture_binhost()],
        ..test_config()
    };
    // Falls back to the binrepo index.
    let m = read_binary_metadata_any(ReadBinaryMetadataAnyArgs {
        config: &config,
        root: Path::new("/"),
        local: &BinaryIndex::default(),
        category: "dev-libs",
        package: "remotebinpkg",
        version: "1.0",
    })
    .expect("the binrepo index has dev-libs/remotebinpkg-1.0");
    assert_eq!(m.get("SIZE").map(String::as_str), Some("573440"));
    assert_eq!(m.get("REPO").map(String::as_str), Some("gentoo"));
}
