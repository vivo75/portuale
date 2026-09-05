use super::*;

#[test]
fn resolve_info_candidate_reports_the_best_visible_ebuild_and_its_use_display() {
    let repos = find_repos(&fixtures_root()).expect("fixture repos.conf must resolve");
    // dev-libs/pkginfopkg: IUSE="+alpha beta", DEFINED_PHASES=info.
    let info = resolve_info_candidate(&repos, "dev-libs/pkginfopkg", &test_config())
        .expect("resolves")
        .expect("has a visible candidate");
    assert_eq!(info.cpv, "dev-libs/pkginfopkg-1.0");
    assert_eq!(info.repo_name, "testrepo");
    assert!(info.defines_pkg_info);
    assert_eq!(
        info.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );

    // dev-libs/newpkg has no pkg_info phase.
    let plain = resolve_info_candidate(&repos, "dev-libs/newpkg", &test_config())
        .expect("resolves")
        .expect("has a visible candidate");
    assert!(!plain.defines_pkg_info);

    // A cat/pkg with no ebuild -> None.
    assert!(
        resolve_info_candidate(&repos, "dev-libs/nonexistent", &test_config())
            .expect("resolves")
            .is_none()
    );
}

#[test]
fn resolve_installed_info_reads_the_vdb_use_line_and_the_differing_vars() {
    let root = fixtures_root();
    // dev-libs/infoinstpkg-1.0: vdb IUSE="alpha beta" USE="alpha",
    // CFLAGS/CHOST recorded, test_config's make.conf sets neither.
    let infos = resolve_installed_info(&root, "dev-libs/infoinstpkg", &test_config());
    assert_eq!(infos.len(), 1);
    let i = &infos[0];
    assert_eq!(i.cpv, "dev-libs/infoinstpkg-1.0");
    assert_eq!(i.repo_name, "testrepo");
    assert_eq!(
        i.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );
    assert_eq!(
        i.differing_vars,
        vec![
            ("CHOST".to_string(), "x86_64-pc-linux-gnu".to_string()),
            ("CFLAGS".to_string(), "-O2 -march=native".to_string()),
        ]
    );
    assert_eq!(
        i.unset_vars,
        vec![
            "CXXFLAGS".to_string(),
            "FEATURES".to_string(),
            "LDFLAGS".to_string()
        ]
    );

    // Not installed -> empty.
    assert!(resolve_installed_info(&root, "dev-libs/newpkg", &test_config()).is_empty());
}

#[test]
fn resolve_installed_info_unsets_everything_without_environment_bz2() {
    // With no `environment.bz2` Portage returns {}: all five vars are `Unset:`,
    // even when a same-named individual vdb file exists.
    let root = tmp_vdb(
        "dev-libs",
        "tmpenvpkg-1.0",
        &[("CFLAGS", b"-O2 -stray\n"), ("USE", b""), ("IUSE", b"")],
    );
    let infos = resolve_installed_info(&root, "dev-libs/tmpenvpkg", &test_config());
    assert_eq!(infos.len(), 1);
    assert!(infos[0].differing_vars.is_empty());
    assert_eq!(
        infos[0].unset_vars,
        vec![
            "CHOST".to_string(),
            "CFLAGS".to_string(),
            "CXXFLAGS".to_string(),
            "FEATURES".to_string(),
            "LDFLAGS".to_string()
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_installed_info_wraps_a_globally_forced_flag() {
    // Portage's `pkg_use_display` wraps a forced flag regardless of its state.
    let root = fixtures_root();
    let config = portage_profile::Config {
        use_force: HashSet::from(["globalforceflag".to_string()]),
        use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
            use_force: vec!["globalforceflag".to_string()],
            ..Default::default()
        }],
        ..test_config()
    };
    let infos = resolve_installed_info(&root, "dev-libs/infoforcedpkg", &config);
    assert_eq!(infos.len(), 1);
    assert_eq!(
        infos[0].use_expand_display,
        vec![(
            "USE".to_string(),
            "(globalforceflag) -otherflag".to_string()
        )]
    );
}

#[test]
fn resolve_installed_info_flags_a_vdb_recorded_pkg_info_phase() {
    let root = fixtures_root();
    // No DEFINED_PHASES file at all is falsy, so `pkg_info()` IS attempted.
    let plain = resolve_installed_info(&root, "dev-libs/infoinstpkg", &test_config());
    assert_eq!(plain.len(), 1);
    assert!(plain[0].defines_pkg_info);
    assert_eq!(plain[0].category, "dev-libs");
    assert_eq!(plain[0].package, "infoinstpkg");
    assert_eq!(plain[0].version, "1.0");

    // `DEFINED_PHASES="-"` is truthy but names no `info` phase -> NOT attempted.
    let dash = resolve_installed_info(&root, "dev-libs/infodashphases", &test_config());
    assert_eq!(dash.len(), 1);
    assert!(!dash[0].defines_pkg_info);
}

#[test]
fn resolve_info_binary_candidate_picks_a_pkg_info_defining_local_binpkg() {
    // A binary-only pkg with DEFINED_PHASES=info is a `--info` candidate.
    let config = portage_profile::Config {
        pkgdir: fixtures_root()
            .join("pkgdir")
            .to_string_lossy()
            .into_owned(),
        ..test_config()
    };
    let local_binpkg = build_local_binpkg_index(&config);
    let got = resolve_info_binary_candidate(&config, "dev-libs/binaryinfopkg", &local_binpkg)
        .expect("no error")
        .expect("a candidate");
    assert_eq!(got.cpv, "dev-libs/binaryinfopkg-1.0");
    assert_eq!(got.repo_name, "testrepo");
    assert_eq!(got.pf, "binaryinfopkg-1.0");
    assert_eq!(
        got.use_expand_display,
        vec![("USE".to_string(), "alpha -beta".to_string())]
    );

    // A binary with DEFINED_PHASES="-" is not a pkg_info() candidate.
    assert!(
        resolve_info_binary_candidate(&config, "dev-libs/binaryonlypkg", &local_binpkg)
            .expect("no error")
            .is_none()
    );
    assert!(has_local_binary_candidate(
        &local_binpkg,
        "dev-libs/binaryonlypkg"
    ));
}
