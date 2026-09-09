use super::*;

/// With no provider registered a present-but-stale entry is returned as-is.
#[test]
fn stale_md5_cache_entry_is_returned_without_a_provider() {
    let repo = masters_test_root("stale-md5");
    std::fs::create_dir_all(repo.join("dev-libs/pkg")).unwrap();
    std::fs::write(repo.join("dev-libs/pkg/pkg-1.0.ebuild"), "EAPI=8\n").unwrap();
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("pkg-1.0"),
        "EAPI=8\nKEYWORDS=~amd64\nSLOT=0\n_md5_=00000000000000000000000000000000\n",
    )
    .unwrap();
    let map = repo_aux_metadata(&repo, "dev-libs", "pkg-1.0").unwrap();
    assert_eq!(map.get("KEYWORDS").map(String::as_str), Some("~amd64"));
}

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
