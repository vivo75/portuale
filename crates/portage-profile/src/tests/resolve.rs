use super::*;

/// End-to-end check against fixtures/repo/profiles + make.conf (which sources
/// make.local). Hand-traced USE/keyword accumulation; env.d USE follows `eroot`.
/// Expected values from the committed fixture tree.
#[test]
fn envd_use_tokens_follow_eroot_not_config_root() {
    let root = fixtures_root();
    let overlay_repos = [("overlay".to_string(), root.join("overlay"))];
    let args = |eroot: &Path| {
        resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &root.join("repo"),
            overlay_repos: &overlay_repos,
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot,
        })
        .expect("fixture config must resolve")
    };
    assert_eq!(
        args(&root).envd_use_tokens,
        vec!["envdusetestflag".to_string()]
    );
    let bare = TempDir::new("portage-profile-test-eroot").keep();
    fs::create_dir_all(&bare).unwrap();
    assert_eq!(args(&bare).envd_use_tokens, Vec::<String>::new());
    let _ = fs::remove_dir_all(&bare);
}

/// `Config::reload_env_d` re-reads the eroot-only `profile.env`,
/// replacing the env.d scalar and incremental layers without touching higher
/// layers. Expected behavior from Portage's per-package `config.reload()`.
#[test]
fn reload_env_d_picks_up_a_rewritten_profile_env() {
    let root = TempDir::new("portage-profile-test-envd-reload").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(portage_dir.join("env")).unwrap();
    fs::create_dir_all(root.join("etc")).unwrap();
    // make.conf owns one key (reload must not override it); CONFIG_PROTECT has
    // both an env.d and a make.conf layer to tell replacement from append.
    fs::write(
        portage_dir.join("make.conf"),
        "ENVDRELOAD_KEEP=\"from-make-conf\"\nENVDRELOAD_LATE=\"from-make-conf\"\n\
         CONFIG_PROTECT=\"/conf-protect\"\n",
    )
    .unwrap();
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_KEEP='from-envd'\n\
         export ENVDRELOAD_GONE='bye'\n\
         export CONFIG_PROTECT='/envd-protect'\n\
         export EAPI='9999'\n",
    )
    .unwrap();
    let resolve = || {
        with_test_env(&[], || {
            resolve_config(ResolveConfigArgs {
                config_root: &root,
                main_repo_location: &repo,
                overlay_repos: &[],
                repo_aliases: &[],
                main_repo_name: "testrepo",
                repo_masters: &HashMap::new(),
                eroot: &root,
            })
            .expect("resolves")
        })
    };
    let mut config = resolve();
    // The first load applies the env blacklist too (Portage config.py):
    // `EAPI` (in `ENV_BLACKLIST`) never enters `other_vars`.
    assert_eq!(config.other_vars.get("EAPI"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_KEEP").map(String::as_str),
        Some("from-make-conf")
    );
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_GONE").map(String::as_str),
        Some("bye")
    );
    assert!(!config.env_d_changed(&root));
    assert_eq!(
        config.resolved_incremental("CONFIG_PROTECT"),
        Some(vec![
            "/conf-protect".to_string(),
            "/envd-protect".to_string()
        ])
    );

    // A merge ran `env-update`: a new key appears, the `make.conf`-owned key
    // changes upstream, a key disappears, the blacklisted key stays filtered.
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_KEEP='changed-envd'\n\
         export ENVDRELOAD_VAR='1'\n\
         export ENVDRELOAD_LATE='late-envd'\n\
         export CONFIG_PROTECT='/new-protect'\n\
         export EAPI='9999'\n",
    )
    .unwrap();
    assert!(config.env_d_changed(&root));
    config.reload_env_d(&root);
    assert!(!config.env_d_changed(&root));
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_VAR").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_KEEP").map(String::as_str),
        Some("from-make-conf")
    );
    // A key env.d only starts setting mid-run still loses to the `make.conf`
    // value it lies under (Portage's layered lookup).
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_LATE").map(String::as_str),
        Some("from-make-conf")
    );
    assert_eq!(config.other_vars.get("ENVDRELOAD_GONE"), None);
    assert_eq!(config.other_vars.get("EAPI"), None);
    // The env.d incremental layer is replaced, not appended: the old
    // token is gone while the `make.conf` layer survives.
    assert_eq!(
        config.resolved_incremental("CONFIG_PROTECT"),
        Some(vec![
            "/conf-protect".to_string(),
            "/new-protect".to_string()
        ])
    );
    let _ = fs::remove_dir_all(&root);
}

/// The profile half of the env.d blacklist drops env.d keys the
/// profile's `PROFILE_ONLY_VARIABLES` names, on first load and every reload.
/// Expected behavior from Portage config.py blacklist.
#[test]
fn reload_env_d_never_admits_a_profile_only_variable() {
    let root = TempDir::new("portage-profile-test-envd-profile-only").keep();
    let repo = root.join("repo");
    let profile = root.join("etc/portage/make.profile");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&profile).unwrap();
    fs::write(
        profile.join("make.defaults"),
        "PROFILE_ONLY_VARIABLES=\"ENVDRELOAD_PO\"\n",
    )
    .unwrap();
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_PO='first'\nexport ENVDRELOAD_OK='first'\n",
    )
    .unwrap();
    let mut config = with_test_env(&[], || {
        resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &repo,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves")
    });
    assert_eq!(config.other_vars.get("ENVDRELOAD_PO"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_OK").map(String::as_str),
        Some("first")
    );
    fs::write(
        root.join("etc/profile.env"),
        "export ENVDRELOAD_PO='second'\nexport ENVDRELOAD_OK='second'\n",
    )
    .unwrap();
    config.reload_env_d(&root);
    assert_eq!(config.other_vars.get("ENVDRELOAD_PO"), None);
    assert_eq!(
        config.other_vars.get("ENVDRELOAD_OK").map(String::as_str),
        Some("second")
    );
    let _ = fs::remove_dir_all(&root);
}

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

#[test]
fn license_groups_come_from_each_repo_profiles_dir_not_the_profile_chain() {
    // A `license_groups` file in a profile-chain directory must be IGNORED:
    // Portage reads it only from `<repo>/profiles/license_groups`.
    let root = fixtures_root();
    let stray = root.join("repo/profiles/base/license_groups");
    std::fs::write(&stray, "EULA StrayShouldBeIgnored\n").unwrap();
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    });
    std::fs::remove_file(&stray).ok();
    let config = config.expect("fixture config must resolve");
    // Only the two repo-level members, in repo-then-overlay order.
    assert_eq!(
        config.license_groups.get("EULA"),
        Some(&vec![
            "SomeEula".to_string(),
            "CrossRepoNonfree".to_string()
        ])
    );
}

#[test]
fn missing_profile_and_make_conf_yield_empty_config() {
    let empty_root = TempDir::new("portage-profile-test-empty-root").keep();
    let _ = fs::create_dir_all(&empty_root);
    let config = resolve_config(ResolveConfigArgs {
        config_root: &empty_root,
        main_repo_location: &empty_root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &empty_root,
    })
    .expect("missing profile/make.conf is not an error");
    assert_eq!(config.use_flags, HashSet::new());
    assert_eq!(config.accept_keywords, HashSet::new());
    assert_eq!(
        config.accept_license,
        vec!["*".to_string(), "-@EULA".to_string()]
    );
    assert_eq!(config.accept_properties, vec!["*".to_string()]);
    assert_eq!(config.accept_restrict, vec!["*".to_string()]);
}

#[test]
fn cross_repo_profile_parent_resolves_a_named_repos_own_profile() {
    // The leaf lives outside any repo but its "parent" names "otherrepo:base":
    // repo-name lookup must not depend on the referencing node's membership.
    let root = TempDir::new("portage-profile-test-cross-repo-named").keep();
    let main_repo = root.join("repo");
    let other_repo = root.join("otherrepo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(main_repo.join("profiles")).unwrap();
    fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        other_repo.join("profiles/base/make.defaults"),
        "USE=\"crossrepoflag\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "otherrepo:base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &main_repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("cross-repo parent must resolve");
    assert!(config.use_flags.contains("crossrepoflag"));
}

#[test]
fn cross_repo_profile_parent_resolves_an_aliased_repo_name() {
    // Same, but naming the repo by alias ("ovl" for "otherrepo").
    // Portage `_expand_parent_colon` keys on aliases too.
    let root = TempDir::new("portage-profile-test-cross-repo-alias").keep();
    let main_repo = root.join("repo");
    let other_repo = root.join("otherrepo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(main_repo.join("profiles")).unwrap();
    fs::create_dir_all(other_repo.join("profiles/base")).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        other_repo.join("profiles/base/make.defaults"),
        "USE=\"aliasedflag\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "ovl:base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    // `otherrepo` is a canonical name, `ovl` its alias.
    let overlay_repos = [("otherrepo".to_string(), other_repo.clone())];
    let repo_aliases = [("ovl".to_string(), other_repo.clone())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &main_repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("aliased cross-repo parent must resolve");
    assert!(config.use_flags.contains("aliasedflag"));

    // ...and without the alias registered, the same clear error as unknown names.
    let err = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &main_repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .unwrap_err();
    assert!(err.to_string().contains("no repo named \"ovl\""), "{err}");
}

#[test]
fn cross_repo_profile_parent_unknown_repo_name_is_a_clear_error() {
    let root = TempDir::new("portage-profile-test-cross-repo-unknown").keep();
    let profile_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("parent"), "doesnotexist:base\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let err = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect_err("unknown repo name must be rejected");
    assert!(
        err.to_string().contains("no repo named"),
        "unexpected error: {err}"
    );
}

#[test]
fn same_repo_colon_profile_parent_resolves_within_the_current_repo() {
    // A bare ":base" means "this same repo", via the referencing node's own
    // repo membership (longest-prefix match); the leaf lives in the main repo.
    // Portage `_allow_parent_colon` gate: needs `portage-2` in layout.conf.
    let root = TempDir::new("portage-profile-test-same-repo-colon").keep();
    let main_repo = root.join("repo");
    fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
    fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
    fs::write(
        main_repo.join("profiles/base/make.defaults"),
        "USE=\"samerepocolon\"\n",
    )
    .unwrap();
    fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();
    // Portage `_allow_parent_colon` gate: a profile node inside a repo
    // needs that repo's `layout.conf` to declare `portage-2`.
    fs::create_dir_all(main_repo.join("metadata")).unwrap();
    fs::write(
        main_repo.join("metadata/layout.conf"),
        "profile-formats = portage-2\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &main_repo,
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("same-repo colon parent must resolve");
    assert!(config.use_flags.contains("samerepocolon"));
}

#[test]
fn colon_profile_parent_is_not_expanded_when_the_repo_lacks_portage_2() {
    // Same setup but with NO `metadata/layout.conf`: the `:base` line stays
    // literal and resolution fails on `<leaf>/:base` -- proving the gate gates.
    let root = TempDir::new("portage-profile-test-colon-no-portage2").keep();
    let main_repo = root.join("repo");
    fs::create_dir_all(main_repo.join("profiles/leaf")).unwrap();
    fs::create_dir_all(main_repo.join("profiles/base")).unwrap();
    fs::write(main_repo.join("profiles/base/make.defaults"), "USE=\"x\"\n").unwrap();
    fs::write(main_repo.join("profiles/leaf/parent"), ":base\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(main_repo.join("profiles/leaf"), &make_profile).unwrap();

    let err = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &main_repo,
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect_err("a colon parent in a non-portage-2 repo must not resolve");
    assert!(err.to_string().contains(":base"), "unexpected error: {err}");
}

#[test]
fn same_repo_colon_profile_parent_outside_any_known_repo_is_a_clear_error() {
    let root = TempDir::new("portage-profile-test-same-repo-colon-outside").keep();
    let profile_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("parent"), ":base\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

    let err = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect_err("same-repo colon outside any known repo must be rejected");
    assert!(
        err.to_string().contains("not inside any known repo"),
        "unexpected error: {err}"
    );
}

#[test]
fn diamond_profile_inheritance_does_not_double_apply_a_shared_ancestor() {
    // Diamond: "shared" must contribute its USE flag once, and the
    // visited-set mechanism must not block a legitimate multi-path DAG.
    let root = TempDir::new("portage-profile-test-diamond").keep();
    let profile_dir = root.join("etc/portage");
    fs::create_dir_all(&profile_dir).unwrap();
    for name in ["shared", "left", "right", "top"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    fs::write(root.join("shared/make.defaults"), "USE=\"sharedflag\"\n").unwrap();
    fs::write(root.join("left/parent"), "../shared\n").unwrap();
    fs::write(root.join("right/parent"), "../shared\n").unwrap();
    fs::write(root.join("top/parent"), "../left\n../right\n").unwrap();
    fs::write(root.join("top/make.defaults"), "USE=\"${USE} topflag\"\n").unwrap();
    let make_profile = profile_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("top"), &make_profile).unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("diamond inheritance must resolve");
    assert_eq!(
        config.use_flags,
        HashSet::from(["sharedflag".to_string(), "topflag".to_string()])
    );
}

#[test]
fn package_mask_unmask_accept_keywords_load_correctly() {
    let root = TempDir::new("portage-profile-test-package-star").keep();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    // package.mask: two atoms added, then one removed via "-atom" --
    // only the surviving one should remain.
    fs::write(
        portage_dir.join("package.mask"),
        "dev-libs/foo\ndev-libs/bar\n-dev-libs/bar\n",
    )
    .unwrap();
    // package.unmask is a separate list, checked per-candidate by the caller.
    fs::write(portage_dir.join("package.unmask"), "dev-libs/baz\n").unwrap();
    // A normal entry, a "**" entry, and a bare-atom line. No global
    // ACCEPT_KEYWORDS here, so the bare atom keeps an empty token list
    // (see the accept_keywords_defaults tests for the substitution).
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-qt/* ~amd64\nsci-misc/live-thing **\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config with package.* files must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/foo".to_string()]);
    assert_eq!(config.package_unmask, vec!["dev-libs/baz".to_string()]);
    assert_eq!(
        config.package_accept_keywords,
        vec![
            ("dev-qt/*".to_string(), vec!["~amd64".to_string()]),
            ("sci-misc/live-thing".to_string(), vec!["**".to_string()]),
            ("dev-libs/bare-no-op".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn package_mask_atom_removal_applies_across_repo_profile_and_user_sources() {
    // Repo-level masks get `::testrepo`-scoped like an overlay's; the profile
    // "-a" still cancels the scoped entry (see `stack_mask_lines`), and the
    // user "-c" cancels the profile entry: removal spans all three sources.
    let root = TempDir::new("portage-profile-test-cross-source-mask").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        repo_profiles.join("package.mask"),
        "dev-libs/a\ndev-libs/b\n",
    )
    .unwrap();
    fs::write(leaf.join("package.mask"), "-dev-libs/a\ndev-libs/c\n").unwrap();
    fs::write(
        portage_dir.join("package.mask"),
        "-dev-libs/c\ndev-libs/d\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.package_mask,
        vec!["dev-libs/b::testrepo".to_string(), "dev-libs/d".to_string()]
    );
}

#[test]
fn overlay_package_mask_is_scoped_to_its_own_repo_only() {
    // The overlay's bare atom must auto-scope to "::overlay", never masking
    // a same-named main-repo package.
    let root = TempDir::new("portage-profile-test-overlay-mask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
}

#[test]
fn overlay_package_unmask_gets_the_same_scoping_as_overlay_package_mask() {
    // Both entries get the identical "::overlay" scoping; the mask/unmask
    // cancellation itself happens downstream in portage-repo's is_visible.
    let root = TempDir::new("portage-profile-test-overlay-mask-unmask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(config.package_mask, vec!["dev-libs/a::overlay".to_string()]);
    assert_eq!(
        config.package_unmask,
        vec!["dev-libs/a::overlay".to_string()]
    );
}

#[test]
fn overlay_package_mask_inherits_the_main_repos_own_entries_via_implicit_masters() {
    // An overlay with no explicit "masters =" implicitly masters the main repo,
    // so main's mask entry lands both "::testrepo"- and "::overlay"-scoped.
    let root = TempDir::new("portage-profile-test-overlay-masters-mask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.package_mask,
        vec![
            "dev-libs/a::testrepo".to_string(),
            "dev-libs/a::overlay".to_string(),
            "dev-libs/b::overlay".to_string(),
        ]
    );
}

#[test]
fn overlay_package_unmask_does_not_inherit_via_masters() {
    // Portage's package.unmask loop never consults masters: main's entry stays
    // "::testrepo"-scoped only, never also "::overlay"-scoped.
    let root = TempDir::new("portage-profile-test-overlay-masters-unmask").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.unmask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.package_unmask,
        vec!["dev-libs/a::testrepo".to_string()]
    );
}

#[test]
fn explicit_repo_masters_overrides_the_implicit_main_repo_default() {
    // An explicit (here empty) "masters =" replaces the implicit main-repo
    // default: no package.mask inheritance at all, even from the main repo.
    let root = TempDir::new("portage-profile-test-explicit-masters-empty").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let repo_masters = HashMap::from([("overlay".to_string(), Vec::<PathBuf>::new())]);
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &repo_masters,
        eroot: &root,
    })
    .expect("config must resolve");
    // Main's entry still masks its own repo (independent of masters) but is
    // not inherited by the explicitly-empty-masters overlay.
    assert_eq!(
        config.package_mask,
        vec!["dev-libs/a::testrepo".to_string()]
    );
}

#[test]
fn explicit_repo_masters_resolves_a_non_main_master_chain() {
    // A repo can declare a non-main master: its mask stacks in like the
    // implicit main-repo case. Main's own entry must NOT apply downstream.
    let root = TempDir::new("portage-profile-test-explicit-masters-chain").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    let downstream = root.join("downstream");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();
    fs::create_dir_all(downstream.join("profiles")).unwrap();

    // Main's entry must NOT apply (not a declared master); the overlay's must.
    fs::write(repo.join("profiles/package.mask"), "dev-libs/a\n").unwrap();
    fs::write(overlay.join("profiles/package.mask"), "dev-libs/b\n").unwrap();

    let overlay_repos = [
        ("overlay".to_string(), overlay.to_path_buf()),
        ("downstream".to_string(), downstream.clone()),
    ];
    let repo_masters = HashMap::from([("downstream".to_string(), vec![overlay.to_path_buf()])]);
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &repo_masters,
        eroot: &root,
    })
    .expect("config must resolve");
    assert!(
        config
            .package_mask
            .contains(&"dev-libs/b::downstream".to_string())
    );
    assert!(
        !config
            .package_mask
            .contains(&"dev-libs/a::downstream".to_string())
    );
}

#[test]
fn overlay_package_use_is_scoped_to_its_own_repo_only() {
    // The overlay's bare atom auto-scopes to "::overlay", like package.mask.
    let root = TempDir::new("portage-profile-test-overlay-package-use").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();

    fs::write(overlay.join("profiles/package.use"), "dev-libs/a flag\n").unwrap();

    let overlay_repos = [("overlay".to_string(), overlay.to_path_buf())];
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &overlay_repos,
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.package_use_repo,
        vec![("dev-libs/a::overlay".to_string(), vec!["flag".to_string()])]
    );
}

#[test]
fn system_packages_stack_across_profile_levels_with_atom_removal_and_star_filter() {
    // Base adds "*dev-libs/a" plus a bare "hint" line (never an atom alone);
    // the leaf's "-*dev-libs/a" spans levels. Final @system: just "dev-libs/b".
    let root = TempDir::new("portage-profile-test-system-packages").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("packages"), "*dev-libs/a\ndev-libs/hint\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("packages"), "-*dev-libs/a\n*dev-libs/b\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    assert_eq!(config.system_packages, vec!["dev-libs/b".to_string()]);
}

#[test]
fn profile_packages_are_the_unstarred_lines_of_profile_set_repo_levels() {
    // Both levels live in a `profile-set` repo: unstarred lines stack with
    // cross-level removal into @profile; starred lines go to @system only.
    let root = TempDir::new("portage-profile-test-profile-packages").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(repo.join("metadata")).unwrap();
    fs::write(
        repo.join("metadata/layout.conf"),
        "profile-formats = portage-2 profile-set\n",
    )
    .unwrap();
    fs::write(
        base.join("packages"),
        "*dev-libs/sysbase\ndev-libs/baseonly\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(
        leaf.join("packages"),
        "-dev-libs/baseonly\ndev-libs/leafonly\n*dev-libs/sysleaf\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.profile_packages,
        vec!["dev-libs/leafonly".to_string()]
    );
    assert_eq!(
        config.system_packages,
        vec![
            "dev-libs/sysbase".to_string(),
            "dev-libs/sysleaf".to_string()
        ]
    );
}

#[test]
fn profile_packages_require_the_profile_set_format() {
    // Without `profile-set` nothing is an @profile source (Portage
    // ProfilePackageSet.load's gate); @system is unaffected.
    let root = TempDir::new("portage-profile-test-no-profile-set").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(repo.join("metadata")).unwrap();
    fs::write(
        repo.join("metadata/layout.conf"),
        "profile-formats = portage-2\n",
    )
    .unwrap();
    fs::write(
        base.join("packages"),
        "*dev-libs/sysbase\ndev-libs/hintonly\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(leaf.join("packages"), "*dev-libs/sysleaf\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    assert!(config.profile_packages.is_empty());
    assert_eq!(
        config.system_packages,
        vec![
            "dev-libs/sysbase".to_string(),
            "dev-libs/sysleaf".to_string()
        ]
    );
}

#[test]
fn package_provided_stacks_the_profile_chain_plus_the_user_file_with_atom_removal() {
    // Base provides a and b; the leaf removes b and adds c; the user file
    // removes a. Final: just dev-libs/c-1.0.
    let root = TempDir::new("portage-profile-test-package-provided").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("package.provided"),
        "dev-libs/a-1.0\ndev-libs/b-1.0\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("package.provided"),
        "-dev-libs/b-1.0\ndev-libs/c-1.0\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(portage_dir.join("profile")).unwrap();
    fs::write(
        portage_dir.join("profile/package.provided"),
        "-dev-libs/a-1.0\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    assert_eq!(config.package_provided, vec!["dev-libs/c-1.0".to_string()]);
}

#[test]
fn use_mask_and_use_force_stack_across_levels_but_stay_out_of_use_flags() {
    // Mask wins when a flag is both forced AND masked (Portage's
    // update-then-difference_update order). Force/mask stack across levels
    // but deliberately stay out of `use_flags` (applied per-candidate later).
    let root = TempDir::new("portage-profile-test-use-mask-force").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("make.defaults"), "USE=\"normalflag maskflag\"\n").unwrap();
    fs::write(base.join("use.force"), "forceflag\nbothflag\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("use.mask"), "maskflag\nbothflag\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.use_flags,
        HashSet::from(["normalflag".to_string(), "maskflag".to_string()])
    );
    assert_eq!(
        config.use_force,
        HashSet::from(["forceflag".to_string(), "bothflag".to_string()])
    );
    assert_eq!(
        config.use_mask,
        HashSet::from(["maskflag".to_string(), "bothflag".to_string()])
    );
}

#[test]
fn archlist_stacks_across_profile_levels_with_removal_semantics() {
    // Same chain-order `-entry`-removal stacking use.mask/use.force get
    // (Portage config.py stack_lists(archlist, incremental=1)).
    let root = TempDir::new("portage-profile-test-archlist").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(base.join("arch.list"), "amd64\nx86\n").unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("arch.list"), "-x86\narm64\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.archlist,
        HashSet::from(["amd64".to_string(), "arm64".to_string()])
    );
}

#[test]
fn use_expand_variable_names_stack_incrementally_across_profile_levels() {
    // USE_EXPAND names add incrementally (not replace) across levels, and each
    // variable's value expands regardless of which level declared the name.
    let root = TempDir::new("portage-profile-test-use-expand-names-stack").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("make.defaults"),
        "USE_EXPAND=\"PYTHON_TARGETS\"\nPYTHON_TARGETS=\"python3_11\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.use_expand,
        HashSet::from(["VIDEO_CARDS".to_string(), "PYTHON_TARGETS".to_string()])
    );
    assert!(config.use_flags.contains("video_cards_nvidia"));
    assert!(config.use_flags.contains("python_targets_python3_11"));
}

#[test]
fn use_expand_variable_value_expands_with_negation_and_plus_stripped() {
    // One value exercising all three token forms: "nvidia" adds, "+intel" adds
    // ("+" stripped), "-nvidia" removes. Final: only video_cards_intel.
    let root = TempDir::new("portage-profile-test-use-expand-negation").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia +intel -nvidia\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

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
    assert!(!config.use_flags.contains("video_cards_nvidia"));
    assert!(config.use_flags.contains("video_cards_intel"));
    assert!(!config.use_flags.contains("+video_cards_intel"));
}

#[test]
fn profile_make_defaults_use_expand_is_folded_per_level_before_package_use() {
    // A profile USE_EXPAND value folds into that level's make_defaults_use
    // (prepended, so package.use can cancel it), never into the conf tier.
    // Portage config.py per-level fold; the neovim REQUIRED_USE case in miniature.
    let root = TempDir::new("portage-profile-test-use-expand-per-level-fold").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    fs::create_dir_all(&base).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"LUA_SINGLE_TARGET\"\nLUA_SINGLE_TARGET=\"lua5-1\"\nUSE=\"rawflag\"\n",
    )
    .unwrap();
    fs::write(
        base.join("package.use"),
        "dev-libs/pkg lua_single_target_luajit -lua_single_target_lua5-1\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

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
    let layer = &config.profile_use_layers[0];
    // folded flag lands in this level's make_defaults_use, prepended.
    assert_eq!(
        layer.make_defaults_use.first().map(String::as_str),
        Some("lua_single_target_lua5-1")
    );
    assert!(layer.make_defaults_use.iter().any(|t| t == "rawflag"));
    // ...and NOT in the conf tier, where it would out-rank package.use.
    assert!(
        !config
            .conf_use_tokens
            .iter()
            .any(|t| t.contains("lua_single_target")),
        "conf_use_tokens = {:?}",
        config.conf_use_tokens
    );
    // the same level's package.use override is still present.
    assert!(
        layer
            .package_use
            .iter()
            .any(|(atom, toks)| atom == "dev-libs/pkg"
                && toks.iter().any(|t| t == "lua_single_target_luajit"))
    );
}

#[test]
fn use_expand_value_set_at_two_profile_levels_stacks_incrementally() {
    // Each level's value folds into that level's USE (USE is incremental), so
    // the later level never wipes the earlier; a `-video_cards_*` still cancels.
    // Portage config.py per-level fold; mirrors Gentoo's default/linux + arch/amd64.
    let root = TempDir::new("portage-profile-test-use-expand-two-level-stack").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"dummy fbdev radeon\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(
        leaf.join("make.defaults"),
        "VIDEO_CARDS=\"amdgpu fbdev vesa -radeon\"\n",
    )
    .unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    // Base-only values survive the leaf's assignment; the later `-` cancels.
    assert!(config.use_flags.contains("video_cards_dummy"));
    // Both levels' additive values are present; the later `-radeon` cancels.
    assert!(config.use_flags.contains("video_cards_amdgpu"));
    assert!(config.use_flags.contains("video_cards_fbdev"));
    assert!(config.use_flags.contains("video_cards_vesa"));
    // A `-` in the later level still cancels the earlier value.
    assert!(!config.use_flags.contains("video_cards_radeon"));
}

#[test]
fn use_expand_prefix_wildcard_cancels_parent_level_values() {
    // Regression pin: the leaf's `VIDEO_CARDS="-* intel"` cancels the
    // parents' `video_cards_*` flags keeping `intel`.
    let root = TempDir::new("portage-profile-test-use-expand-prefix-wildcard").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"dummy fbdev intel\"\n",
    )
    .unwrap();
    fs::write(leaf.join("parent"), "../repo/profiles/base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "VIDEO_CARDS=\"-* intel\"\n").unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    assert!(config.use_flags.contains("video_cards_intel"));
    assert!(
        !config.use_flags.contains("video_cards_dummy"),
        "the leaf `-*` must cancel the parent's dummy: {:?}",
        config.use_flags
    );
    assert!(!config.use_flags.contains("video_cards_fbdev"));
}

#[test]
fn use_expand_unprefixed_variable_contributes_a_bare_flag_no_prefix() {
    // ARCH="amd64" contributes the bare flag, not "arch_amd64" -- the defining
    // difference from a prefixed USE_EXPAND variable.
    let root = TempDir::new("portage-profile-test-use-expand-unprefixed").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"amd64\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

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
        config.use_expand_unprefixed,
        HashSet::from(["ARCH".to_string()])
    );
    assert!(config.use_flags.contains("amd64"));
    assert!(!config.use_flags.contains("arch_amd64"));
}

#[test]
fn use_expand_unprefixed_variable_value_expands_with_negation_and_plus_stripped() {
    // Same three token forms as the prefixed case, unprefixed: "foo" adds,
    // "+bar" adds, "-foo" removes. Final: only "bar".
    let root = TempDir::new("portage-profile-test-use-expand-unprefixed-negation").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let base = repo_profiles.join("base");
    fs::create_dir_all(&base).unwrap();

    fs::write(
        base.join("make.defaults"),
        "USE_EXPAND_UNPREFIXED=\"ARCH\"\nARCH=\"foo +bar -foo\"\n",
    )
    .unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&base, &make_profile).unwrap();

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
    assert!(!config.use_flags.contains("foo"));
    assert!(config.use_flags.contains("bar"));
    assert!(!config.use_flags.contains("+bar"));
}

#[test]
fn package_accept_keywords_stacks_profile_chain_then_user_no_repo_source() {
    // Profile-chain first (Portage KeywordsManager.getPKeywords); no repo-level
    // source exists, so the repo file is deliberately never created here.
    let root = TempDir::new("portage-profile-test-accept-keywords-stack").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(leaf.join("package.accept_keywords"), "dev-libs/a ~amd64\n").unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/b ~amd64\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.package_accept_keywords,
        vec![
            ("dev-libs/a".to_string(), vec!["~amd64".to_string()]),
            ("dev-libs/b".to_string(), vec!["~amd64".to_string()]),
            // No global ACCEPT_KEYWORDS here, so the bare atom keeps an empty list.
            ("dev-libs/bare-no-op".to_string(), Vec::new()),
        ]
    );
}

#[test]
fn accept_keywords_defaults_substitutes_tilde_arch_for_a_bare_atom() {
    // A bare atom means "~" + every plain global ACCEPT_KEYWORDS token: here
    // just "amd64", so the entry gets an implicit "~amd64".
    let root = TempDir::new("portage-profile-test-accept-keywords-defaults").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&leaf).unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(leaf.join("make.defaults"), "ACCEPT_KEYWORDS=\"amd64\"\n").unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/foo\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.package_accept_keywords,
        vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
    );
}

#[test]
fn accept_keywords_defaults_excludes_already_prefixed_tokens() {
    // Only *plain* global tokens derive (keyword[:1] not in "~-"): "~x86" and
    // "-arm" are excluded entirely, not doubly-prefixed or left bare.
    let root = TempDir::new("portage-profile-test-accept-keywords-defaults-filter").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&leaf).unwrap();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        leaf.join("make.defaults"),
        "ACCEPT_KEYWORDS=\"amd64 ~x86 -arm\"\n",
    )
    .unwrap();
    fs::write(
        portage_dir.join("package.accept_keywords"),
        "dev-libs/foo\n",
    )
    .unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.package_accept_keywords,
        vec![("dev-libs/foo".to_string(), vec!["~amd64".to_string()])]
    );
}

#[test]
fn package_use_splits_repo_profile_and_user_into_their_own_fields() {
    // Each source lands in its own `Config` field at its own USE_ORDER position
    // (the "Config depth" slice); package.use is purely additive, no removal.
    let root = TempDir::new("portage-profile-test-package-use-stack").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(repo_profiles.join("package.use"), "dev-libs/a flaga\n").unwrap();
    fs::write(leaf.join("package.use"), "dev-libs/b flagb\n").unwrap();
    fs::write(portage_dir.join("package.use"), "dev-libs/c flagc\n").unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.package_use_repo,
        vec![("dev-libs/a".to_string(), vec!["flaga".to_string()])]
    );
    assert_eq!(
        config.package_use,
        vec![("dev-libs/b".to_string(), vec!["flagb".to_string()])]
    );
    assert_eq!(
        config.package_use_user,
        vec![("dev-libs/c".to_string(), vec!["flagc".to_string()])]
    );
}

#[test]
fn env_layer_overrides_config_vars() {
    // Exercises the env layer's incremental (ACCEPT_KEYWORDS, USE), last-wins
    // (USE_EXPAND value + plain scalar) and USE_EXPAND-name paths.
    let root = TempDir::new("portage-profile-test-env-layer").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n\
         USE_EXPAND_UNPREFIXED=\"ARCH\"\nUSE=\"baseflag\"\n\
         USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();

    // No env: baseline.
    with_test_env(&[], || {
        let c = resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &repo,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves");
        assert!(c.accept_keywords.contains("amd64"));
        assert!(!c.accept_keywords.contains("~amd64"));
        assert!(c.use_flags.contains("video_cards_nvidia"));
        assert!(!c.other_vars.contains_key("CFLAGS"));
    });

    with_test_env(
        &[
            ("ACCEPT_KEYWORDS", "~amd64"),
            ("USE", "-baseflag envflag"),
            ("VIDEO_CARDS", "amdgpu"),
            ("CFLAGS", "-O3"),
        ],
        || {
            let c = resolve_config(ResolveConfigArgs {
                config_root: &root,
                main_repo_location: &repo,
                overlay_repos: &[],
                repo_aliases: &[],
                main_repo_name: "testrepo",
                repo_masters: &HashMap::new(),
                eroot: &root,
            })
            .expect("resolves");
            // ACCEPT_KEYWORDS: incremental -- profile amd64 kept, env ~amd64 added.
            assert!(c.accept_keywords.contains("amd64"));
            assert!(c.accept_keywords.contains("~amd64"));
            // USE: env stacks on the conf layer.
            assert!(c.use_flags.contains("envflag"));
            assert!(!c.use_flags.contains("baseflag"));
            // USE_EXPAND value: env replaces the profile value.
            assert!(c.use_flags.contains("video_cards_amdgpu"));
            assert!(!c.use_flags.contains("video_cards_nvidia"));
            // Plain scalar -> other_vars (emerge --info).
            assert_eq!(c.other_vars.get("CFLAGS").map(String::as_str), Some("-O3"));
        },
    );
}

#[test]
fn env_layer_stacks_onto_the_config_protect_and_env_unset_folds() {
    // CONFIG_PROTECT/_MASK and ENV_UNSET are in Portage
    // const.INCREMENTALS, so the calling env stacks as the final source.
    let root = TempDir::new("portage-profile-test-info-incrementals").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    fs::write(
        portage_dir.join("make.conf"),
        "CONFIG_PROTECT=\"/conf-protect\"\n\
         CONFIG_PROTECT_MASK=\"/conf-mask\"\n\
         ENV_UNSET=\"CONF_UNSET\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();
    let resolve = || {
        resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &repo,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves")
    };

    // No env: the `make.conf` layer alone.
    with_test_env(&[], || {
        let c = resolve();
        assert_eq!(
            c.resolved_incremental("CONFIG_PROTECT"),
            Some(vec!["/conf-protect".to_string()])
        );
        assert_eq!(
            c.resolved_incremental("ENV_UNSET"),
            Some(vec!["CONF_UNSET".to_string()])
        );
    });

    // Calling env stacks onto the file fold (union, sorted, with `-tok` removal).
    with_test_env(
        &[
            ("CONFIG_PROTECT", "/env-protect"),
            ("CONFIG_PROTECT_MASK", "-/conf-mask /env-mask"),
            ("ENV_UNSET", "ENV_UNSET_X"),
        ],
        || {
            let c = resolve();
            assert_eq!(
                c.resolved_incremental("CONFIG_PROTECT"),
                Some(vec![
                    "/conf-protect".to_string(),
                    "/env-protect".to_string()
                ])
            );
            assert_eq!(
                c.resolved_incremental("CONFIG_PROTECT_MASK"),
                Some(vec!["/env-mask".to_string()])
            );
            assert_eq!(
                c.resolved_incremental("ENV_UNSET"),
                Some(vec!["CONF_UNSET".to_string(), "ENV_UNSET_X".to_string()])
            );
        },
    );
}

#[test]
fn user_package_use_star_atom_folds_onto_the_global_use() {
    // A user "*/*" entry pops out of the per-package dict and stacks onto the
    // global USE like make.conf USE= would (shorthand included).
    // Portage: UseManager.extract_global_USE_changes
    let root = TempDir::new("portage-profile-test-global-puse").keep();
    let repo = root.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage/package.use");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"\n\
         USE_EXPAND=\"VIDEO_CARDS\"\nVIDEO_CARDS=\"nvidia\"\n\
         USE=\"profileflag\"\n",
    )
    .unwrap();
    let make_profile = root.join("etc/portage/make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();
    fs::write(
        portage_dir.join("00-global"),
        "*/* globalflag -profileflag\n*/* VIDEO_CARDS: -nvidia nouveau\ndev-libs/x perpkgflag\n",
    )
    .unwrap();

    with_test_env(&[], || {
        let c = resolve_config(ResolveConfigArgs {
            config_root: &root,
            main_repo_location: &repo,
            overlay_repos: &[],
            repo_aliases: &[],
            main_repo_name: "testrepo",
            repo_masters: &HashMap::new(),
            eroot: &root,
        })
        .expect("resolves");
        assert!(c.use_flags.contains("globalflag"));
        assert!(!c.use_flags.contains("profileflag"));
        assert!(c.use_flags.contains("video_cards_nouveau"));
        assert!(!c.use_flags.contains("video_cards_nvidia"));
        // The "*/*" entry is popped; only the per-package one stays.
        assert!(!c.package_use_user.iter().any(|(a, _)| a == "*/*"));
        assert!(c.package_use_user.iter().any(|(a, _)| a == "dev-libs/x"));
    });
}

#[test]
fn profile_use_layers_are_built_one_per_chain_level() {
    let root = TempDir::new("portage-profile-test-profile-use-layers").keep();
    let repo = root.join("repo");
    let base = repo.join("profiles/base");
    let leaf = repo.join("profiles/leaf");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(base.join("make.defaults"), "USE=\"a b\"\n").unwrap();
    fs::write(base.join("package.use"), "dev-libs/x flag\n").unwrap();
    fs::write(leaf.join("parent"), "../base\n").unwrap();
    fs::write(leaf.join("make.defaults"), "USE=\"-b c\"\n").unwrap();
    // leaf has no package.use.
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.profile_use_layers,
        vec![
            ProfileUseLayer {
                make_defaults_use: vec!["a b".to_string()],
                package_use: vec![("dev-libs/x".to_string(), vec!["flag".to_string()])],
            },
            ProfileUseLayer {
                make_defaults_use: vec!["-b c".to_string()],
                package_use: vec![],
            },
        ]
    );
    // The flat `use_tokens` still carries every level's USE= in order.
    assert_eq!(
        config.use_tokens,
        vec!["a b".to_string(), "-b c".to_string()]
    );
}

#[test]
fn repo_make_defaults_use_is_read_per_repo_with_var_expansion() {
    let root = TempDir::new("portage-profile-test-repo-make-defaults").keep();
    let repo = root.join("repo");
    let overlay = root.join("overlay");
    let prof = repo.join("profiles/default");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&prof).unwrap();
    fs::create_dir_all(overlay.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\nUSE_EXPAND_UNPREFIXED=\"ARCH\"\n",
    )
    .unwrap();
    // The main repo's top-level profiles/make.defaults.
    fs::write(
        repo.join("profiles/make.defaults"),
        "USE=\"repoflag\"\nUSE=\"more arch_${ARCH}\"\n",
    )
    .unwrap();
    // An overlay's own -- keyed by its repo name.
    fs::write(
        overlay.join("profiles/make.defaults"),
        "USE=\"overlayflag\"\n",
    )
    .unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, &make_profile).unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &repo,
        overlay_repos: &[("myoverlay".to_string(), overlay.to_path_buf())],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config must resolve");
    assert_eq!(
        config.repo_make_defaults_use,
        vec![
            (
                String::new(),
                vec!["repoflag".to_string(), "more arch_amd64".to_string()]
            ),
            ("myoverlay".to_string(), vec!["overlayflag".to_string()]),
        ]
    );
}

#[test]
fn envd_use_tokens_are_read_from_profile_env() {
    let root = TempDir::new("portage-profile-test-envd").keep();
    let repo = root.join("repo");
    let etc = root.join("etc");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&etc).unwrap();
    fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();

    // Portage `env-update` output shape: `export` lines, a plain assignment,
    // and unrelated keys that must be ignored.
    fs::write(
        etc.join("profile.env"),
        "# generated\n\
         export PATH='/usr/bin'\n\
         export USE='envdflag -other'\n\
         LDPATH=/usr/lib\n",
    )
    .unwrap();

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
    assert_eq!(config.envd_use_tokens, vec!["envdflag -other".to_string()]);

    // No profile.env -> empty.
    fs::remove_file(etc.join("profile.env")).unwrap();
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
    assert!(config.envd_use_tokens.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_env_resolves_env_file_use_including_source_and_missing_files() {
    let root = TempDir::new("portage-profile-test-package-env").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let env_dir = portage_dir.join("env");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&env_dir).unwrap();

    fs::write(
        portage_dir.join("package.env"),
        "dev-libs/a debug shared\n\
         dev-libs/b gone\n\
         dev-libs/c\n",
    )
    .unwrap();
    // `debug` sources `common` (relative); `gone` is missing and contributes
    // nothing; dev-libs/c (no file names) is dropped entirely.
    fs::write(
        env_dir.join("debug"),
        "source common\nUSE=\"dbgflag\"\nCFLAGS=\"-O0 -g\"\n",
    )
    .unwrap();
    fs::write(env_dir.join("common"), "USE=\"commonflag\"\n").unwrap();
    fs::write(env_dir.join("shared"), "USE=\"sharedflag -commonflag\"\n").unwrap();

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
        config.package_env,
        vec![
            (
                "dev-libs/a".to_string(),
                vec!["debug".to_string(), "shared".to_string()]
            ),
            ("dev-libs/b".to_string(), vec!["gone".to_string()]),
        ]
    );
    // dev-libs/c (no file names) is dropped.
    assert_eq!(
        config.package_env_use,
        vec![(
            "dev-libs/a".to_string(),
            // Order and negation preserved for `apply_incremental`.
            vec![
                "commonflag".to_string(),
                "dbgflag".to_string(),
                "sharedflag".to_string(),
                "-commonflag".to_string(),
            ]
        )]
    );
    // The non-`USE` scalar half: `debug`'s CFLAGS only (`shared` sets
    // no scalar, `common` none, `gone`/`c` contribute nothing).
    assert_eq!(
        config.package_env_vars,
        vec![(
            "dev-libs/a".to_string(),
            vec![("CFLAGS".to_string(), "-O0 -g".to_string())]
        )]
    );
}

#[test]
fn package_env_expands_dollar_vars_against_the_global_map_with_chaining() {
    // Portage `_grab_pkg_env`'s per-package expand map: make.conf values plus
    // same-file and earlier-file chaining. The scalar half expands the same way.
    let root = TempDir::new("portage-profile-test-package-env-expand").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    let env_dir = portage_dir.join("env");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&env_dir).unwrap();
    fs::write(portage_dir.join("make.conf"), "SHARED=\"sharedflag\"\n").unwrap();
    fs::write(portage_dir.join("package.env"), "dev-libs/a first second\n").unwrap();
    fs::write(
        env_dir.join("first"),
        "INNER=\"innerflag\"\nUSE=\"${SHARED} ${INNER}\"\n",
    )
    .unwrap();
    fs::write(
        env_dir.join("second"),
        "USE=\"across-${INNER}\"\nCFLAGS=\"-O2 ${SHARED}\"\n",
    )
    .unwrap();

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
        config.package_env_use,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                "sharedflag".to_string(),
                "innerflag".to_string(),
                "across-innerflag".to_string(),
            ]
        )]
    );
    assert_eq!(
        config.package_env_vars,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                ("INNER".to_string(), "innerflag".to_string()),
                ("CFLAGS".to_string(), "-O2 sharedflag".to_string()),
            ]
        )]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_use_expand_shorthand_applies_prefix_to_following_tokens_on_the_same_line() {
    // "nvidia"/"-intel" get the video_cards_ prefix (negation outside it);
    // "plainflag" (before the marker) does not.
    let root = TempDir::new("portage-profile-test-package-use-expand-shorthand").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/a plainflag VIDEO_CARDS: nvidia -intel\n",
    )
    .unwrap();

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
        config.package_use_user,
        vec![(
            "dev-libs/a".to_string(),
            vec![
                "plainflag".to_string(),
                "video_cards_nvidia".to_string(),
                "-video_cards_intel".to_string(),
            ]
        )]
    );
}

#[test]
fn package_use_expand_shorthand_resets_at_the_start_of_each_line() {
    // The first line's shorthand prefix must not leak into the second line.
    let root = TempDir::new("portage-profile-test-package-use-expand-reset").keep();
    let repo = root.join("repo");
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();

    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/a VIDEO_CARDS: nvidia\ndev-libs/a plainflag\n",
    )
    .unwrap();

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
        config.package_use_user,
        vec![
            (
                "dev-libs/a".to_string(),
                vec!["video_cards_nvidia".to_string()]
            ),
            ("dev-libs/a".to_string(), vec!["plainflag".to_string()]),
        ]
    );
}

#[test]
fn package_use_expand_shorthand_is_user_level_only() {
    // In repo/profile files the "VAR:" syntax stays a literal, unexpanded token:
    // Portage's shorthand support is user-only (see `parse_package_use_lines`).
    let root = TempDir::new("portage-profile-test-package-use-expand-user-only").keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(root.join("etc/portage")).unwrap();

    fs::write(
        repo_profiles.join("package.use"),
        "dev-libs/a VIDEO_CARDS: nvidia\n",
    )
    .unwrap();

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
        config.package_use_repo,
        vec![(
            "dev-libs/a".to_string(),
            vec!["VIDEO_CARDS:".to_string(), "nvidia".to_string()]
        )]
    );
}

/// Shared body of the mask/force stacking tests: repo-level plus
/// profile-chain entries stack, and a deliberately-written user-level
/// file is completely ignored (Portage has no such source; see
/// UseManager's file/variable table). Only the file names and the
/// asserted config fields differ between the plain and stable variants.
fn check_mask_and_force_stack_repo_then_profile_only_no_user_source(
    dir_name: &str,
    mask_file: &str,
    force_file: &str,
    check: impl Fn(&Config),
) {
    let root = TempDir::new(dir_name).keep();
    let repo = root.join("repo");
    let repo_profiles = repo.join("profiles");
    let portage_dir = root.join("etc/portage");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(&repo_profiles).unwrap();
    fs::create_dir_all(&portage_dir).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(repo_profiles.join(mask_file), "dev-libs/a maska\n").unwrap();
    fs::write(leaf.join(mask_file), "dev-libs/b maskb\n").unwrap();
    fs::write(portage_dir.join(mask_file), "dev-libs/c maskc\n").unwrap();
    fs::write(repo_profiles.join(force_file), "dev-libs/a forcea\n").unwrap();
    fs::write(leaf.join(force_file), "dev-libs/b forceb\n").unwrap();
    fs::write(portage_dir.join(force_file), "dev-libs/c forcec\n").unwrap();

    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
    check(&config);
}

#[test]
fn package_use_mask_and_force_stack_repo_then_profile_only_no_user_source() {
    // A deliberately-written user-level file must be completely ignored:
    // Portage has no such source (see UseManager's file/variable table).
    check_mask_and_force_stack_repo_then_profile_only_no_user_source(
        "portage-profile-test-package-use-mask-force",
        "package.use.mask",
        "package.use.force",
        |config| {
            assert_eq!(
                config.package_use_mask,
                vec![
                    ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                    ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
                ]
            );
            assert_eq!(
                config.package_use_force,
                vec![
                    ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                    ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
                ]
            );
        },
    );
}

#[test]
fn use_stable_mask_and_force_stack_profile_chain_only_and_stay_out_of_use_flags() {
    // Profile-chain only, and never folded into use_flags (stability is
    // per-candidate, so portage-repo applies these conditionally instead).
    let root = TempDir::new("portage-profile-test-use-stable-mask-force").keep();
    let repo = root.join("repo");
    let leaf = root.join("leaf-profile");
    fs::create_dir_all(repo.join("profiles")).unwrap();
    fs::create_dir_all(&leaf).unwrap();

    fs::write(leaf.join("use.stable.mask"), "stablemaskflag\n").unwrap();
    fs::write(leaf.join("use.stable.force"), "stableforceflag\n").unwrap();

    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    let make_profile = portage_dir.join("make.profile");
    let _ = fs::remove_file(&make_profile);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, &make_profile).unwrap();

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
        config.use_stable_mask,
        HashSet::from(["stablemaskflag".to_string()])
    );
    assert_eq!(
        config.use_stable_force,
        HashSet::from(["stableforceflag".to_string()])
    );
    assert!(!config.use_flags.contains("stablemaskflag"));
    assert!(!config.use_flags.contains("stableforceflag"));
}

#[test]
fn package_use_stable_mask_and_force_stack_repo_then_profile_only_no_user_source() {
    // Mirrors the non-stable test above for the stable variant; the user-level
    // file is ignored (no such source in Portage).
    check_mask_and_force_stack_repo_then_profile_only_no_user_source(
        "portage-profile-test-package-use-stable-mask-force",
        "package.use.stable.mask",
        "package.use.stable.force",
        |config| {
            assert_eq!(
                config.package_use_stable_mask,
                vec![
                    ("dev-libs/a".to_string(), vec!["maska".to_string()]),
                    ("dev-libs/b".to_string(), vec!["maskb".to_string()]),
                ]
            );
            assert_eq!(
                config.package_use_stable_force,
                vec![
                    ("dev-libs/a".to_string(), vec!["forcea".to_string()]),
                    ("dev-libs/b".to_string(), vec!["forceb".to_string()]),
                ]
            );
        },
    );
}

#[test]
fn package_use_loads_tokens_and_skips_bare_atom_lines() {
    let root = TempDir::new("portage-profile-test-package-use").keep();
    let portage_dir = root.join("etc/portage");
    fs::create_dir_all(&portage_dir).unwrap();
    fs::write(
        portage_dir.join("package.use"),
        "dev-libs/foo flag1 -flag2\n*/bar +flag3\ndev-libs/bare-no-op\n",
    )
    .unwrap();

    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("config with package.use must resolve");
    assert_eq!(
        config.package_use_user,
        vec![
            (
                "dev-libs/foo".to_string(),
                vec!["flag1".to_string(), "-flag2".to_string()]
            ),
            ("*/bar".to_string(), vec!["+flag3".to_string()]),
        ]
    );
}

/// Scaffolding for the #346 S1 user-profile tests: a temp config root with
/// `repo/profiles/leaf` as the selected profile (`make.profile` symlinked
/// to it) plus `etc/portage/profile/<file>` per `user_files`. Portage
/// 3.0.82.2 appends that directory as its last profile node
/// (LocationsManager.py:169-189); every expected value below replays an
/// S0 `u*` cell from `LLM/evidence/2026-10-15-346-s0/u/`, never portuale
/// output.
fn user_profile_test_root(
    tag: &str,
    chain_files: &[(&str, &str)],
    user_files: &[(&str, &str)],
) -> PathBuf {
    let root = TempDir::new(tag).keep();
    let leaf = root.join("repo/profiles/leaf");
    let user = root.join("etc/portage/profile");
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(&user).unwrap();
    for (file, content) in chain_files {
        fs::write(leaf.join(file), content).unwrap();
    }
    for (file, content) in user_files {
        fs::write(user.join(file), content).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, root.join("etc/portage/make.profile")).unwrap();
    root
}

fn resolve_user_profile_test_root(root: &Path) -> Config {
    resolve_config(ResolveConfigArgs {
        config_root: root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: root,
    })
    .expect("user-profile config must resolve")
}

#[test]
fn user_profile_node_is_present_only_with_make_profile() {
    // S0 u15: without `make.profile` Portage never appends the user node
    // (the run aborts as invalid-profile); portuale resolves with an
    // empty chain and must ignore the user files entirely.
    let root = TempDir::new("portage-profile-test-user-node-gate").keep();
    let user = root.join("etc/portage/profile");
    fs::create_dir_all(&user).unwrap();
    fs::write(user.join("use.force"), "x\n").unwrap();
    fs::write(user.join("package.use"), "dev-libs/t341-dep x\n").unwrap();
    let config = resolve_user_profile_test_root(&root);
    assert!(config.profile_use_layers.is_empty());
    // Only the synthetic repo level exists; the user files are ignored.
    assert_eq!(config.use_mask_force_levels.len(), 1);
    assert_eq!(
        config.use_mask_force_levels[0],
        UseMaskForceLevel::default()
    );
    assert!(config.package_use.is_empty());
    assert!(config.use_force.is_empty());

    // With `make.profile` the same directory becomes the last level.
    let root = user_profile_test_root(
        "portage-profile-test-user-node-present",
        &[],
        &[("use.force", "x\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(config.use_mask_force_levels.len(), 3);
    assert_eq!(
        config.use_mask_force_levels[2].use_force,
        vec!["x".to_string()]
    );
    assert_eq!(
        config.use_force,
        vec!["x".to_string()].into_iter().collect()
    );
}

#[test]
fn user_profile_make_defaults_and_package_use_form_the_last_level() {
    // S0 u03/u04/u17: the user `make.defaults` USE and `package.use`
    // stack after the chain, as one more `profile_use_layers` level.
    let root = user_profile_test_root(
        "portage-profile-test-user-level",
        &[("make.defaults", "USE=\"chainflag\"\n")],
        &[
            ("make.defaults", "USE=\"x\"\nFEATURES=\"test\"\n"),
            ("package.use", "dev-libs/t341-dep y\n"),
        ],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(config.profile_use_layers.len(), 2);
    assert_eq!(
        config.profile_use_layers[1].make_defaults_use,
        vec!["x".to_string()]
    );
    assert_eq!(
        config.profile_use_layers[1].package_use,
        vec![("dev-libs/t341-dep".to_string(), vec!["y".to_string()])]
    );
    // The flat `use_tokens` still carry every level's USE= in order.
    assert_eq!(
        config.use_tokens,
        vec!["chainflag".to_string(), "x".to_string()]
    );
    // The user `make.defaults` scalars land with the other layers.
    assert_eq!(
        config.other_vars.get("FEATURES").map(String::as_str),
        Some("test")
    );
}

#[test]
fn user_profile_use_force_and_mask_stack_last() {
    // S0 u02/u05/u06/u07/u12/u13: every user-profile force/mask file
    // closes its stack — the flat folds and the per-level view alike.
    let root = user_profile_test_root(
        "portage-profile-test-user-force-mask",
        &[],
        &[
            ("use.force", "x\n"),
            ("use.mask", "y\n"),
            ("use.stable.mask", "z\n"),
            ("package.use.force", "dev-libs/t341-dep x\n"),
            ("package.use.mask", "dev-libs/t341-dep y\n"),
            ("package.use.stable.force", "dev-libs/t341-dep w\n"),
        ],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.use_force,
        vec!["x".to_string()].into_iter().collect()
    );
    assert_eq!(config.use_mask, vec!["y".to_string()].into_iter().collect());
    assert_eq!(
        config.use_stable_mask,
        vec!["z".to_string()].into_iter().collect()
    );
    assert_eq!(
        config.package_use_force,
        vec![("dev-libs/t341-dep".to_string(), vec!["x".to_string()])]
    );
    assert_eq!(
        config.package_use_mask,
        vec![("dev-libs/t341-dep".to_string(), vec!["y".to_string()])]
    );
    assert_eq!(
        config.package_use_stable_force,
        vec![("dev-libs/t341-dep".to_string(), vec!["w".to_string()])]
    );
    let user_level = config.use_mask_force_levels.last().expect("user level");
    assert_eq!(user_level.use_force, vec!["x".to_string()]);
    assert_eq!(user_level.use_mask, vec!["y".to_string()]);
    assert_eq!(user_level.use_stable_mask, vec!["z".to_string()]);
    assert_eq!(
        user_level.package_use_force,
        vec![("dev-libs/t341-dep".to_string(), vec!["x".to_string()])]
    );
    assert_eq!(
        user_level.package_use_mask,
        vec![("dev-libs/t341-dep".to_string(), vec!["y".to_string()])]
    );
    assert_eq!(
        user_level.package_use_stable_force,
        vec![("dev-libs/t341-dep".to_string(), vec!["w".to_string()])]
    );
}

#[test]
fn user_profile_accept_keywords_stacks_between_chain_and_user_file() {
    // S0 u08: the user node's `package.accept_keywords` accepts the
    // package after the chain, before the user file.
    let root = user_profile_test_root(
        "portage-profile-test-user-accept-keywords",
        &[("package.accept_keywords", "dev-libs/chainpkg ~amd64\n")],
        &[(
            "package.accept_keywords",
            "dev-libs/bareacceptkeywordspkg ~amd64\n",
        )],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.package_accept_keywords,
        vec![
            ("dev-libs/chainpkg".to_string(), vec!["~amd64".to_string()]),
            (
                "dev-libs/bareacceptkeywordspkg".to_string(),
                vec!["~amd64".to_string()]
            ),
        ]
    );
}

#[test]
fn user_package_keywords_feeds_the_accept_dict_first_with_a_deprecation_warning() {
    // S0 k01/k05: the user `package.keywords` file merges into the
    // accept-keywords dict before `package.accept_keywords` entries extend
    // it per atom; the message-only deprecation warning fires on read.
    let root = user_profile_test_root("portage-profile-test-user-package-keywords", &[], &[]);
    fs::write(
        root.join("etc/portage/package.keywords"),
        "dev-libs/bareacceptkeywordspkg ~amd64\n",
    )
    .unwrap();
    fs::write(
        root.join("etc/portage/package.accept_keywords"),
        "dev-libs/bareacceptkeywordspkg -~amd64\n",
    )
    .unwrap();
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.package_accept_keywords,
        vec![
            (
                "dev-libs/bareacceptkeywordspkg".to_string(),
                vec!["~amd64".to_string()]
            ),
            (
                "dev-libs/bareacceptkeywordspkg".to_string(),
                vec!["-~amd64".to_string()]
            ),
        ]
    );
    let keywords_path = root.join("etc/portage/package.keywords");
    let accept_path = root.join("etc/portage/package.accept_keywords");
    assert!(config.config_warnings.iter().any(|w| w
        == &format!(
            "{} is deprecated, use {} instead",
            keywords_path.display(),
            accept_path.display()
        )));
}

#[test]
fn profile_package_keywords_stack_chain_then_user_node() {
    // S0 k02/k03: chain and user-profile `package.keywords` add to the
    // package's own KEYWORDS, in chain-then-node order.
    let root = user_profile_test_root(
        "portage-profile-test-profile-package-keywords",
        &[("package.keywords", "dev-libs/chainpkg amd64\n")],
        &[("package.keywords", "dev-libs/bareacceptkeywordspkg amd64\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.package_keywords_profile,
        vec![
            ("dev-libs/chainpkg".to_string(), vec!["amd64".to_string()]),
            (
                "dev-libs/bareacceptkeywordspkg".to_string(),
                vec!["amd64".to_string()]
            ),
        ]
    );
}

#[test]
fn user_profile_packages_feed_system_and_profile_sets() {
    // S0 u09/u18/u19: unstarred user `packages` lines join `@profile`
    // (the node always carries `profile-set`); starred lines join
    // `@system` only and never `@profile`.
    let root = user_profile_test_root(
        "portage-profile-test-user-packages",
        &[],
        &[("packages", "dev-libs/t341-dep\n*dev-libs/systempkg\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.profile_packages,
        vec!["dev-libs/t341-dep".to_string()]
    );
    assert_eq!(
        config.system_packages,
        vec!["dev-libs/systempkg".to_string()]
    );
}

#[test]
fn user_profile_eapi_gates_package_use_wildcards() {
    // S0 u10: with `profile/eapi` = `5`, a `*/` wildcard in the user
    // `package.use` is dropped with exactly one warning; without the
    // file (S0 u11) the same line is kept, silently.
    let root = user_profile_test_root(
        "portage-profile-test-user-eapi5",
        &[],
        &[("eapi", "5\n"), ("package.use", "*/t341-dep x\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert!(config.profile_use_layers[1].package_use.is_empty());
    assert_eq!(config.config_warnings.len(), 1);
    assert!(
        config.config_warnings[0].ends_with("/etc/portage/profile/package.use: */t341-dep"),
        "unexpected warning: {}",
        config.config_warnings[0]
    );

    let root = user_profile_test_root(
        "portage-profile-test-user-noeapi",
        &[],
        &[("package.use", "*/t341-dep x\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(
        config.profile_use_layers[1].package_use,
        vec![("*/t341-dep".to_string(), vec!["x".to_string()])]
    );
    assert!(config.config_warnings.is_empty());
}

#[test]
fn user_profile_unmask_counters_chain_mask_and_parent_stays_unwalked() {
    // S0 u14: the user `package.unmask` lifts a chain `package.mask`
    // (both flat folds already stack this way; the node must preserve
    // it). S0 u16: a `parent` file inside the user profile is never
    // walked — the pointed-to `package.use` must have no effect.
    let root = user_profile_test_root(
        "portage-profile-test-user-unmask",
        &[("package.mask", "dev-libs/t341-dep\n")],
        &[("package.unmask", "dev-libs/t341-dep\n")],
    );
    let config = resolve_user_profile_test_root(&root);
    assert_eq!(config.package_mask, vec!["dev-libs/t341-dep".to_string()]);
    assert_eq!(config.package_unmask, vec!["dev-libs/t341-dep".to_string()]);

    let root = TempDir::new("portage-profile-test-user-parent").keep();
    let leaf = root.join("repo/profiles/leaf");
    let user = root.join("etc/portage/profile");
    let pointed = root.join("pointed");
    fs::create_dir_all(&leaf).unwrap();
    fs::create_dir_all(&user).unwrap();
    fs::create_dir_all(&pointed).unwrap();
    fs::write(pointed.join("package.use"), "dev-libs/t341-dep x\n").unwrap();
    fs::write(user.join("parent"), "../pointed\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&leaf, root.join("etc/portage/make.profile")).unwrap();
    let config = resolve_user_profile_test_root(&root);
    // Chain level plus the user level only; the `parent` target adds none.
    assert_eq!(config.profile_use_layers.len(), 2);
    assert!(config.profile_use_layers[1].package_use.is_empty());
}
