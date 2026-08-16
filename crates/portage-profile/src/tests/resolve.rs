use super::*;

#[test]
fn resolves_fixture_profile_chain_and_make_conf() {
    let root = fixtures_root();
    // The fixture's default/parent has a cross-repo entry, so this test needs
    // "overlay" in its repo list too -- mirroring what production passes.
    let overlay_repos = [("overlay".to_string(), root.join("overlay"))];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config must resolve");
    assert_eq!(
        config.use_flags,
        HashSet::from([
            "foo".to_string(),
            "baz".to_string(),
            "localflag".to_string(),
            "confflag".to_string(),
            "video_cards_nvidia".to_string(),
            // UNPREFIXED="ARCH": how "amd64" exists as a plain USE flag.
            "amd64".to_string(),
            // ELIBC="glibc": USE_EXPAND folding; also in USE_EXPAND_IMPLICIT.
            "elibc_glibc".to_string(),
            // CPU_FLAGS_X86="sse2": HIDDEN affects only `emerge -pv` display.
            "cpu_flags_x86_sse2".to_string(),
            // LUA_SINGLE_TARGET: the per-level USE_EXPAND-fold fixture.
            "lua_single_target_lua5-1".to_string(),
        ])
    );
    assert_eq!(
        config.use_expand,
        HashSet::from([
            "VIDEO_CARDS".to_string(),
            "ELIBC".to_string(),
            // LINGUAS: in USE_EXPAND for the _* wildcard fixture, valueless.
            "LINGUAS".to_string(),
            // PYTHON_TARGETS + CPU_FLAGS_X86: the USE_EXPAND-grouping fixtures.
            "PYTHON_TARGETS".to_string(),
            "CPU_FLAGS_X86".to_string(),
            // LUA_SINGLE_TARGET: the per-level fold fixture.
            "LUA_SINGLE_TARGET".to_string(),
        ])
    );
    assert_eq!(
        config.use_expand_hidden,
        HashSet::from(["CPU_FLAGS_X86".to_string()])
    );
    assert_eq!(
        config.use_expand_implicit,
        HashSet::from(["ELIBC".to_string()])
    );
    // EAPI 5+ IUSE_EFFECTIVE: ELIBC is in USE_EXPAND ∩ USE_EXPAND_IMPLICIT,
    // so every USE_EXPAND_VALUES_ELIBC value is valid implicit IUSE.
    assert_eq!(
        config.iuse_effective,
        HashSet::from(["elibc_glibc".to_string(), "elibc_musl".to_string()])
    );
    assert_eq!(config.accept_keywords, HashSet::from(["amd64".to_string()]));
    // Neither the fixture chain nor make.conf sets ACCEPT_LICENSE: Portage's
    // "* -@EULA" default applies. `license_groups` comes from each repo's
    // own `profiles/` dir (main first, then overlay), never a chain level.
    assert_eq!(
        config.accept_license,
        vec![
            "*".to_string(),
            "-SomeEula".to_string(),
            "-CrossRepoNonfree".to_string()
        ]
    );
    assert_eq!(
        config.license_groups.get("EULA"),
        Some(&vec![
            "SomeEula".to_string(),
            "CrossRepoNonfree".to_string()
        ])
    );
}
