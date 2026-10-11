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

    // No value anywhere: `None` (the caller falls back to real
    // `make.globals`'s own default, `BINPKG_FORMAT="gpkg"` --
    // `ebuild_package::resolve_binpkg_format`).
    fs::write(portage_dir.join("make.conf"), "CFLAGS=\"-O2\"\n").unwrap();
    with_test_env(&[], || {
        let c = resolve();
        assert!(!c.other_vars.contains_key("BINPKG_FORMAT"));
        assert_eq!(env_over_config_scalar(&c, "BINPKG_FORMAT"), None);
    });
}
