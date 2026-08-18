use super::*;

#[test]
fn use_tokens_capture_the_ordered_raw_use_values_that_produced_use_flags() {
    // `use_tokens` keeps each raw USE contribution separately, in accumulation
    // order (profile chain, then conf tier), so callers can replay them onto
    // a different seed (e.g. a package's IUSE defaults) via apply_incremental.
    let root = TempDir::new("portage-profile-test-use-tokens").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("make.defaults"), "USE=\"foo\"\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "USE=\"-foo bar\"\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();
    fs::write(portage_dir.join("make.conf"), "USE=\"baz\"\n").unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.use_tokens,
        vec!["foo".to_string(), "-foo bar".to_string()]
    );
    assert_eq!(config.conf_use_tokens, vec!["baz".to_string()]);
    let mut replayed = HashSet::new();
    for token in config.use_tokens.iter().chain(&config.conf_use_tokens) {
        apply_use_incremental(token, &mut replayed);
    }
    assert_eq!(replayed, config.use_flags);
}

#[test]
fn apply_incremental_is_reusable_for_per_package_use_overrides() {
    let mut set = HashSet::from(["foo".to_string()]);
    apply_incremental("flag1 -foo +flag2", &mut set);
    assert_eq!(
        set,
        HashSet::from(["flag1".to_string(), "flag2".to_string()])
    );
}
