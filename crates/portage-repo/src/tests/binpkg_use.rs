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

/// The memo is transparent and separates every key axis.
#[test]
fn binpkg_respect_use_memo_hits_on_repeat_and_separates_its_inputs() {
    let dir = slotundo_temp_dir("binpkg-respect-use-memo");
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join("metadata/md5-cache/dev-libs")).unwrap();
    fs::write(
        repo.join("metadata/md5-cache/dev-libs/memo-1.0"),
        "EAPI=8\nIUSE=foo newflag\nKEYWORDS=amd64\nSLOT=0\n",
    )
    .unwrap();
    let mut ebuild = candidate("1.0", &["amd64"]);
    ebuild.repo_location = repo;
    ebuild.iuse = "foo newflag".to_string();
    let binary = |iuse: &str, baked: &[&str]| {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = iuse.to_string();
        c.binary_use = Some(baked.iter().map(|s| s.to_string()).collect());
        c
    };
    let mut config = portage_profile::Config {
        conf_use_tokens: vec!["foo".to_string()],
        ..Default::default()
    };
    let call = |config: &portage_profile::Config, binary_iuse: &str, baked: &[&str]| {
        binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
            candidate: &binary(binary_iuse, baked),
            ebuild_at_version: Some(&ebuild),
            category: "dev-libs",
            package: "memo",
            config,
            newuse: false,
            changed_use: false,
            respect_use: true,
        })
    };

    // Unique inputs: a miss; newflag rejects.
    let before = binpkg_respect_use_ok_uncached_calls();
    assert!(!call(&config, "foo", &["foo"]));
    let after_first = binpkg_respect_use_ok_uncached_calls();
    assert_eq!(after_first, before + 1, "first call is a miss");
    assert!(!call(&config, "foo", &["foo"]));
    assert_eq!(
        binpkg_respect_use_ok_uncached_calls(),
        after_first,
        "a repeat with identical arguments must hit the memo"
    );
    // Differing binary IUSE misses (only the counter proves it).
    assert!(!call(&config, "foo newflag", &["foo newflag"]));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 1);
    // A differing baked USE misses.
    assert!(!call(&config, "foo", &[]));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 2);
    // Two repo_locations with different md5 IUSE behind the same candidate iuse
    // must not share.
    let ebuild_at_same_iuse = |tag: &str, md5_iuse: &str| {
        let repo = dir.join(format!("repo-same-iuse-{tag}"));
        fs::create_dir_all(repo.join("metadata/md5-cache/dev-libs")).unwrap();
        fs::write(
            repo.join("metadata/md5-cache/dev-libs/memo-1.0"),
            format!("EAPI=8\nIUSE={md5_iuse}\nKEYWORDS=amd64\nSLOT=0\n"),
        )
        .unwrap();
        let mut e = candidate("1.0", &["amd64"]);
        e.repo_location = repo;
        e.iuse = "foo".to_string();
        e
    };
    let keep = ebuild_at_same_iuse("keep", "foo");
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary("foo", &["foo"]),
        ebuild_at_version: Some(&keep),
        category: "dev-libs",
        package: "memo",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 3);
    let reject = ebuild_at_same_iuse("reject", "foo newflag");
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary("foo", &["foo"]),
        ebuild_at_version: Some(&reject),
        category: "dev-libs",
        package: "memo",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 4);
    // The build identity separates instances: a miss, then a hit.
    let mut other_build = binary("foo", &["foo"]);
    other_build.build_id = Some("2".to_string());
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &other_build,
        ebuild_at_version: Some(&keep),
        category: "dev-libs",
        package: "memo",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 5);
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &other_build,
        ebuild_at_version: Some(&keep),
        category: "dev-libs",
        package: "memo",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));
    assert_eq!(
        binpkg_respect_use_ok_uncached_calls(),
        after_first + 5,
        "a repeat with identical arguments must hit the memo"
    );
    // A differing config misses.
    config.conf_use_tokens.clear();
    assert!(!call(&config, "foo", &["foo"]));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 6);
    let _ = fs::remove_dir_all(&dir);
}

/// A profile forced/masked flag never rejects through the IUSE presence
/// difference alone: masked keeps, forced rejects on enabled state.
#[test]
fn binpkg_respect_use_subtracts_forced_and_masked_flags_like_portage() {
    let dir = slotundo_temp_dir("binpkg-respect-use-forced");
    let binary = || {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = "foo".to_string();
        c.binary_use = Some(HashSet::from(["foo".to_string()]));
        c
    };
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
    config.conf_use_tokens = vec!["foo".to_string()];
    config.use_mask_force_levels = vec![portage_profile::UseMaskForceLevel {
        use_mask: vec!["mflag".to_string()],
        use_force: vec!["fflag".to_string()],
        ..Default::default()
    }];

    // Masked flag: presence subtracted, states agree (disabled both sides).
    let ebuild = ebuild_at("masked", "foo mflag");
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary(),
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: true,
        changed_use: false,
        respect_use: false
    }));
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

    // Forced flag: subtracted too, but the ebuild enables it, so the enabled
    // term rejects.
    let ebuild = ebuild_at("forced", "foo fflag");
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

    // No-ebuild arm: the binary's own force/mask set (Portage `useforce ∪ usemask`).
    let mut masked_binary = binary();
    masked_binary.iuse = "foo mflag".to_string();
    masked_binary.binary_use = Some(HashSet::from(["foo".to_string()]));
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &masked_binary,
        ebuild_at_version: None,
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: true,
        changed_use: false,
        respect_use: false
    }));
    let _ = fs::remove_dir_all(&dir);
}

/// The subtraction input is not intersected with the ebuild's IUSE: a masked flag
/// the ebuild dropped is forgiven on the presence term.
#[test]
fn binpkg_respect_use_forgives_a_profile_masked_flag_the_ebuild_iuse_dropped() {
    let dir = slotundo_temp_dir("binpkg-respect-use-dropped-masked");
    let binary = {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        // Baked IUSE carries the dropped flag; baked USE does not enable it.
        c.iuse = "foo removed".to_string();
        c.binary_use = Some(HashSet::from(["foo".to_string()]));
        c
    };
    let repo = dir.join("repo-dropped");
    fs::create_dir_all(repo.join("metadata/md5-cache/dev-libs")).unwrap();
    fs::write(
        repo.join("metadata/md5-cache/dev-libs/x-1.0"),
        "EAPI=8\nIUSE=foo\nKEYWORDS=amd64\nSLOT=0\n",
    )
    .unwrap();
    let mut ebuild = candidate("1.0", &["amd64"]);
    ebuild.repo_location = repo;
    ebuild.iuse = "foo".to_string();
    let mut config = test_config();
    config.conf_use_tokens = vec!["foo".to_string()];
    config.use_mask_force_levels = vec![portage_profile::UseMaskForceLevel {
        use_mask: vec!["removed".to_string()],
        ..Default::default()
    }];

    // Portage forgives `removed` despite the IUSE difference: kept under both gates.
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary,
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: true,
        changed_use: false,
        respect_use: false
    }));
    assert!(binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary,
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config,
        newuse: false,
        changed_use: false,
        respect_use: true
    }));

    // Control: unmasked, the presence difference rejects.
    let mut config_unmasked = config.clone();
    config_unmasked.use_mask_force_levels.clear();
    assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
        candidate: &binary,
        ebuild_at_version: Some(&ebuild),
        category: "dev-libs",
        package: "x",
        config: &config_unmasked,
        newuse: true,
        changed_use: false,
        respect_use: false
    }));
    let mut enabled_binary = binary.clone();
    enabled_binary
        .binary_use
        .as_mut()
        .unwrap()
        .insert("removed".to_string());
    for (newuse, changed_use, respect_use) in [
        (true, false, false),
        (false, false, true),
        (false, true, true),
    ] {
        assert!(!binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
            candidate: &enabled_binary,
            ebuild_at_version: Some(&ebuild),
            category: "dev-libs",
            package: "x",
            config: &config,
            newuse,
            changed_use,
            respect_use
        }));
    }
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

/// A user `package.use` `=`+build-id line applies only to the equal binary
/// instance (the same `_pkg_str` rule as accept_keywords above): with
/// `--binpkg-respect-use` and no ebuild at the version, Portage compares
/// the profile selection for the binary's own cpv against the baked USE,
/// so a line naming build 2 leaves build 1's effective USE alone.
/// Portage 3.0.82.2, S0 cell `b03-use-bid2-respect` (rc 1,
/// `masked by: use flag configuration mismatch`): full argv
/// `emerge -pv --package-moves=n --usepkgonly=y --binpkg-respect-use=y
/// dev-libs/t341-dep` with build 1 baked `USE: x` and build 2 baked `USE:`
/// empty. The line gives build 2 effective `{x}` (mismatching its baked
/// `{}`) while build 1 keeps effective `{}` (mismatching its baked `{x}`):
/// both builds rejected.
#[test]
fn user_package_use_build_id_line_applies_only_to_the_equal_instance() {
    let build = |bid: &str, baked: &[&str]| {
        let mut c = candidate("1", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.repo_name = "t341".to_string();
        c.slot = "0".to_string();
        c.sub_slot = "1".to_string();
        c.iuse = "x".to_string();
        c.binary_use = Some(baked.iter().map(|s| s.to_string()).collect());
        c.build_id = Some(bid.to_string());
        c
    };
    let config = portage_profile::Config {
        package_use_user: vec![("=dev-libs/t341-dep-1-2".to_string(), vec!["x".to_string()])],
        ..test_config()
    };
    for (bid, baked) in [("1", &["x"][..]), ("2", &[][..])] {
        assert!(
            !binpkg_respect_use_ok(BinpkgRespectUseOkArgs {
                candidate: &build(bid, baked),
                ebuild_at_version: None,
                category: "dev-libs",
                package: "t341-dep",
                config: &config,
                newuse: false,
                changed_use: false,
                respect_use: true
            }),
            "build {bid} must mismatch its baked USE"
        );
    }
}

/// Backlog #402: under `--binpkg-respect-use`, a top-level atom whose only
/// binaries are USE-mismatched reports Portage's masked block — one
/// cpv-deduped `use flag configuration mismatch` row — not the plain
/// `no binary packages` miss.
///
/// Portage 3.0.82.2, S0 cell `b03-use-bid2-respect` (rc 1, full argv
/// `emerge -pv --package-moves=n --usepkgonly=y --binpkg-respect-use=y
/// dev-libs/t341-dep`, re-run live on this host 2026-10-19): user
/// `package.use` `=dev-libs/t341-dep-1-2 x`, build 1 baked `USE: x`,
/// build 2 baked `USE:` empty — both builds mismatch, stderr
/// `!!! All ebuilds that could satisfy "dev-libs/t341-dep" ...` +
/// `- dev-libs/t341-dep-1::t341 (masked by: use flag configuration mismatch)`.
#[test]
fn binary_masked_report_lists_respect_use_mismatches_as_masked() {
    let build = |bid: &str, baked: &[&str]| {
        let mut c = candidate("1", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.repo_name = "t341".to_string();
        c.slot = "0".to_string();
        c.sub_slot = "1".to_string();
        c.iuse = "x".to_string();
        c.binary_use = Some(baked.iter().map(|s| s.to_string()).collect());
        c.build_id = Some(bid.to_string());
        c
    };
    let config = portage_profile::Config {
        package_use_user: vec![("=dev-libs/t341-dep-1-2".to_string(), vec!["x".to_string()])],
        ..test_config()
    };
    let binaries = vec![build("1", &["x"]), build("2", &[])];
    // The plain binary pool sees no masked row (every match is `is_visible`),
    // so the top-level path fell through to the plain miss.
    assert!(
        all_binary_masked_report(
            &binaries,
            "dev-libs/t341-dep",
            &config,
            "\"dev-libs/t341-dep\"",
        )
        .is_none()
    );
    // With the respect-use gate the same pool reports one cpv-deduped row.
    let report = all_binary_masked_report_with_respect_use(AllBinaryMaskedReportArgs {
        binaries: &binaries,
        atom_str: "dev-libs/t341-dep",
        config: &config,
        xinfo: "\"dev-libs/t341-dep\"",
        gate: BinaryRespectUseGate {
            respect_use: true,
            newuse: false,
            changed_use: false,
            user_patches: false,
            ebuilds: &[],
            changed_deps: None,
        },
        binary_header: false,
    })
    .expect("USE-mismatched binaries report as masked");
    assert!(
        report
            .contains("- dev-libs/t341-dep-1::t341 (masked by: use flag configuration mismatch)\n"),
        "{report}"
    );
    assert_eq!(
        report
            .matches("(masked by: use flag configuration mismatch)")
            .count(),
        1,
        "one cpv-deduped row for both builds:\n{report}"
    );
}
