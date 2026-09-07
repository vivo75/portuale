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
fn binpkg_respect_use_checks_the_ebuild_at_the_same_version() {
    // Portage compares baked IUSE/USE against the same-version ebuild's profile
    // view. Binary: IUSE=foo, baked USE=foo.
    let dir = slotundo_temp_dir("binpkg-respect-use");
    let binary = || {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = "foo".to_string();
        c.binary_use = Some(HashSet::from(["foo".to_string()]));
        c
    };
    // Fresh repo dir per cell: metadata memoises by path.
    let ebuild_at = |tag: &str, iuse: &str| {
        let repo = dir.join(format!("repo-{tag}"));
        fs::create_dir_all(repo.join("metadata/md5-cache/dev-libs")).unwrap();
        fs::write(
            repo.join("metadata/md5-cache/dev-libs/x-1.0"),
            format!("EAPI=8\nIUSE={iuse}\nKEYWORDS=amd64\nSLOT=0\n"),
        )
        .unwrap();
        let mut c = candidate("1.0", &["amd64"]);
        c.repo_location = repo;
        c.iuse = iuse.to_string();
        c
    };

    let mut config = test_config();
    // The profile selects foo.
    config.conf_use_tokens = vec!["foo".to_string()];

    // Matching sets: kept.
    let ebuild = ebuild_at("match", "foo");
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));

    // Ebuild gained newflag: the IUSE difference rejects (regression pin).
    let ebuild = ebuild_at("newflag", "foo newflag");
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    // Same under --newuse with respect-use off.
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: true,
        changed_use: false,
        respect_use: false
    }));
    // changed-use counts only enabled differences: kept.
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: true,
        respect_use: false
    }));
    // No gate: nothing compared.
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: false
    }));
    // No ebuild: Portage compares the profile selection over the binary's own
    // IUSE; foo is still selected, so the binary is kept.
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: None,
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    // Profile dropped foo while baked USE has it: rejected with no ebuild either.
    config.conf_use_tokens.clear();
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: None,
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));

    // Enabled-state flip with an ebuild: rejected.
    let ebuild = ebuild_at("flip", "foo");
    config.conf_use_tokens.clear();
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    let _ = fs::remove_dir_all(&dir);
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
