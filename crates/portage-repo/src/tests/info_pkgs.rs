use super::*;

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
