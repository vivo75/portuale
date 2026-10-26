use super::*;

#[test]
fn run_buildpkgonly_skips_already_installed_and_no_visible_candidate() {
    // Neither outcome has a resolvable version, so this must return `Ok`
    // without attempting any execution (proven by the nonexistent paths).
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
            build_time: None,
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
            build_time: None,
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
            // Pinned to "bzip2" rather than the default "zstd", so the test
            // does not depend on the host having zstd installed.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
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
        build_time: None,
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
            // Pinned to "bzip2" rather than the default "zstd", so the test
            // does not depend on the host having zstd installed.
            binpkg_compress: "bzip2".to_string(),
            // Pinned explicitly: this test asserts XPAK magic, not the
            // (now gpkg) default.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
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

    // `packagepkg` installs a file, so the size walk must record a
    // positive `SIZE` in the archive's metadata.
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

/// A matched `PORTAGE_TMPDIR` that does not exist fails the entry with
/// Portage's exact `_check_temp_dir` message. Expected value from Portage source.
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
        jobs: 1,
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

/// `--buildpkgonly` re-derives `PackageOptions`' binpkg-affecting fields from the
/// per-entry `package.env` `FEATURES` fold: the layout stays run-wide while the
/// `BUILD_ID` export follows the entry's own token. Expected value from Portage source.
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
            // Pinned explicitly: this test pins the `.xpak` multi-instance
            // paths, not the (now gpkg) default.
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
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

/// A `package.env` scalar match reaches the xpak tar pipe for that entry only:
/// the matched entry compresses with its own codec, its unmatched neighbour
/// keeps the run-wide one. Expected value from Portage source.
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
    // In-memory package.env table (no committed fixture change): gzip for
    // the matched package only; run-wide stays on bzip2, so the neighbour
    // pins the unchanged run-wide behavior too.
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
        jobs: 1,
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

/// `--buildpkgonly` threads the same resolved env as the `-b` merge path: the
/// archive's `metadata/USE`/`metadata/FEATURES` and the `Packages` index `USE`
/// carry the effective flags. Expected value from Portage source.
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
        jobs: 1,
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
    // Portage always writes `IUSE` (empty for a no-IUSE ebuild), the
    // profile-computed `IUSE_EFFECTIVE`, and the installed-size `SIZE` walk.
    assert_eq!(meta.get("IUSE").map(String::as_str), Some(""));
    assert_eq!(
        meta.get("IUSE_EFFECTIVE").map(String::as_str),
        Some("abi_x86_64 amd64 elibc_glibc kernel_linux")
    );
    // `phaseenvpkg` installs only a `keepdir`, so its image holds no regular
    // file and the walk writes `0` -- the member must still be *present*.
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

/// A successful `--buildpkgonly` package runs the `clean` phase afterwards,
/// so the builddir is gone. Expected value from Portage source.
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
        jobs: 1,
        mode: StatusMode::for_tests(),
    })
    .expect("--buildpkgonly succeeds");

    // The default format is gpkg now, so the artefact assertion follows
    // the default rather than pinning the old xpak shape.
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
        build_time: None,
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
            // Pinned to "bzip2" rather than the default "zstd", so the test
            // does not depend on the host having zstd installed.
            binpkg_compress: "bzip2".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
        mode: StatusMode::for_tests(),
    });
    // `fetchpkg`'s fixture has a nonempty SRC_URI but no Manifest entry --
    // refused before any network access is even attempted (unverifiable
    // content is worse than a loud failure).
    let err = result.expect_err("an unverifiable SRC_URI must be refused");
    assert!(err.contains("no Manifest entry"), "{err}");
    assert!(
        !pkgdir.join("dev-libs/fetchpkg-1.0.tbz2").exists(),
        "must not have built anything"
    );
}

/// Without `--keep-going`, a failing entry stops the whole run: a later,
/// independently-buildable entry is never attempted.
#[test]
fn real_buildpkgonly_without_keep_going_stops_at_the_first_failure() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();

    // fetchpkg (always fails) listed *before* packagepkg (builds cleanly).
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
        jobs: 1,
        mode: StatusMode::for_tests(),
    });
    let err = result.expect_err("fetchpkg must still fail");
    assert!(err.contains("no Manifest entry"), "{err}");
    assert!(
        !pkgdir.join("dev-libs/packagepkg-1.0.tbz2").exists(),
        "packagepkg must never be attempted once fetchpkg fails without --keep-going"
    );
}

/// At `--jobs 1` the up-front pass replays each entry before the next
/// starts, so the probe entry finds no buffered log and both packages
/// build. Portage runs the pass with `--jobs` (`Scheduler.py:961-964`),
/// clamped to the entry count -- not the entry count itself.
#[test]
fn run_buildpkgonly_pretend_pass_uses_jobs_not_the_entry_count() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();
    let new = |p: &str| buildpkgonly_entry("dev-libs", p, "1.0");
    let entries = vec![new("schedppseqa"), new("schedppseqb")];
    run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            config_root: config_root.clone(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
        mode: StatusMode::for_tests(),
    })
    .expect("--buildpkgonly succeeds: the probe saw live replay");
    for p in ["schedppseqa", "schedppseqb"] {
        assert!(
            pkgdir.join(format!("dev-libs/{p}-1.0.tbz2")).is_file(),
            "{p} built"
        );
    }
    let _removed = std::fs::remove_dir_all(&root);
    let _removed = std::fs::remove_dir_all(&portage_tmpdir);
    let _removed = std::fs::remove_dir_all(&pkgdir);
}

/// Above one job the same up-front pass stays buffered: the probe entry
/// finds the first entry's still-buffered log and aborts before anything
/// builds.
#[test]
fn run_buildpkgonly_pretend_pass_stays_buffered_above_one_job() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();
    let new = |p: &str| buildpkgonly_entry("dev-libs", p, "1.0");
    let entries = vec![new("schedppseqa"), new("schedppseqb")];
    let err = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            config_root: config_root.clone(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 3,
        mode: StatusMode::for_tests(),
    })
    .expect_err("the probe fails on the buffered log");
    assert!(
        is_reported_failure(&err),
        "pretend failure is already reported: {err}"
    );
    for p in ["schedppseqa", "schedppseqb"] {
        assert!(
            !pkgdir.join(format!("dev-libs/{p}-1.0.tbz2")).exists(),
            "{p} never builds after the pretend failure"
        );
    }
    let _removed = std::fs::remove_dir_all(&root);
    let _removed = std::fs::remove_dir_all(&portage_tmpdir);
    let _removed = std::fs::remove_dir_all(&pkgdir);
}

/// With `--keep-going`, a failing entry does *not* stop the run: the rest
/// still builds, and the final error names the failure.
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
        jobs: 1,
        mode: StatusMode::for_tests(),
    });
    let err = result.expect_err("fetchpkg still fails overall");
    assert!(err.contains("dev-libs/fetchpkg-1.0"), "{err}");
    assert!(err.contains("no Manifest entry"), "{err}");

    // The default format is gpkg now.
    assert!(
        pkgdir.join("dev-libs/packagepkg-1.0.gpkg.tar").is_file(),
        "packagepkg must still be built with --keep-going, despite fetchpkg's own failure"
    );
}

/// The scratch `up540` tree for the user-patch pins: one trivial ebuild with
/// real `src_unpack`/`src_install` bodies (so `src_prepare` -- hence the
/// vendored `eapply_user` -- runs; a `DEFINED_PHASES=install`-only ebuild
/// would skip it on both managers), plus a `repos.conf` for `find_repos`
/// and, unless `patches` is empty, a local patch set under the config root.
/// Returns the config root, the repo dir, and the expected combined patch
/// digest recomputed the way `eapply_user` does (per-patch sha256, then
/// sha256 over the concatenated hex).
fn stage_user_patch_tree(
    scratch: &std::path::Path,
    patches: &[(&str, &[u8])],
) -> (std::path::PathBuf, std::path::PathBuf, String) {
    use sha2::Digest as _;

    let cfg = scratch.join("cfg");
    let repo = scratch.join("repo");
    let ebuild = "EAPI=8\nDESCRIPTION=\"backlog #540 pin\"\nHOMEPAGE=\"https://example.com\"\nS=\"${WORKDIR}/src\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nsrc_unpack() { mkdir -p \"${S}\"; }\nsrc_install() { :; }\n";
    let ebuild_path = repo.join("app-misc/ppatch/ppatch-1.0.ebuild");
    fs::create_dir_all(ebuild_path.parent().unwrap()).unwrap();
    fs::write(&ebuild_path, ebuild).unwrap();
    // The hand-written md5-cache entry `list_candidates` reads (real MD5,
    // like every committed fixture entry).
    let digest = portage_util::hex_lower(&md5::Md5::digest(ebuild.as_bytes()));
    let cache_dir = repo.join("metadata/md5-cache/app-misc");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(
        cache_dir.join("ppatch-1.0"),
        format!("DEFINED_PHASES=install unpack\nDESCRIPTION=backlog #540 pin\nEAPI=8\nKEYWORDS=amd64\nSLOT=0\n_md5_={digest}\n"),
    )
    .unwrap();
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::write(repo.join("profiles/repo_name"), "up540\n").unwrap();
    fs::write(repo.join("profiles/categories"), "app-misc\n").unwrap();
    fs::create_dir_all(repo.join("metadata")).unwrap();
    fs::write(repo.join("metadata/layout.conf"), "masters = testrepo\n").unwrap();
    fs::create_dir_all(cfg.join("etc/portage/repos.conf")).unwrap();
    fs::write(
        cfg.join("etc/portage/repos.conf/r.conf"),
        format!(
            "[DEFAULT]\nmain-repo = up540\n\n[testrepo]\nlocation = {}\n\n[up540]\nlocation = {}\n",
            fixtures_root().display(),
            repo.display()
        ),
    )
    .unwrap();
    let mut inner_hex = String::new();
    let mut sorted: Vec<(&str, &[u8])> = patches.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (_, body) in &sorted {
        let inner = portage_util::hex_lower(&sha2::Sha256::digest(body));
        inner_hex.push_str(&inner);
    }
    if !patches.is_empty() {
        let patch_dir = cfg.join("etc/portage/patches/app-misc/ppatch");
        fs::create_dir_all(&patch_dir).unwrap();
        for (basename, body) in patches {
            fs::write(patch_dir.join(basename), body).unwrap();
        }
    }
    let outer = portage_util::hex_lower(&sha2::Sha256::digest(inner_hex.as_bytes()));
    (cfg, repo, outer)
}

/// The expected `user_patch.digests` bytes: the `hash\0basename\0` rows in
/// sorted-basename order (the shell's `printf '%s\0' hash basename`).
fn expected_digests_file(patches: &[(&str, &[u8])]) -> Vec<u8> {
    use sha2::Digest as _;

    let mut sorted: Vec<(&str, &[u8])> = patches.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Vec::new();
    for (basename, body) in &sorted {
        let inner = portage_util::hex_lower(&sha2::Sha256::digest(body));
        out.extend_from_slice(inner.as_bytes());
        out.push(0);
        out.extend_from_slice(basename.as_bytes());
        out.push(0);
    }
    out
}

/// Runs `--buildpkgonly` for `app-misc/ppatch-1.0` from the staged tree and
/// returns the built multi-instance gpkg path.
fn build_ppatch_binpkg(cfg: &std::path::Path, pkgdir: &std::path::Path) -> std::path::PathBuf {
    let repos = find_repos(cfg).unwrap();
    let entries = vec![GraphEntry {
        discovery: 0,
        category: "app-misc".into(),
        package: "ppatch".into(),
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("up540".into()),
        use_flags_display: vec![],
        use_expand_display: vec![],
        use_expand_display_p: vec![],
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        oldbest: vec![],
        required_by: vec![],
        source: CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        build_time: None,
        deps: Vec::new(),
    }];
    let result = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &tempdir(),
        portage_tmpdir: &tempdir(),
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.to_path_buf(),
            distdir: tempdir(),
            config_root: cfg.to_path_buf(),
            shell: PackageOptions::default().shell,
            binpkg_multi_instance: true,
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
        mode: StatusMode::for_tests(),
    });
    assert!(result.is_ok(), "{result:?}");
    let gpkg = pkgdir.join("app-misc/ppatch/ppatch-1.0-1.gpkg.tar");
    assert!(gpkg.is_file(), "multi-instance gpkg layout: {gpkg:?}");
    gpkg
}

/// A build with user patches applied records them exactly like Portage --
/// `build-info/USER_PATCHES` (sha256 over the concatenated per-patch
/// sha256s) and `build-info/user_patch.digests` (`hash\0basename\0` per
/// patch) packed into the gpkg `metadata.tar`, plus the `USER_PATCHES`
/// stanza key on inject -- while a patch-free build
/// carries neither (host `up-d` stanza has no key). The digests are
/// byte-identical to the vendored `eapply_user` both managers run, so the
/// expected value is recomputed here from the patch bytes, not read back
/// from portuale output.
#[test]
fn buildpkgonly_records_user_patches_in_metadata_and_index() {
    let patch_body = b"--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-ppatch\n";
    let scratch = tempdir();
    let (cfg, _repo, outer) = stage_user_patch_tree(&scratch, &[("user.patch", patch_body)]);
    let pkgdir = tempdir();
    let gpkg = build_ppatch_binpkg(&cfg, &pkgdir);
    let meta = crate::binpkg::read_gpkg_metadata(&gpkg).expect("gpkg metadata parses");
    assert_eq!(
        meta.get("USER_PATCHES").map(String::as_str),
        Some(outer.as_str()),
        "metadata USER_PATCHES is the shell-computed digest"
    );
    assert_eq!(
        meta.get("user_patch.digests").map(|s| s.as_bytes()),
        Some(expected_digests_file(&[("user.patch", patch_body)]).as_slice()),
        "metadata digests file is the NUL-separated (hash, basename) rows"
    );
    let packages = fs::read_to_string(pkgdir.join("Packages")).unwrap();
    assert!(
        packages.contains(&format!("USER_PATCHES: {outer}\n")),
        "inject carries the key, like the host up-a stanza: {packages}"
    );

    // The patch-free side: without local patches the build records
    // neither the metadata members nor the stanza key (host `up-d`).
    fs::remove_dir_all(cfg.join("etc/portage/patches")).unwrap();
    let pkgdir2 = tempdir();
    let gpkg2 = build_ppatch_binpkg(&cfg, &pkgdir2);
    let meta2 = crate::binpkg::read_gpkg_metadata(&gpkg2).expect("gpkg metadata parses");
    assert!(
        !meta2.contains_key("USER_PATCHES") && !meta2.contains_key("user_patch.digests"),
        "no patch members without patches: {:?}",
        meta2.keys().collect::<Vec<_>>()
    );
    let packages2 = fs::read_to_string(pkgdir2.join("Packages")).unwrap();
    assert!(
        !packages2.contains("USER_PATCHES"),
        "no stanza key without patches, like the host up-d stanza"
    );
}

/// A binary merge installs both binpkg metadata members into the VDB
/// (`USER_PATCHES` and `user_patch.digests`), which is what a later
/// `user_patch.digests`), which is what a later `--usepkg` resolve and the
/// extraction table read. Pinned end to end: build under patches, merge the
/// built gpkg with `merge_binpkg`, read the VDB back byte-for-byte.
#[test]
fn merge_binpkg_copies_user_patch_files_into_the_vdb() {
    let patch_body = b"--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1 @@\n+hello-ppatch\n";
    let scratch = tempdir();
    let (cfg, _repo, outer) = stage_user_patch_tree(&scratch, &[("user.patch", patch_body)]);
    let pkgdir = tempdir();
    let gpkg = build_ppatch_binpkg(&cfg, &pkgdir);

    let root = tempdir();
    fs::create_dir_all(&root).unwrap();
    let outcome = crate::ebuild_merge::merge_binpkg(
        &gpkg,
        &root,
        &tempdir(),
        &crate::ebuild_merge::MergeOptions::default(),
    )
    .expect("merge succeeds");
    assert_eq!(outcome.status, 0);

    // Both members land in the VDB verbatim (Portage installs every
    // build-info file; the manual probe agreed byte-for-byte).
    let vdb = root.join("var/db/pkg/app-misc/ppatch-1.0");
    assert_eq!(
        fs::read_to_string(vdb.join("USER_PATCHES")).unwrap().trim(),
        outer.as_str(),
        "VDB USER_PATCHES is the shell-computed digest"
    );
    assert_eq!(
        fs::read(vdb.join("user_patch.digests")).unwrap(),
        expected_digests_file(&[("user.patch", patch_body)]),
        "VDB user_patch.digests is the NUL-separated rows"
    );
}

/// The scheduler-driven `--buildpkgonly` path runs the same up-front
/// `pkg_pretend` pass as the merge scheduler (oracle: `--buildpkgonly -j`
/// of the `10-pretend` tree). A dying pretend aborts before anything builds, with
/// the already-reported failure -- not a per-entry "build failed".
#[test]
fn run_buildpkgonly_pretend_failure_aborts_before_anything_builds() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let pkgdir = tempdir();
    let new = |p: &str| buildpkgonly_entry("dev-libs", p, "1.0");
    let entries = vec![new("schedppfail"), new("schedwait")];
    let err = run_buildpkgonly(RunBuildpkgonlyArgs {
        entries: &entries,
        config: &portage_profile::Config::default(),
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &PackageOptions {
            debug: false,
            pkgdir: pkgdir.clone(),
            distdir: tempdir(),
            config_root: config_root.clone(),
            shell: PackageOptions::default().shell,
            binpkg_compress: "bzip2".to_string(),
            binpkg_format: "xpak".to_string(),
            ..PackageOptions::default()
        },
        keep_going: false,
        jobs: 1,
        mode: StatusMode::for_tests(),
    })
    .expect_err("a dying pretend fails the buildpkgonly run");
    assert!(
        is_reported_failure(&err),
        "pretend failure is already reported, like the scheduler pass: {err}"
    );
    assert!(
        !pkgdir.join("dev-libs/schedppfail-1.0.tbz2").exists(),
        "the failing entry builds nothing"
    );
    assert!(
        !pkgdir.join("dev-libs/schedwait-1.0.tbz2").exists(),
        "the slow entry never builds"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&pkgdir);
}
