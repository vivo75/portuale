use super::*;

#[test]
fn run_buildpkgonly_skips_already_installed_and_no_visible_candidate() {
    // Neither outcome ever has a resolvable version (see
    // entry_version), so this must return Ok(()) without attempting
    // any real execution at all -- proven by using a nonexistent
    // ROOT/PORTAGE_TMPDIR/PackageOptions that would fail loudly if
    // touched.
    let entries = vec![
        GraphEntry {
            discovery: 0,
            category: "dev-libs".into(),
            package: "samepkg".into(),
            outcome: PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: None,
            sub_slot: None,
            repo_name: None,
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        },
        GraphEntry {
            discovery: 0,
            category: "dev-libs".into(),
            package: "nosuchpkg".into(),
            outcome: PretendOutcome::NoVisibleCandidate,
            blockers: vec![],
            slot: None,
            sub_slot: None,
            repo_name: None,
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: vec![],
            source: CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        },
    ];
    let bogus = PathBuf::from("/nonexistent/does/not/exist");
    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &[],
        root: &bogus,
        portage_tmpdir: &bogus,
        options: &PackageOptions {
            debug: false,
            pkgdir: bogus.clone(),
            distdir: bogus.clone(),
            shell: PackageOptions::default().shell,
            // Pinned to "bzip2" (near-universal base package) rather
            // than real Default's "zstd", so these tests don't
            // depend on the test-running host actually having zstd
            // installed -- real xpak/tbz2 building is codec-
            // agnostic either way.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    assert!(result.is_ok());
}

#[test]
fn real_buildpkgonly_builds_a_real_binary_package_end_to_end() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "packagepkg".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        oldbest: vec![],
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }];

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            // Pinned to "bzip2" (near-universal base package) rather
            // than real Default's "zstd", so these tests don't
            // depend on the test-running host actually having zstd
            // installed -- real xpak/tbz2 building is codec-
            // agnostic either way.
            binpkg_compress: "bzip2".to_string(),
            // Pinned explicitly (backlog #173): this test asserts
            // XPAK magic via the xpak reader, not the (now gpkg)
            // default.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    assert!(result.is_ok(), "{result:?}");

    let tbz2 = pkgdir.join("dev-libs/packagepkg-1.0.tbz2");
    assert!(tbz2.is_file(), "{tbz2:?} should exist");
    let bytes = fs::read(&tbz2).unwrap();
    assert!(
        bytes.windows(8).any(|w| w == b"XPAKPACK"),
        "missing real XPAK magic bytes"
    );

    // #39: `packagepkg` installs a real file, so the
    // `_post_src_install_uid_fix` size walk must record a positive
    // `SIZE` in the archive's metadata.
    let meta = crate::binpkg::read_xpak_metadata(&tbz2).expect("xpak metadata parses");
    assert!(
        meta.get("SIZE")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .is_some_and(|n| n > 0),
        "SIZE must be positive for a package with a real file, got {:?}",
        meta.get("SIZE")
    );

    let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
    assert!(packages.contains("CPV: dev-libs/packagepkg-1.0"));
}

/// Backlog #99/#129 (Phase 2 S0 cell E): a matched `PORTAGE_TMPDIR`
/// that does not exist on disk is real's exact `_check_temp_dir`
/// failure, and `run_buildpkgonly` now re-derives the tmpdir per
/// entry instead of only ever using the run-wide one.
#[test]
fn run_buildpkgonly_recomputes_per_entry_portage_tmpdir_and_fails_on_a_missing_match() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();
    let missing = portage_tmpdir.join("no-such-buildpkgonly-tmpdir");

    let entries = vec![source_entry(
        "penvbuildpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )];
    let config = portage_profile::Config {
        package_env_vars: vec![(
            "dev-libs/penvbuildpkg".to_string(),
            vec![(
                "PORTAGE_TMPDIR".to_string(),
                missing.to_str().unwrap().to_string(),
            )],
        )],
        ..portage_profile::Config::default()
    };

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &config,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir,
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    let err = result.expect_err("a missing matched PORTAGE_TMPDIR must fail the entry");
    assert!(
        err.contains(&format!(
            "The directory specified in your PORTAGE_TMPDIR variable, '{}',",
            missing.display()
        )),
        "expected real's exact _check_temp_dir message, got: {err}"
    );
    assert!(
        err.contains(
            "does not exist.  Please create this directory or correct your PORTAGE_TMPDIR setting."
        ),
        "expected real's exact _check_temp_dir message, got: {err}"
    );
}

/// Backlog #130: `--buildpkgonly` re-derives `PackageOptions`'
/// binpkg-affecting fields (`binpkg_multi_instance` here) from the
/// per-entry `package.env` `FEATURES` fold, not the single shared,
/// Real per-package `FEATURES` (backlog #147 S1 ruling (i)):
/// the binpkg layout is run-wide-only (real's bintree binds its
/// allocator once, `bintree.py:529-531` -- S0 A1x/A2 prove both
/// directions), while the `BUILD_ID` export follows the entry's
/// own token (real `EbuildBinpkg.py:47-48`). With run-wide
/// multi-instance ON, a per-entry `-binpkg-multi-instance`
/// negation still takes the multi path (layout ignores it) but
/// exports no `BUILD_ID` (the gate honors it) -- S0 A2 shape.
/// Needs no Portage checkout: the install/package phases run through
/// the native helpers (#326 S2-S7), verified live with
/// `PORTUALE_PORTAGE_CHECKOUT=/nonexistent`.
#[test]
fn run_buildpkgonly_resolves_per_entry_binpkg_multi_instance_from_package_env() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![
        source_entry(
            "packagepkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
        source_entry(
            "newpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
    ];
    let config = portage_profile::Config {
        package_env_vars: vec![
            (
                "dev-libs/packagepkg".to_string(),
                vec![("FEATURES".to_string(), "binpkg-multi-instance".to_string())],
            ),
            (
                "dev-libs/newpkg".to_string(),
                vec![("FEATURES".to_string(), "-binpkg-multi-instance".to_string())],
            ),
        ],
        ..portage_profile::Config::default()
    };

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &config,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            binpkg_multi_instance: true,
            // Pinned explicitly (backlog #173): this test pins the
            // `.xpak` multi-instance paths, not the (now gpkg)
            // default.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    assert!(result.is_ok(), "{result:?}");

    // Layout is run-wide: the negated neighbour takes the multi
    // path too, not the single-instance default.
    let matched = pkgdir.join("dev-libs/packagepkg/packagepkg-1.0-1.xpak");
    assert!(
        matched.is_file(),
        "{matched:?} should exist -- run-wide multi-instance layout"
    );
    let negated = pkgdir.join("dev-libs/newpkg/newpkg-1.0-1.xpak");
    assert!(
        negated.is_file(),
        "{negated:?} should exist -- the per-entry negation must not move the layout"
    );
    // ...while the `BUILD_ID` export follows the entry's own
    // token: present for the match, absent for the negation.
    let index = std::fs::read_to_string(pkgdir.join("Packages")).expect("index written");
    let stanza = |cpv: &str| {
        index
            .split("\n\n")
            .find(|block| block.contains(&format!("CPV: {cpv}\n")))
            .unwrap_or_else(|| panic!("{cpv} stanza should exist"))
    };
    assert!(
        stanza("dev-libs/packagepkg-1.0")
            .lines()
            .any(|line| line.starts_with("BUILD_ID: 1")),
        "the matched entry's index stanza should carry its BUILD_ID"
    );
    assert!(
        stanza("dev-libs/newpkg-1.0")
            .lines()
            .all(|line| !line.starts_with("BUILD_ID")),
        "the negated entry's index stanza should carry no BUILD_ID"
    );
}

/// Real per-package `BINPKG_COMPRESS` (backlog #147 S2): a
/// `package.env` scalar match must reach the xpak tar pipe for
/// that entry only (real `doebuild_environment()` reads the
/// package's own `mysettings`, `doebuild.py:697` -- S0 B-xpak
/// proves gzip magic with a zstd run-wide). The observable is
/// the archive magic: the matched entry compresses with its own
/// codec, its unmatched neighbour in the same run keeps the
/// run-wide one. Needs no Portage checkout: the install/package
/// phases run through the native helpers (#326 S2-S7), verified live
/// with `PORTUALE_PORTAGE_CHECKOUT=/nonexistent`, like the
/// multi-instance test above.
#[test]
fn run_buildpkgonly_resolves_per_entry_binpkg_compress_from_package_env() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![
        source_entry(
            "penvbuildpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
        source_entry(
            "newpkg",
            PretendOutcome::New {
                version: "1.0".into(),
            },
        ),
    ];
    // In-memory package.env table (no committed fixture change --
    // the scalars are env-driven, not ebuild-driven): gzip for the
    // matched package only. Run-wide stays on the bzip2 fallback
    // (no `BINPKG_COMPRESS` in this config or process env), so the
    // neighbour pins the unchanged run-wide behavior too.
    let config = portage_profile::Config {
        package_env_vars: vec![(
            "dev-libs/penvbuildpkg".to_string(),
            vec![("BINPKG_COMPRESS".to_string(), "gzip".to_string())],
        )],
        ..portage_profile::Config::default()
    };

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &config,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    assert!(result.is_ok(), "{result:?}");

    let magic_of = |path: std::path::PathBuf| {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        assert!(
            bytes.len() > 4,
            "{path:?} should hold a real archive, not an empty file"
        );
        bytes[..4].to_vec()
    };
    let matched = pkgdir.join("dev-libs/penvbuildpkg-1.0.tbz2");
    assert_eq!(
        magic_of(matched.clone()),
        b"\x1f\x8b\x08\x00".to_vec(),
        "{matched:?} should be gzip-compressed -- the matched entry's package.env \
         BINPKG_COMPRESS=gzip must reach the xpak pipe, not just the run-wide bzip2"
    );
    let unmatched = pkgdir.join("dev-libs/newpkg-1.0.tbz2");
    assert_eq!(
        magic_of(unmatched.clone())[..3],
        b"BZh".to_vec(),
        "{unmatched:?} should keep the run-wide bzip2 bytes -- the neighbour must not \
         inherit the match"
    );
}

/// #37 S2: `--buildpkgonly` threads the same resolved env as the
/// `-b` merge path -- the archive's `metadata/USE`/`metadata/FEATURES`
/// (real `__dyn_install` writes both into `build-info` from the phase
/// env) and the `Packages` index `USE` field carry the effective
/// flags, not the raw harness env / empty standalone env.
#[allow(clippy::field_reassign_with_default)]
#[test]
fn buildpkgonly_with_resolved_config_writes_resolved_use_and_features() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();
    let mut config = portage_profile::Config::default();
    config.use_tokens = vec!["abi_x86_64 amd64 elibc_glibc kernel_linux".to_string()];
    config.iuse_effective = ["abi_x86_64", "amd64", "elibc_glibc", "kernel_linux"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    config
        .other_vars
        .insert("FEATURES".to_string(), "sandbox".to_string());

    let entries = vec![source_entry(
        "phaseenvpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )];
    run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &config,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            binpkg_format: "gpkg".to_string(),
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    })
    .expect("--buildpkgonly succeeds");

    let archive = pkgdir.join("dev-libs/phaseenvpkg-1.0.gpkg.tar");
    let meta = crate::binpkg::read_gpkg_metadata(&archive)
        .expect("portuale's gpkg reader parses the real writer's output");
    assert_eq!(
        meta.get("USE").map(String::as_str),
        Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
    );
    assert_eq!(meta.get("FEATURES").map(String::as_str), Some("sandbox"));
    assert_eq!(meta.get("SLOT").map(String::as_str), Some("0"));
    // #39: real `_post_src_install_write_metadata` always writes
    // `IUSE` (empty for a no-IUSE ebuild), the profile-computed
    // `IUSE_EFFECTIVE`, and `_post_src_install_uid_fix`'s own
    // `${D}`-walk `SIZE`.
    assert_eq!(meta.get("IUSE").map(String::as_str), Some(""));
    assert_eq!(
        meta.get("IUSE_EFFECTIVE").map(String::as_str),
        Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
    );
    // `phaseenvpkg` installs only a `keepdir` (the `.keep_*` marker
    // lands at merge time), so its image holds no regular file and
    // real's own walk writes `0` -- the member must be *present*.
    assert!(
        meta.get("SIZE")
            .and_then(|s| s.parse::<u64>().ok())
            .is_some(),
        "SIZE must be the real installed-size walk, got {:?}",
        meta.get("SIZE")
    );

    let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
    assert!(
        packages.contains("USE: abi_x86_64 amd64 elibc_glibc kernel_linux"),
        "Packages index USE missing the resolved flags:
{packages}"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
    let _ = fs::remove_dir_all(&pkgdir);
}

/// Backlog #42: real `_buildpkgonly_success_hook_exit` runs the
/// `clean` phase after a successful `--buildpkgonly` package, so the
/// builddir is gone afterwards (the phase itself, not a `noclean`
/// gate, honors `keepwork`).
#[test]
fn buildpkgonly_post_cleans_the_builddir() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![source_entry(
        "packagepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )];
    run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    })
    .expect("--buildpkgonly succeeds");

    // Backlog #173: the default format is gpkg now (real
    // `cnf/make.globals:43`), so the incidental artefact assertion
    // follows the default rather than pinning the old xpak shape.
    assert!(pkgdir.join("dev-libs/packagepkg-1.0.gpkg.tar").is_file());
    let builddir = portage_tmpdir.join("portage/dev-libs/packagepkg-1.0");
    assert!(
        !builddir.join(".installed").exists(),
        "the buildpkgonly tail must clean .installed"
    );
    assert!(
        !builddir.join("image").exists(),
        "the buildpkgonly tail must clean the image"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
    let _ = fs::remove_dir_all(&pkgdir);
}

#[test]
fn real_buildpkgonly_refuses_a_real_src_uri_with_no_manifest_entry() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "fetchpkg".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        oldbest: vec![],
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }];

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            // Pinned to "bzip2" (near-universal base package) rather
            // than real Default's "zstd", so these tests don't
            // depend on the test-running host actually having zstd
            // installed -- real xpak/tbz2 building is codec-
            // agnostic either way.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    // `fetchpkg`'s own fixture has a real, nonempty SRC_URI but no
    // Manifest entry at all -- refused before any network access is
    // even attempted (see `crate::fetch::fetch_src_uri`'s own doc
    // comment: unverifiable content is worse than a loud failure).
    let err = result.expect_err("an unverifiable SRC_URI must be refused");
    assert!(err.contains("no Manifest entry"), "{err}");
    assert!(
        !pkgdir.join("dev-libs/fetchpkg-1.0.tbz2").exists(),
        "must not have built anything"
    );
}

/// Without real `--keep-going`, a failing entry stops the whole run
/// immediately -- a later, independently-buildable entry in the same
/// list never even gets attempted.
#[test]
fn real_buildpkgonly_without_keep_going_stops_at_the_first_failure() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    // fetchpkg (no Manifest entry, always fails) listed *before*
    // packagepkg (builds cleanly) -- proves packagepkg is never even
    // attempted once fetchpkg fails.
    let entries = vec![
        buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
        buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
    ];

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        mode: StatusMode::for_tests(),
    });
    let err = result.expect_err("fetchpkg must still fail");
    assert!(err.contains("no Manifest entry"), "{err}");
    assert!(
        !pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists(),
        "packagepkg must never be attempted once fetchpkg fails without --keep-going"
    );
}

/// With real `--keep-going`, a failing entry does *not* stop the
/// run -- packagepkg still gets built despite fetchpkg's own
/// failure, and the final error names both entries.
#[test]
fn real_buildpkgonly_with_keep_going_builds_past_a_failure() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    let entries = vec![
        buildpkgonly_entry("dev-libs", "fetchpkg", "1.0"),
        buildpkgonly_entry("dev-libs", "packagepkg", "1.0"),
    ];

    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: true,
        mode: StatusMode::for_tests(),
    });
    let err = result.expect_err("fetchpkg still fails overall");
    assert!(err.contains("dev-libs/fetchpkg-1.0"), "{err}");
    assert!(err.contains("no Manifest entry"), "{err}");

    // Backlog #173: the default format is gpkg now (real
    // `cnf/make.globals:43`).
    assert!(
        pkgdir.join("dev-libs/packagepkg-1.0.gpkg.tar").is_file(),
        "packagepkg must still be built with --keep-going, despite fetchpkg's own failure"
    );
}
