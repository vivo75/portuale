use super::*;

#[test]
fn env_over_config_scalar_follows_the_calling_env_over_make_conf_chain() {
    // Backlog #173: `BINPKG_FORMAT`'s full chain is calling env over
    // `make.conf` over profile `make.defaults` over `make.globals`
    // (real `config.regenerate()`'s `env` `USE_ORDER` layer on top of
    // the file stack). The scratch root carries no `make.globals`
    // (the fixture shape), so the file side is `make.conf` here.
    let root = TempDir::new("portage-profile-test-binpkg-format").keep();
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
    fs::write(portage_dir.join("make.conf"), "BINPKG_FORMAT=\"xpak\"\n").unwrap();
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

    // No env: the `make.conf` value wins over the (absent) profile /
    // `make.globals` values.
    with_test_env(&[], || {
        let c = resolve();
        assert_eq!(
            c.other_vars.get("BINPKG_FORMAT").map(String::as_str),
            Some("xpak")
        );
        assert_eq!(
            env_over_config_scalar(&c, "BINPKG_FORMAT").as_deref(),
            Some("xpak")
        );
    });

    // Calling env beats `make.conf`.
    with_test_env(&[("BINPKG_FORMAT", "gpkg")], || {
        let c = resolve();
        assert_eq!(
            env_over_config_scalar(&c, "BINPKG_FORMAT").as_deref(),
            Some("gpkg")
        );
    });

    // No env and no `make.conf` value: the embedded `make.globals` default,
    // `BINPKG_FORMAT="gpkg"`, is the base layer on every root.
    fs::write(portage_dir.join("make.conf"), "CFLAGS=\"-O2\"\n").unwrap();
    with_test_env(&[], || {
        let c = resolve();
        assert_eq!(
            c.other_vars.get("BINPKG_FORMAT").map(String::as_str),
            Some("gpkg")
        );
    });
}

#[test]
fn the_embedded_make_globals_seeds_the_unmerge_defaults_under_the_stack() {
    // Backlog #356 S4 (Q3), reworked by #384: the embedded `make.globals`
    // (the base of every root, hermetic ones included) carries Portage's
    // two unmerge defaults at the bottom of the stack before `make.conf` expansion, so later layers
    // fold as on a real system. Expected behaviour from Portage 3.0.82.2
    // (`cnf/make.globals:77-84` `unmerge-orphans`, `:93`
    // `UNINSTALL_IGNORE=/lib/modules/*`).
    let root = TempDir::new("portage-profile-test-synthetic-unmerge-base").keep();
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
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
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

    // Bare `make.conf`: the base token folds in, the default paths land.
    fs::write(portage_dir.join("make.conf"), "CFLAGS=\"-O2\"\n").unwrap();
    with_test_env(&[], || {
        let c = resolve();
        assert!(
            c.resolved_incremental("FEATURES")
                .is_some_and(|f| f.iter().any(|t| t == "unmerge-orphans")),
            "the embedded make.globals must carry unmerge-orphans"
        );
        assert_eq!(
            c.other_vars.get("UNINSTALL_IGNORE").map(String::as_str),
            Some("/lib/modules/*")
        );
    });

    // An explicit `-unmerge-orphans` drops the token; a literal
    // `UNINSTALL_IGNORE=` replaces the paths.
    fs::write(
        portage_dir.join("make.conf"),
        "FEATURES=\"-unmerge-orphans\"\nUNINSTALL_IGNORE=\"/x/*\"\n",
    )
    .unwrap();
    with_test_env(&[], || {
        let c = resolve();
        assert!(
            c.resolved_incremental("FEATURES")
                .is_none_or(|f| !f.iter().any(|t| t == "unmerge-orphans")),
            "a make.conf negation must beat the embedded base"
        );
        assert_eq!(
            c.other_vars.get("UNINSTALL_IGNORE").map(String::as_str),
            Some("/x/*")
        );
    });

    // The append form expands through the seeded scalar, as in Portage.
    fs::write(
        portage_dir.join("make.conf"),
        "UNINSTALL_IGNORE=\"${UNINSTALL_IGNORE} /usr/share/t355/ign/*\"\n",
    )
    .unwrap();
    with_test_env(&[], || {
        let c = resolve();
        assert_eq!(
            c.other_vars.get("UNINSTALL_IGNORE").map(String::as_str),
            Some("/lib/modules/* /usr/share/t355/ign/*")
        );
    });
    let _ = std::fs::remove_dir_all(&root);
}
