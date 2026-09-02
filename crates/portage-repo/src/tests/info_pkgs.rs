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
