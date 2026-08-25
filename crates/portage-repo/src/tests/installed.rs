use super::*;

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
