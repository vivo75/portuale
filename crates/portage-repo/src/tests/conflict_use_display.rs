use super::*;

/// End to end over the fixture tree: a flagless blk0 package renders `USE=""`
/// with the implicit group; a missing version yields no display.
#[test]
fn skipped_update_use_display_for_renders_the_fixture_blk0_shape() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0x", "3"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0b", "1"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    // EAPI 8, no IUSE: only the implicit group survives.
    assert_eq!(
        skipped_update_use_display_for(&repos, &config, "dev-libs", "mgxc", "2"),
        vec![
            ("USE".to_string(), String::new()),
            ("ELIBC".to_string(), "glibc".to_string()),
        ],
    );
    assert!(skipped_update_use_display_for(&repos, &config, "dev-libs", "blk0x", "9").is_empty());
}
