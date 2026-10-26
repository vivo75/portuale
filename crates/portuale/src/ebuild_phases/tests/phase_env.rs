use super::*;

/// Path variables lose their trailing slash from EAPI 7 on, so
/// `ROOT=/` is exported empty. The decision reads
/// `path_variables_end_with_trailing_slash` (EAPI 6 and below), never a
/// hard-coded EAPI list.
#[test]
fn eapi_path_var_strips_the_trailing_slash_from_eapi_7() {
    use portage_dep::eapi::eapi_attrs;
    let trailing = |eapi: &str| {
        eapi_attrs(Some(eapi))
            .unwrap()
            .path_variables_end_with_trailing_slash
    };
    assert_eq!(eapi_path_var(trailing("8"), "//"), "");
    assert_eq!(
        eapi_path_var(trailing("7"), "/var/tmp/p/image/"),
        "/var/tmp/p/image"
    );
    assert_eq!(eapi_path_var(trailing("6"), "/"), "/");
    assert_eq!(
        eapi_path_var(trailing("5"), "/var/tmp/p/image/"),
        "/var/tmp/p/image/"
    );
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

/// The xattr helper dir goes in front of the helpers dir only when
/// `xattr` is in `FEATURES`.
#[test]
fn phase_path_puts_the_xattr_wrapper_first_under_features_xattr() {
    let helpers = Path::new("/bin/ebuild-helpers");
    let extra = [("PATH".to_string(), "/usr/bin:/bin".to_string())];
    assert_eq!(
        phase_path(helpers, &extra, "sandbox xattr userpriv"),
        "/bin/ebuild-helpers/xattr:/bin/ebuild-helpers:/usr/bin:/bin"
    );
    assert_eq!(
        phase_path(helpers, &extra, "sandbox userpriv"),
        "/bin/ebuild-helpers:/usr/bin:/bin"
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
            drop_ebuild_phase: false,
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

/// E1: `phase_func_name` is Portage's `_phase_func_map` exactly: the 15
/// runnable phases map to their function, every internal phase maps to
/// nothing (so `EBUILD_PHASE_FUNC` stays unset there).
// Portage: doebuild.py, _phase_func_map
#[test]
fn phase_func_name_maps_exactly_portages_phase_func_map() {
    for (phase, func) in [
        ("config", "pkg_config"),
        ("setup", "pkg_setup"),
        ("nofetch", "pkg_nofetch"),
        ("unpack", "src_unpack"),
        ("prepare", "src_prepare"),
        ("configure", "src_configure"),
        ("compile", "src_compile"),
        ("test", "src_test"),
        ("install", "src_install"),
        ("preinst", "pkg_preinst"),
        ("postinst", "pkg_postinst"),
        ("prerm", "pkg_prerm"),
        ("postrm", "pkg_postrm"),
        ("info", "pkg_info"),
        ("pretend", "pkg_pretend"),
    ] {
        assert_eq!(phase_func_name(phase), Some(func), "{phase}");
    }
    for phase in [
        "clean", "cleanrm", "depend", "fetch", "instprep", "merge", "package", "qmerge", "unmerge",
        "bogus",
    ] {
        assert_eq!(phase_func_name(phase), None, "{phase}");
    }
    // `ESYSROOT` survives only for `setup` and the `src_*` phases.
    // Portage: config.py, environ (`src_like_phase`)
    assert!(is_src_like_phase("setup"));
    for phase in [
        "unpack",
        "prepare",
        "configure",
        "compile",
        "test",
        "install",
    ] {
        assert!(is_src_like_phase(phase), "{phase}");
    }
    for phase in [
        "pretend", "preinst", "postinst", "prerm", "postrm", "config", "info", "clean", "instprep",
    ] {
        assert!(!is_src_like_phase(phase), "{phase}");
    }
}

/// E1–E6: a standalone `setup` exports the per-EAPI phase environment —
/// `EBUILD_PHASE_FUNC`, `A`, `SYSROOT`/`ESYSROOT`/`BROOT`, `SLOT`,
/// `REPLACING_VERSIONS` (fresh root: `""`), `IUSE_EFFECTIVE` with its
/// `___in_portage_iuse` body, and `PORTAGE_INTERNAL_CALLER`.
/// `dev-libs/usebuildpkg` (EAPI 8, `IUSE="buildflag"`, `SLOT="0"`,
/// no `SRC_URI`) pins the values; Portage's own are in
/// `LLM/evidence/2026-10-12-348-s0/b/b-env-7-portage-ebuild-setup.*`.
#[test]
fn standalone_setup_exports_the_eapi_phase_environment() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let repo = fixtures.join("repo");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-s2-env").keep();
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let vars_for = |phase: &str, extra_env: &[(String, String)]| {
        let env = compute_environment(
            &repo.join("dev-libs/usebuildpkg/usebuildpkg-1.0.ebuild"),
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
            extra_env,
            drop_ebuild_phase: false,
        })
    };
    let get = |vars: &[(String, String)], key: &str| {
        vars.iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .next_back()
    };
    let setup_vars = vars_for("setup", &[]);
    assert_eq!(
        get(&setup_vars, "EBUILD_PHASE_FUNC").as_deref(),
        Some("pkg_setup")
    );
    assert_eq!(get(&setup_vars, "A").as_deref(), Some(""));
    assert_eq!(get(&setup_vars, "SYSROOT").as_deref(), Some(""));
    assert_eq!(get(&setup_vars, "ESYSROOT").as_deref(), Some(""));
    assert_eq!(get(&setup_vars, "BROOT").as_deref(), Some(""));
    assert_eq!(get(&setup_vars, "SLOT").as_deref(), Some("0"));
    assert_eq!(get(&setup_vars, "REPLACING_VERSIONS").as_deref(), Some(""));
    assert_eq!(
        get(&setup_vars, "PORTAGE_INTERNAL_CALLER").as_deref(),
        Some("1")
    );
    let iuse = get(&setup_vars, "IUSE_EFFECTIVE").expect("IUSE_EFFECTIVE is set");
    assert!(
        iuse.split_whitespace().any(|f| f == "buildflag"),
        "declared IUSE joins the implicit set: {iuse}"
    );
    let body = get(&setup_vars, "BASH_FUNC____in_portage_iuse%%")
        .expect("the in_portage_iuse gate is exported");
    assert!(body.starts_with("() {"), "{body}");
    assert!(body.contains("[\"buildflag\"]=1"), "{body}");

    // `preinst` is not src-like: no `ESYSROOT`, but the function,
    // `REPLACING_VERSIONS` default and `SLOT` stay.
    let preinst_vars = vars_for("preinst", &[]);
    assert_eq!(
        get(&preinst_vars, "EBUILD_PHASE_FUNC").as_deref(),
        Some("pkg_preinst")
    );
    assert_eq!(get(&preinst_vars, "ESYSROOT"), None);
    assert_eq!(get(&preinst_vars, "SYSROOT").as_deref(), Some(""));
    assert_eq!(
        get(&preinst_vars, "REPLACING_VERSIONS").as_deref(),
        Some("")
    );

    // `prerm`: neither replace var survives without a caller value.
    let prerm_vars = vars_for("prerm", &[]);
    assert_eq!(
        get(&prerm_vars, "EBUILD_PHASE_FUNC").as_deref(),
        Some("pkg_prerm")
    );
    assert_eq!(get(&prerm_vars, "REPLACING_VERSIONS"), None);
    assert_eq!(get(&prerm_vars, "REPLACED_BY_VERSION"), None);

    // Caller values win, but the phase gates still apply: a
    // `REPLACING_VERSIONS` threaded into `compile` is dropped, a
    // binary `MERGE_TYPE` and a `REPLACED_BY_VERSION` for `prerm` stay.
    let compile_vars = vars_for(
        "compile",
        &[
            ("MERGE_TYPE".to_string(), "binary".to_string()),
            ("REPLACING_VERSIONS".to_string(), "7".to_string()),
        ],
    );
    assert_eq!(get(&compile_vars, "MERGE_TYPE").as_deref(), Some("binary"));
    assert_eq!(get(&compile_vars, "REPLACING_VERSIONS"), None);
    let prerm_replaced = vars_for(
        "prerm",
        &[("REPLACED_BY_VERSION".to_string(), "8".to_string())],
    );
    assert_eq!(
        get(&prerm_replaced, "REPLACED_BY_VERSION").as_deref(),
        Some("8")
    );
    let setup_replaced = vars_for(
        "setup",
        &[("REPLACED_BY_VERSION".to_string(), "8".to_string())],
    );
    assert_eq!(get(&setup_replaced, "REPLACED_BY_VERSION"), None);

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Below `iuse_effective` / `broot` / `sysroot` (EAPI 4 here) the gates
/// close: no `EBUILD_PHASE_FUNC`, no `ESYSROOT`/`BROOT`, no
/// `IUSE_EFFECTIVE` pair — while `SYSROOT`, `A` and `SLOT` keep their
/// ungated shapes. `MERGE_TYPE` stays: `exports_merge_type` opens at 4.
// Portage: eapi.py, _get_eapi_attrs; config.py, environ
#[test]
fn old_eapi_closes_the_gated_phase_exports() {
    let tmp = TempDir::new("ebuild-phases-test-s2-eapi4").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();
    let pkg_dir = repo.join("dev-libs/oldpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("oldpkg-1.0.ebuild"), "EAPI=4\nSLOT=\"0\"\n").unwrap();
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("oldpkg-1.0"),
        "EAPI=4\nIUSE=\nKEYWORDS=amd64\nSLOT=0\n\
         _md5_=0000000000000000000000000000000\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("pt");
    let env = compute_environment(&pkg_dir.join("oldpkg-1.0.ebuild"), &portage_tmpdir)
        .expect("synthetic ebuild parses");
    assert_eq!(env.eapi, "4");
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let vars = phase_env_vars(PhaseEnvVarsArgs {
        env: &env,
        root: &tmp,
        ebuild_phase_value: "setup",
        debug: false,
        bin_dir: &bin_dir,
        helpers_dir: &bin_dir.join("ebuild-helpers"),
        config_root: &tmp,
        extra_env: &[],
        drop_ebuild_phase: false,
    });
    let get = |key: &str| {
        vars.iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .next_back()
    };
    assert_eq!(get("EBUILD_PHASE_FUNC"), None);
    assert_eq!(get("ESYSROOT"), None);
    assert_eq!(get("BROOT"), None);
    assert_eq!(get("IUSE_EFFECTIVE"), None);
    assert_eq!(get("BASH_FUNC____in_portage_iuse%%"), None);
    assert_eq!(get("SYSROOT").as_deref(), Some(""));
    assert_eq!(get("A").as_deref(), Some(""));
    assert_eq!(get("SLOT").as_deref(), Some("0"));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A `misc-functions.sh` spawn (`install_qa_check ...`) is not an ebuild
/// phase — Portage's `MiscFunctionsProcess._spawn` pops `EBUILD_PHASE`
/// from the child env, so `use()`'s `IUSE`-membership gate stays off there —
/// while an ebuild phase spawn keeps it. Everything else is identical.
/// Portage: _emerge/EbuildPhase.py, MiscFunctionsProcess._spawn
#[test]
fn drop_ebuild_phase_pops_only_the_phase_var() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let repo = fixtures.join("repo");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-s2-drop-phase").keep();
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let env = compute_environment(
        &repo.join("dev-libs/usebuildpkg/usebuildpkg-1.0.ebuild"),
        &portage_tmpdir,
    )
    .expect("fixture ebuild parses");
    let vars_for = |drop: bool| {
        phase_env_vars(PhaseEnvVarsArgs {
            env: &env,
            root: &fixtures,
            ebuild_phase_value: "install",
            debug: false,
            bin_dir: &bin_dir,
            helpers_dir: &bin_dir.join("ebuild-helpers"),
            config_root: &fixtures,
            extra_env: &[],
            drop_ebuild_phase: drop,
        })
    };
    let get = |vars: &[(String, String)], key: &str| {
        vars.iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .next_back()
    };
    let kept = vars_for(false);
    assert_eq!(get(&kept, "EBUILD_PHASE").as_deref(), Some("install"));
    let popped = vars_for(true);
    assert_eq!(get(&popped, "EBUILD_PHASE"), None);
    // The rest of the spawn env is untouched.
    for key in [
        "EBUILD_PHASE_FUNC",
        "PORTAGE_INTERNAL_CALLER",
        "IUSE_EFFECTIVE",
        "USE",
        "T",
        "D",
        "S",
    ] {
        assert_eq!(
            get(&popped, key),
            get(&kept, key),
            "{key} must survive the pop"
        );
    }
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The brush setup script defines `___in_portage_iuse` instead of
/// emitting an unparsable `export BASH_FUNC_…%%=…` line.
#[test]
fn brush_setup_script_defines_in_portage_iuse_as_a_function() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let repo = fixtures.join("repo");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-s2-brush").keep();
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let env = compute_environment(
        &repo.join("dev-libs/usebuildpkg/usebuildpkg-1.0.ebuild"),
        &portage_tmpdir,
    )
    .expect("fixture ebuild parses");
    let script = phase_setup_script(PhaseSetupScriptArgs {
        env: &env,
        root: &fixtures,
        ebuild_phase_value: "setup",
        debug: false,
        bin_dir: &bin_dir,
        helpers_dir: &bin_dir.join("ebuild-helpers"),
        config_root: &fixtures,
        extra_env: &[],
        drop_ebuild_phase: false,
    });
    assert!(script.contains("___in_portage_iuse() {"), "{script}");
    assert!(!script.contains("export BASH_FUNC"), "{script}");
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `PORTAGE_CONFIGROOT` always carries Portage's trailing slash
/// (`LocationsManager` normalizes `config_root.rstrip(sep) + sep`).
#[test]
fn config_root_with_slash_matches_portage_locations_manager() {
    assert_eq!(
        config_root_with_slash(Path::new("/var/tmp/s346/e-e01/cfg")),
        "/var/tmp/s346/e-e01/cfg/"
    );
    assert_eq!(config_root_with_slash(Path::new("/")), "/");
    assert_eq!(
        config_root_with_slash(Path::new("/already/slashed/")),
        "/already/slashed/"
    );
}

/// The phase env names the user's bashrc and hook directory under the
/// config root, as `doebuild_environment` does (`doebuild.py:535-540`;
/// observed values in `LLM/evidence/2026-10-18-372-s0/`). Backlog #372.
#[test]
fn phase_env_names_the_user_bashrc_and_hook_dir_under_the_config_root() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-bashrc-vars");
    let bin_dir = bin_dir().unwrap().to_path_buf();
    let env = compute_environment(
        &fixtures.join("repo/dev-libs/usebuildpkg/usebuildpkg-1.0.ebuild"),
        portage_tmpdir.path(),
    )
    .expect("fixture ebuild parses");
    let vars = phase_env_vars(PhaseEnvVarsArgs {
        env: &env,
        root: &fixtures,
        ebuild_phase_value: "compile",
        debug: false,
        bin_dir: &bin_dir,
        helpers_dir: &bin_dir.join("ebuild-helpers"),
        config_root: Path::new("/var/tmp/cfg"),
        extra_env: &[],
        drop_ebuild_phase: false,
    });
    let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(
        get("PORTAGE_BASHRC"),
        Some("/var/tmp/cfg/etc/portage/bashrc")
    );
    assert_eq!(
        get("PM_EBUILD_HOOK_DIR"),
        Some("/var/tmp/cfg/etc/portage/env")
    );
}
