use super::*;

#[test]
fn still_listed_parents_applies_the_higher_slot_refinement() {
    // A higher different slot still matching the set atom suppresses the parent warning.
    let root = fixtures_root();
    let sets = vec![(
        "dualslotset".to_string(),
        vec!["dev-libs/dualslotpkg".to_string()],
    )];

    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

    assert_eq!(
        still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "2.0"),
        vec!["dualslotset"]
    );
}

#[test]
fn still_listed_parents_is_empty_when_no_set_lists_the_cp() {
    let root = fixtures_root();
    let sets = vec![("someset".to_string(), vec!["dev-libs/other".to_string()])];
    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());
}

/// A scratch config root resolving `FEATURES` from a `make.globals` carrying
/// `binpkg-multi-instance` plus `make_conf` as `make.conf`.
fn unmerge_features_probe_root(tag: &str, make_conf: &str) -> std::path::PathBuf {
    let root = TempDir::new(&format!("unmerge_features_{tag}")).keep();
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    let globals_dir = root.join("usr/share/portage/config");
    std::fs::create_dir_all(&prof).unwrap();
    std::fs::create_dir_all(&portage_dir).unwrap();
    std::fs::create_dir_all(&globals_dir).unwrap();
    std::fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    std::fs::write(portage_dir.join("make.conf"), make_conf).unwrap();
    std::fs::write(
        portage_dir.join("repos.conf"),
        "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
    )
    .unwrap();
    std::fs::write(
        globals_dir.join("make.globals"),
        "FEATURES=\"binpkg-multi-instance\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
    root
}
