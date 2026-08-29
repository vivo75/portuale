use super::*;

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
