use super::*;

/// Path variables lose their trailing slash from EAPI 7 on, so
/// `ROOT=/` is exported empty.
#[test]
fn eapi_path_var_strips_the_trailing_slash_from_eapi_7() {
    assert_eq!(eapi_path_var("8", "//"), "");
    assert_eq!(eapi_path_var("7", "/var/tmp/p/image/"), "/var/tmp/p/image");
    assert_eq!(eapi_path_var("6", "/"), "/");
    assert_eq!(eapi_path_var("5", "/var/tmp/p/image/"), "/var/tmp/p/image/");
}

/// A threaded `PATH` pair is the base behind the helper dir,
/// deduplicated, and never replaces the helper-prefixed value verbatim.
#[test]
fn phase_path_prefixes_helpers_onto_the_threaded_path() {
    let helpers = Path::new("/bin/ebuild-helpers");
    let extra = [
        ("PATH".to_string(), "/env/bin".to_string()),
        (
            "PATH".to_string(),
            "/usr/local/bin:/bin/ebuild-helpers:/usr/bin:/usr/local/bin".to_string(),
        ),
    ];
    assert_eq!(
        phase_path(helpers, &extra, ""),
        "/bin/ebuild-helpers:/usr/local/bin:/usr/bin"
    );
}

/// Every brush `export` value is single-quoted, so config-derived
/// values can neither word-split nor command-substitute.
#[test]
fn shell_single_quote_neutralises_every_shell_metacharacter() {
    assert_eq!(shell_single_quote("plain"), "'plain'");
    assert_eq!(shell_single_quote("a b"), "'a b'");
    assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
    assert_eq!(
        shell_single_quote("$(touch /tmp/pwned) `id` \\ \" $HOME"),
        "'$(touch /tmp/pwned) `id` \\ \" $HOME'"
    );
    assert_eq!(shell_single_quote(""), "''");
    assert_eq!(
        shell_single_quote("-O2 -DMSG='hi'\n-Wl,-z,now"),
        "'-O2 -DMSG='\\''hi'\\''\n-Wl,-z,now'"
    );
}

#[test]
fn eclass_locations_value_quotes_the_containing_repo_root() {
    // Canonicalized first, as `compute_environment` always hands it.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo")
        .canonicalize()
        .unwrap();
    let pkg_dir = repo_root.join("dev-libs/eclasspkg");
    let value = eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root"));
    // Portage: bin/ebuild.sh, the PORTAGE_ECLASS_LOCATIONS eval.
    assert_eq!(value, format!("'{}'", repo_root.display()));
}

#[test]
fn eclass_locations_value_is_empty_outside_any_repo_checkout() {
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_value_none").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("dev-libs/standalone");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(
        eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root")),
        ""
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn eclass_locations_value_puts_the_own_repo_first_then_masters_in_declared_order() {
    // Portage: config.py, eclass_locations (own repo first, then masters).
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_masters_order").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let main = tmp.join("main");
    let secondary = tmp.join("secondary");
    let overlay = tmp.join("overlay");
    for repo in [&main, &secondary, &overlay] {
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
    }
    std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
    std::fs::write(secondary.join("profiles/repo_name"), "secondary\n").unwrap();
    std::fs::write(overlay.join("profiles/repo_name"), "overlay\n").unwrap();
    std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
    std::fs::write(
        tmp.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n\
             [main]\nlocation = {}\n\n\
             [secondary]\nlocation = {}\n\n\
             [overlay]\nlocation = {}\nmasters = main secondary\n",
            main.display(),
            secondary.display(),
            overlay.display(),
        ),
    )
    .unwrap();

    let pkg_dir = overlay.join("dev-libs/overlaypkg");
    let value = eclass_locations_value(&pkg_dir, &tmp);
    assert_eq!(
        value,
        format!(
            "'{}' '{}' '{}'",
            overlay.display(),
            secondary.display(),
            main.display()
        ),
        "own repo first, then masters in real declared order"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn eclass_locations_value_does_not_duplicate_the_own_repo_when_it_is_also_a_master() {
    // The own repo is not duplicated when it is also a master.
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_no_dup").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let main = tmp.join("main");
    std::fs::create_dir_all(main.join("profiles")).unwrap();
    std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
    std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
    std::fs::write(
        tmp.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = {}\n",
            main.display()
        ),
    )
    .unwrap();

    let pkg_dir = main.join("dev-libs/mainpkg");
    let value = eclass_locations_value(&pkg_dir, &tmp);
    assert_eq!(value, format!("'{}'", main.display()));

    let _ = std::fs::remove_dir_all(&tmp);
}

/// A standalone phase env layers a matching `package.env` entry's env
/// file over the base flags, with no resolved graph entry anywhere.
#[test]
fn standalone_phase_env_layers_matching_package_env_build_vars() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let repo = fixtures.join("repo");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-standalone-penv").keep();
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let vars_for = |pkg: &str, pf: &str, phase: &str| {
        let env = compute_environment(
            &repo.join(format!("dev-libs/{pkg}/{pf}.ebuild")),
            &portage_tmpdir,
        )
        .expect("fixture ebuild parses");
        phase_env_vars(PhaseEnvVarsArgs {
            env: &env,
            root: &fixtures,
            ebuild_phase_value: phase,
            debug: false,
            bin_dir: &bin_dir,
            helpers_dir: &bin_dir.join("ebuild-helpers"),
            config_root: &fixtures,
            extra_env: &[],
        })
    };
    let get = |vars: &[(String, String)], key: &str| {
        vars.iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .next_back()
    };
    let setup_vars = vars_for("penvbuildpkg", "penvbuildpkg-1.0", "setup");
    assert_eq!(
        get(&setup_vars, "CFLAGS").as_deref(),
        Some("-Os -march=fixturepkgenv")
    );
    assert_eq!(get(&setup_vars, "MAKEOPTS").as_deref(), Some("-j7"));
    assert_eq!(get(&setup_vars, "CC").as_deref(), Some("fixture-cc"));
    assert_eq!(get(&setup_vars, "CXX").as_deref(), Some("fixture-cxx"));
    assert_eq!(get(&setup_vars, "AR").as_deref(), Some("fixture-ar"));
    assert_eq!(
        get(&setup_vars, "RUSTFLAGS").as_deref(),
        Some("-C target-cpu=fixturepkg")
    );
    assert_eq!(get(&setup_vars, "ENV_UNSET").as_deref(), Some("PENV_UNSET"));
    let plain_vars = vars_for("newpkg", "newpkg-1.0", "setup");
    assert!(
        !plain_vars
            .iter()
            .any(|(_, v)| v.contains("-march=fixturepkgenv")),
        "unmapped package must not see package.env values"
    );
    let depend_vars = vars_for("penvbuildpkg", "penvbuildpkg-1.0", "depend");
    assert!(
        !depend_vars
            .iter()
            .any(|(_, v)| v.contains("-march=fixturepkgenv")),
        "depend keeps its empty base"
    );
}
