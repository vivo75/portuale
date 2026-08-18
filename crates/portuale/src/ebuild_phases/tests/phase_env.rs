use super::*;

#[test]
fn eclass_locations_value_quotes_the_containing_repo_root() {
    // Canonicalized first, as `compute_environment` always hands it.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo")
        .canonicalize()
        .unwrap();
    let pkg_dir = repo_root.join("dev-libs/eclasspkg");
    let value = eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root"));
    // Portage: bin/ebuild.sh, the PORTAGE_ECLASS_LOCATIONS eval.
    assert_eq!(value, format!("'{}'", repo_root.display()));
}

#[test]
fn eclass_locations_value_is_empty_outside_any_repo_checkout() {
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_value_none").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("dev-libs/standalone");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(
        eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root")),
        ""
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
