use super::*;

/// `FEATURES=metadata-transfer` in the resolved config disables the pregen cache.
#[test]
fn metadata_transfer_is_read_from_the_resolved_features() {
    let root = masters_test_root("metadata-transfer");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n",
    )
    .unwrap();
    std::fs::write(
        root.join("etc/portage/make.conf"),
        "FEATURES=\"metadata-transfer -parallel-fetch\"\n",
    )
    .unwrap();
    assert!(metadata_transfer_in_config(&root, &root));
    std::fs::write(
        root.join("etc/portage/make.conf"),
        "FEATURES=\"parallel-fetch\"\n",
    )
    .unwrap();
    assert!(!metadata_transfer_in_config(&root, &root));
}
