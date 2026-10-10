use super::*;

#[test]
fn binary_index_from_entries_feeds_list_binary_candidates_and_read_binary_metadata() {
    // The `$PKGDIR` directory-scan path: `BinaryIndex::from_entries`
    // (what `config.scanned_binpkgs` becomes) is consumed exactly
    // like a parsed `<pkgdir>/Packages`.
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

    // A missing pkgdir is an empty index (unchanged tolerance).
    assert!(BinaryIndex::from_pkgdir(Path::new("/nonexistent")).is_empty());
}

#[test]
fn binpkg_respect_use_checks_the_ebuild_at_the_same_version() {
    // #69 S1: real compares a binary's baked IUSE/USE against the
    // *same-version ebuild's* profile view, not against the binary's
    // own selection. Binary: IUSE=foo, baked USE=foo.
    let dir = slotundo_temp_dir("binpkg-respect-use");
    let binary = || {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = "foo".to_string();
        c.binary_use = Some(HashSet::from(["foo".to_string()]));
        c
    };
    // Each call gets a fresh repo dir: `repo_aux_metadata` memoises
    // by path, and the cells rewrite the same cpv's IUSE.
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
    // The profile selects foo, matching the binary's baked USE.
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

    // The ebuild gained newflag: rejected by plain respect-use alone
    // (the IUSE-set difference), the regression pin.
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
    // ...and by --newuse even with respect-use off.
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
    // --reinstall=changed-use (respect-use off) only counts enabled
    // differences: both sides enable foo -> kept.
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
    // No gate (respect-use off, no reinstall form): nothing compared.
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
    // No same-version ebuild: real `depgraph.py:8265-8271`
    // (`pkgsettings.setcpv(pkg)`) compares against the *profile's*
    // selection for the binary cpv over the binary's own IUSE. The
    // profile still selects foo here, so the binary is kept.
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
    // The profile no longer selects foo while the binary's baked USE
    // has it (the #69 R0 n1 shape) -> rejected, even though there is
    // no ebuild to compare against.
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

    // Enabled-state flip: the profile no longer selects foo while
    // the binary was built with it -> rejected under respect-use.
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

/// #117: the `binpkg_respect_use_ok` memo is transparent (a repeated
/// call runs the body once) and must distinguish every key axis: the
/// ebuild's own metadata, the binary's IUSE and baked USE, the three
/// flags, and the config.
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

    // Unique inputs -> certainly a miss; the ebuild gained newflag, so
    // the binary is rejected.
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
    // A differing binary IUSE misses. Here the baked USE enables
    // newflag while the ebuild's effective USE does not, so the
    // enabled-state term rejects: false, like the first call (only
    // the counter can prove the miss, which is exactly why it exists).
    assert!(!call(&config, "foo newflag", &["foo newflag"]));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 1);
    // A differing baked USE misses.
    assert!(!call(&config, "foo", &[]));
    assert_eq!(binpkg_respect_use_ok_uncached_calls(), after_first + 2);
    // Backlog #122 (audit O4): two `repo_location`s with different
    // md5-cache `IUSE` behind identical candidate `iuse` must not
    // share -- the uncached body reads the ebuild's md5-cache entry
    // through `candidate_iuse_and_use`. The old key hashed only the
    // candidate's own `iuse` and would have served the first call's
    // `true` for the second.
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
    // Backlog #124 E3: the build identity separates instances -- the
    // same CPV at a different BUILD_ID is a miss, and a repeat hits.
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

/// #69 R2 / real `depgraph.py:3148-3150` (`flags -= forced_flags`):
/// a flag the profile force/masks never rejects a binary through the
/// IUSE *presence* difference alone. The ebuild gained a **masked**
/// flag (`mflag`, disabled on the ebuild side) and, in the second
/// cell, a **forced** one (`fflag`, enabled): the masked cell keeps
/// the binary (this is the R0 n3a shape and the regression pin for
/// the subtraction), the forced cell rejects through the
/// enabled-within-IUSE term instead (R0 n3b).
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

    // Masked flag added to the ebuild's IUSE: presence difference is
    // subtracted, enabled states agree (disabled on both sides).
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

    // Forced flag added to the ebuild's IUSE: presence difference is
    // subtracted too, but the ebuild side now *enables* it, so the
    // enabled-within-IUSE term rejects -- exactly real's n3b cell.
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

    // The no-ebuild arm passes the binary cpv's own force/mask set
    // (real's `pkgsettings.useforce ∪ usemask`), though its presence
    // term is empty by construction there (`cur_iuse = old_iuse`),
    // so the outcome is the same keep either way.
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

/// Audit D1 (`LLM/02.68-74-review-through-B5.md`): the subtraction
/// input is not intersected with the ebuild's IUSE. A flag the
/// profile masks that the new ebuild's IUSE has **dropped** is
/// forgiven on the presence term -- real keeps the binary (probed
/// live against 3.0.82.2: `_reinstall_for_flags` returns `None`;
/// `depgraph.py:8264` chains `myeb.use.force/mask` unfiltered, and
/// `config.setcpv`'s `getUseMask(cpv_slot)` never filters by IUSE).
/// The old filtered set (R2's `forced_or_masked_flags`) lost the
/// flag, so the presence difference rejected the binary.
#[test]
fn binpkg_respect_use_forgives_a_profile_masked_flag_the_ebuild_iuse_dropped() {
    let dir = slotundo_temp_dir("binpkg-respect-use-dropped-masked");
    let binary = {
        let mut c = candidate("1.0", &["amd64"]);
        c.source = CandidateSource::Binary;
        // Baked IUSE carries the flag the ebuild dropped; the baked
        // USE does *not* enable it (a masked flag cannot be baked
        // enabled -- the live probe's `orig_use` is `{"foo"}`).
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

    // Real's subtraction forgives `removed` despite the IUSE
    // difference (binary had it, ebuild does not): the binary is
    // kept under --newuse and under respect-use alike.
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

    // Control, the pre-D1 shape: without the profile masking the
    // flag the presence difference does reject -- the profile
    // masking, not the filter, is what carries the keep.
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
    // Not in the (empty) local index; found in the binrepo's cached
    // `Packages`, SIZE and REPO carried through.
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
