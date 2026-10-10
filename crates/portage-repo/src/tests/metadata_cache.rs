use super::*;

/// `has_usable_md5_cache` is the per-repo "cache format available?"
/// bit real `porttree.py` derives from its auxdb/cache-format
/// selection: `fixtures/repo` ships `metadata/md5-cache`, a fresh
/// ebuild-only tree does not (so C2 must generate its metadata from
/// the ebuild).
#[test]
fn has_usable_md5_cache_is_the_presence_of_the_cache_dir() {
    assert!(has_usable_md5_cache(&fixtures_root().join("repo")));
    let bare = TempDir::new("portage-repo-has-usable-md5-cache-test").keep();
    assert!(!has_usable_md5_cache(&bare));
}

/// #46 S3 E3: with no provider registered (unit tests, `mrg`-only
/// builds) a present-but-stale entry is returned as-is -- the
/// pre-C2 contract; the read-path validation must not turn it into
/// an error.
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

/// #46 S3 E4: an entry whose ebuild is gone is trusted outright --
/// no validation can run, and every resolver reader reaches this
/// through an ebuild walk, so the entry is unreachable from
/// `list_candidates` anyway.
#[test]
fn md5_cache_entry_without_an_ebuild_is_returned() {
    let repo = masters_test_root("no-ebuild-md5");
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("pkg-1.0"), "EAPI=8\nSLOT=0\n").unwrap();
    let map = repo_aux_metadata(&repo, "dev-libs", "pkg-1.0").unwrap();
    assert_eq!(map.get("SLOT").map(String::as_str), Some("0"));
}

/// #55 S0 cells a-f (`TEST/findings/l2.md` "## #55 S0"): real
/// lowercases and splits `cache-formats`, auto-detects
/// `md5-dict` then `pms` from directory presence when it is empty,
/// and `get_pregenerated_cache` keeps only the **first known**
/// format -- an unknown name is skipped, no known name means no
/// pregen cache at all.
#[test]
fn pregen_md5_cache_follows_the_first_known_cache_format() {
    let root = masters_test_root("cache-formats");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("main/metadata")).unwrap();
    let layout = root.join("main/metadata/layout.conf");
    let formats = || {
        let repos = find_repos(&root).expect("repos.conf resolves");
        let main = repos.iter().find(|r| r.is_main).unwrap();
        (main.cache_formats.clone(), pregen_md5_cache_enabled(main))
    };

    // Cell a: no key, `metadata/md5-cache` present -> auto `md5-dict`.
    std::fs::create_dir_all(root.join("main/metadata/md5-cache")).unwrap();
    std::fs::write(&layout, "masters = \n").unwrap();
    assert_eq!(formats(), (vec!["md5-dict".to_string()], true));
    // Both directories present -> `md5-dict` still first.
    std::fs::create_dir_all(root.join("main/metadata/cache")).unwrap();
    assert_eq!(
        formats(),
        (vec!["md5-dict".to_string(), "pms".to_string()], true)
    );
    // Only `metadata/cache` -> auto `pms`, no md5-cache read.
    std::fs::remove_dir_all(root.join("main/metadata/md5-cache")).unwrap();
    assert_eq!(formats(), (vec!["pms".to_string()], false));
    // Neither directory -> empty list, no pregen cache at all.
    std::fs::remove_dir_all(root.join("main/metadata/cache")).unwrap();
    assert_eq!(formats(), (Vec::<String>::new(), false));

    // Cells b/e: explicit `md5-dict`, and `md5-dict pms` -> used
    // (`MD5-DICT` proves the value is lowercased first).
    std::fs::write(&layout, "cache-formats = MD5-DICT\n").unwrap();
    assert_eq!(formats(), (vec!["md5-dict".to_string()], true));
    std::fs::write(&layout, "cache-formats = md5-dict pms\n").unwrap();
    assert_eq!(
        formats(),
        (vec!["md5-dict".to_string(), "pms".to_string()], true)
    );
    // Cells c/d: `pms` first, with or without a following
    // `md5-dict` -> real reads `metadata/cache`; portuale (no pms
    // reader, L1) falls through to depcache/depend.
    std::fs::write(&layout, "cache-formats = pms\n").unwrap();
    assert_eq!(formats(), (vec!["pms".to_string()], false));
    std::fs::write(&layout, "cache-formats = pms md5-dict\n").unwrap();
    assert_eq!(
        formats(),
        (vec!["pms".to_string(), "md5-dict".to_string()], false)
    );
    // Cell f: only unknown names -> kept in the list (real skips
    // them at yield time) and no pregen cache.
    std::fs::write(&layout, "cache-formats = foo\n").unwrap();
    assert_eq!(formats(), (vec!["foo".to_string()], false));
}

/// #55 S0 cell g: `FEATURES=metadata-transfer` in the resolved
/// config stack disables the pregen cache rung (real
/// `porttree.py:322`), independent of `cache-formats`. The process
/// env is the same incremental layer (`const.INCREMENTALS`), so a
/// `FEATURES=… emerge` override resolves through here too.
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

/// #55 L3 (S0 cell h): real `egencache --update` writes
/// `metadata/cache` for `pms`, both dirs for `pms md5-dict`, and
/// errors when no known format remains; portuale's egencache-shaped
/// `--regen` may write the md5-cache half exactly when `md5-dict`
/// is among the resolved known formats, in any position.
#[test]
fn regen_writes_md5_cache_when_md5_dict_is_among_the_resolved_formats() {
    let root = masters_test_root("regen-formats");
    std::fs::create_dir_all(root.join("etc/portage")).unwrap();
    std::fs::write(
        root.join("etc/portage/repos.conf"),
        "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = main\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("main/metadata")).unwrap();
    let layout = root.join("main/metadata/layout.conf");
    let writes = |root: &Path| {
        let repos = find_repos(root).expect("repos.conf resolves");
        let main = repos.iter().find(|r| r.is_main).unwrap();
        regen_writes_md5_cache(main)
    };
    for (value, expected) in [
        ("pms", false),
        ("pms md5-dict", true),
        ("md5-dict pms", true),
        ("md5-dict", true),
        ("foo", false),
        ("foo md5-dict", true),
    ] {
        std::fs::write(&layout, format!("cache-formats = {value}\n")).unwrap();
        assert_eq!(writes(&root), expected, "cache-formats = {value}");
    }
    // No key, no directories -> empty list -> egencache's
    // `force=True` default `("md5-dict",)` still writes the cache
    // (unlike the reader, `get_pregenerated_cache(force=False)`).
    std::fs::write(&layout, "masters = \n").unwrap();
    assert!(writes(&root));
    // An auto-detected `pms` (the directory, no key) is the
    // skip case again.
    std::fs::create_dir_all(root.join("main/metadata/cache")).unwrap();
    assert!(!writes(&root));
}
