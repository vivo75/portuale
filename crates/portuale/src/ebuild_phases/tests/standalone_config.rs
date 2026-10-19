use super::*;

#[test]
fn restrict_mirror_from_restrict_evaluates_conditionals_against_the_empty_use_set() {
    assert!(restrict_mirror_from_restrict("mirror"));
    assert!(restrict_mirror_from_restrict("fetch mirror"));
    // The deprecated negative spelling counts too (Portage: fetch.py).
    assert!(restrict_mirror_from_restrict("nomirror"));
    assert!(!restrict_mirror_from_restrict(""));
    assert!(!restrict_mirror_from_restrict("fetch strip"));
    // USE-conditional: the fetch-side USE set is always empty here,
    // so `foo? ( mirror )` drops entirely -- not a literal token match
    assert!(!restrict_mirror_from_restrict("foo? ( mirror )"));
    assert!(restrict_mirror_from_restrict("mirror foo? ( strip )"));
}

#[test]
fn restrict_fetch_from_restrict_matches_the_fetch_token_only() {
    assert!(restrict_fetch_from_restrict("fetch"));
    assert!(restrict_fetch_from_restrict("mirror fetch"));
    assert!(!restrict_fetch_from_restrict(""));
    assert!(!restrict_fetch_from_restrict("mirror strip"));
    // USE-conditional drops against the always-empty fetch-side USE
    assert!(!restrict_fetch_from_restrict("foo? ( fetch )"));
}

#[test]
fn depend_phase_reduces_restrict_on_the_config_use_set() {
    // A conditional RESTRICT reduces on the config USE set for depend,
    // but drops its gated tokens on the empty set; see notes for the fixture.
    let tmp = TempDir::new("ebuild-phases-test-depend_use_set").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();
    let pkg_dir = repo.join("dev-libs/condrestrictpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("condrestrictpkg-1.0.ebuild"),
        "EAPI=8\nSLOT=\"0\"\n",
    )
    .unwrap();
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("condrestrictpkg-1.0"),
        "DEFINED_PHASES=-\nEAPI=8\nIUSE=\nKEYWORDS=amd64\n\
         RESTRICT=confflag? ( strip ) other? ( bindist )\nSLOT=0\n\
         _md5_=0000000000000000000000000000000\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("pt");
    let env = compute_environment(&pkg_dir.join("condrestrictpkg-1.0.ebuild"), &portage_tmpdir)
        .expect("synthetic ebuild parses");
    let empty = std::collections::HashSet::new();
    assert_eq!(
        restrict_and_properties(&env, &empty),
        (String::new(), String::new())
    );
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let use_set = depend_use_set(&env, &fixtures, &fixtures);
    assert!(use_set.contains("confflag"), "{use_set:?}");
    assert_eq!(
        restrict_and_properties(&env, &use_set),
        ("strip".to_string(), String::new())
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn flat_field_on_dedups_and_sorts_like_portage_flatten() {
    // Deduped and sorted like Portage's _flatten (Portage: config.py, _flatten).
    let empty = std::collections::HashSet::new();
    assert_eq!(flat_field_on("!test? ( test ) test", &empty), "test");
    assert_eq!(flat_field_on("zoo alpha zoo", &empty), "alpha zoo");
    let mut use_set = std::collections::HashSet::new();
    use_set.insert("bar".to_string());
    assert_eq!(
        flat_field_on("foo bar? ( bar baz )", &use_set),
        "bar baz foo"
    );
    assert_eq!(flat_field_on("foo bar? ( bar baz )", &empty), "foo");
    assert_eq!(flat_field_on("", &empty), "");
}

/// The `depend` phase never gets the standalone base env: the loader
/// short-circuits before any config load.
#[test]
fn depend_phase_standalone_base_env_stays_empty_with_a_real_repo() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phaseenvpkg/phaseenvpkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-depend-base").keep();
    let env = compute_environment(&ebuild_path, &portage_tmpdir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let (use_str, flags) =
        phase_standalone_base_env(&env, &fixtures, Path::new("/"), "depend", &[]);
    assert_eq!(use_str, "", "depend must not get a config-derived USE");
    assert!(flags.is_empty(), "depend must not get config build vars");
    // The `extra_env`-carries-USE guard is the same short-circuit.
    let (use_str, flags) = phase_standalone_base_env(
        &env,
        &fixtures,
        Path::new("/"),
        "install",
        &[("USE".to_string(), "confflag".to_string())],
    );
    assert_eq!(use_str, "");
    assert!(flags.is_empty());
}

/// A matched per-package `PORTAGE_TMPDIR` re-derives the tmpdir when the
/// calling environment does not carry the key; a missing dir fails.
#[test]
fn per_package_portage_tmpdir_feeds_the_build_directory() {
    let cpv = "dev-libs/envdumppkg-1.0:0/0";
    let run = Path::new("/var/tmp/portage-test-default");
    let files = |v: &str| {
        vec![(
            "dev-libs/envdumppkg".to_string(),
            vec![("PORTAGE_TMPDIR".to_string(), v.to_string())],
        )]
    };
    // No match: the run tmpdir stands.
    let out = resolve_entry_portage_tmpdir(
        &files("/tmp/probe-tmp"),
        "dev-libs/other-1.0:0/0",
        run,
        None,
    )
    .expect("no match keeps the run tmpdir");
    assert_eq!(out, run);
    let out =
        resolve_entry_portage_tmpdir(&[], cpv, run, None).expect("no entry keeps the run tmpdir");
    assert_eq!(out, run);
    let probe = TempDir::new("ebuild-phases-test-per_package_portage_tmpdir").keep();
    let out = resolve_entry_portage_tmpdir(
        &files(probe.to_str().expect("probe path is UTF-8")),
        cpv,
        run,
        None,
    )
    .expect("matched dir resolves");
    assert_eq!(out, probe);
    let probe2 = probe.join("second");
    std::fs::create_dir_all(&probe2).expect("second probe tmpdir is creatable");
    let mut two = files(probe.to_str().expect("probe path is UTF-8"));
    two.push((
        "dev-libs/envdumppkg".to_string(),
        vec![(
            "PORTAGE_TMPDIR".to_string(),
            probe2.to_str().expect("probe path is UTF-8").to_string(),
        )],
    ));
    let out = resolve_entry_portage_tmpdir(&two, cpv, run, None).expect("later match wins");
    assert_eq!(out, probe2);
    // Matched, directory missing: Portage's `_check_temp_dir` failure.
    let missing = probe.join("no-such-dir");
    let err = resolve_entry_portage_tmpdir(
        &files(missing.to_str().expect("probe path is UTF-8")),
        cpv,
        run,
        None,
    )
    .expect_err("missing dir must fail");
    assert_eq!(
        err,
        format!(
            "The directory specified in your PORTAGE_TMPDIR variable, '{}',\n\
             does not exist.  Please create this directory or correct your PORTAGE_TMPDIR setting.",
            missing.display()
        )
    );
    // The calling environment wins over the env file.
    let out = resolve_entry_portage_tmpdir(
        &files(probe.to_str().expect("probe path is UTF-8")),
        cpv,
        run,
        Some(run),
    )
    .expect("process tmpdir wins");
    assert_eq!(out, run);
    let out = resolve_entry_portage_tmpdir(&files(""), cpv, run, None)
        .expect("empty match keeps the run tmpdir");
    assert_eq!(out, run);
    std::fs::remove_dir_all(&probe).ok();
}

/// The standalone compressor resolver reads the matched `package.env`
/// triple through the ebuild's own md5-cache identity.
#[test]
fn resolve_standalone_binpkg_compress_matches_the_twin_fixture() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let config_root = fixtures.clone();
    let matched = resolve_standalone_binpkg_compress(
        &fixtures.join("repo/dev-libs/penvcmppkg/penvcmppkg-1.0.ebuild"),
        &config_root,
        &fixtures,
    );
    assert_eq!(
        matched,
        Some(("gzip".to_string(), None, None,)),
        "the penv-compress match must resolve, with no flags/bzip2 overrides"
    );
    let unmatched = resolve_standalone_binpkg_compress(
        &fixtures.join("repo/dev-libs/newpkg/newpkg-1.0.ebuild"),
        &config_root,
        &fixtures,
    );
    assert_eq!(unmatched, None, "no match, no override");
}

/// The standalone chain-scalar reader serves global binpkg scalars
/// below the calling env; see notes for the fixture contract.
#[test]
fn resolve_standalone_chain_scalar_reads_the_fixture_make_conf() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let config_root = fixtures.clone();
    let ebuild = fixtures.join("repo/dev-libs/newpkg/newpkg-1.0.ebuild");
    let pkgdir = resolve_standalone_chain_scalar(&ebuild, &config_root, &fixtures, "PKGDIR")
        .expect("fixture make.conf sets PKGDIR");
    assert!(
        pkgdir.ends_with("pkgdir"),
        "the file-side PKGDIR must reach the caller, got {pkgdir:?}"
    );
    assert_eq!(
        resolve_standalone_chain_scalar(
            &ebuild,
            &config_root,
            &fixtures,
            "BINPKG_COMPRESS_DEFINITELY_UNSET"
        ),
        None,
        "an unconfigured chain contributes no value"
    );
    assert_eq!(
        resolve_standalone_chain_scalar(&ebuild, &config_root, &fixtures, "BINPKG_FORMAT"),
        resolve_standalone_binpkg_format(&ebuild, &config_root, &fixtures),
        "the wrapper stays a thin alias for its key"
    );
}

/// The unmerge config chain resolves `BINPKG_FORMAT` for the
/// `FEATURES=unmerge-backup` quickpkg; see notes for the full contract.
#[test]
fn resolve_unmerge_config_reads_binpkg_format_from_make_conf() {
    let probe = TempDir::new("ebuild-phases-test-unmerge_binpkg_format").keep();
    std::fs::remove_dir_all(&probe).ok();
    let repo = probe.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = probe.join("etc/portage");
    std::fs::create_dir_all(&prof).unwrap();
    std::fs::create_dir_all(&portage_dir).unwrap();
    std::fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\n",
    )
    .unwrap();
    std::fs::write(portage_dir.join("make.conf"), "BINPKG_FORMAT=\"xpak\"\n").unwrap();
    std::fs::write(
        portage_dir.join("repos.conf"),
        "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();

    let config = resolve_unmerge_config(&probe, &probe).expect("scratch chain resolves");
    assert_eq!(
        config.other_vars.get("BINPKG_FORMAT").map(String::as_str),
        Some("xpak"),
        "the make.conf value must reach the chain the backup path reads"
    );
    // The file-side lookup shape (no calling env): the backup names
    // a `.tbz2` for this value.
    assert_eq!(
        crate::ebuild_package::resolve_binpkg_format(|key| config.other_vars.get(key).cloned()),
        "xpak"
    );
    // Only meaningful when the ambient env is silent on the key.
    if std::env::var_os("BINPKG_FORMAT").is_none() {
        assert_eq!(
            crate::ebuild_package::resolve_binpkg_format(|key| {
                portage_profile::env_over_config_scalar(&config, key)
            }),
            "xpak"
        );
    }

    // No value in the chain: the embedded `make.globals` default applies.
    std::fs::write(portage_dir.join("make.conf"), "# no BINPKG_FORMAT here\n").unwrap();
    let config = resolve_unmerge_config(&probe, &probe).expect("scratch chain resolves");
    assert_eq!(
        config.other_vars.get("BINPKG_FORMAT").map(String::as_str),
        Some("gpkg"),
        "an unconfigured chain yields the embedded make.globals default"
    );
    assert_eq!(
        crate::ebuild_package::resolve_binpkg_format(|key| config.other_vars.get(key).cloned()),
        "gpkg"
    );

    // No `repos.conf` at all: no chain, `None` (the caller keeps the
    // default) -- the same tolerance the standalone helpers have
    // outside any repo checkout.
    std::fs::remove_file(portage_dir.join("repos.conf")).unwrap();
    assert!(
        resolve_unmerge_config(&probe, &probe).is_none(),
        "without repos.conf there is no chain to read"
    );
    std::fs::remove_dir_all(&probe).ok();
}

/// E5: `md5_slot` reads the cache `SLOT` verbatim (`None` outside a repo).
#[test]
fn md5_slot_reads_the_cache_slot_verbatim() {
    let tmp = TempDir::new("ebuild-phases-test-s2-md5slot").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();
    let pkg_dir = repo.join("dev-libs/slotpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("slotpkg-2.1.ebuild"), "EAPI=8\nSLOT=\"0/1\"\n").unwrap();
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("slotpkg-2.1"),
        "EAPI=8\nIUSE=\nKEYWORDS=amd64\nSLOT=0/1\n\
         _md5_=0000000000000000000000000000000\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("pt");
    let env = compute_environment(&pkg_dir.join("slotpkg-2.1.ebuild"), &portage_tmpdir)
        .expect("synthetic ebuild parses");
    assert_eq!(md5_slot(&env).as_deref(), Some("0/1"));

    // Outside any repo checkout there is no cache to read.
    let lone = tmp.join("lone");
    std::fs::create_dir_all(lone.join("dev-libs/solo")).unwrap();
    std::fs::write(lone.join("dev-libs/solo/solo-1.0.ebuild"), "EAPI=8\n").unwrap();
    let lone_env =
        compute_environment(&lone.join("dev-libs/solo/solo-1.0.ebuild"), &portage_tmpdir)
            .expect("lone ebuild parses");
    assert_eq!(md5_slot(&lone_env), None);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// E3: `phase_replacing_versions` joins the installed same-main-slot
/// versions, sorted (`""` on a fresh root); other slots and other
/// packages never leak in.
#[test]
fn phase_replacing_versions_lists_the_installed_same_slot_versions() {
    let tmp = TempDir::new("ebuild-phases-test-s2-replacing").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let root = tmp.join("root");
    assert_eq!(
        phase_replacing_versions(&root, "dev-libs", "rvpkg", "0"),
        ""
    );
    let vdb = |pf: &str, slot: &str| {
        let dir = root.join(format!("var/db/pkg/dev-libs/{pf}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SLOT"), format!("{slot}\n")).unwrap();
    };
    vdb("rvpkg-2.0", "0");
    vdb("rvpkg-1.0", "0");
    vdb("rvpkg-9.9", "1");
    vdb("other-3.0", "0");
    assert_eq!(
        phase_replacing_versions(&root, "dev-libs", "rvpkg", "0"),
        "1.0 2.0"
    );
    assert_eq!(
        phase_replacing_versions(&root, "dev-libs", "rvpkg", "1"),
        "9.9"
    );
    // A full `slot/sub` query matches on the main slot.
    assert_eq!(
        phase_replacing_versions(&root, "dev-libs", "rvpkg", "0/1"),
        "1.0 2.0"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// P3: the REQUIRED_USE refusal text matches Portage's `_validate_deps`
/// byte for byte — reduced subset line, and the humanized complete
/// expression only when the reduction shrank anything. Reduced forms
/// re-checked live against Portage 3.0.82.2 (`check_required_use(...,
/// eapi='8').tounicode()`).
#[test]
fn required_use_message_matches_portage_validate_deps_text() {
    fn sets(
        enabled: &[&str],
        iuse: &[&str],
    ) -> (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ) {
        (
            enabled.iter().map(|s| s.to_string()).collect(),
            iuse.iter().map(|s| s.to_string()).collect(),
        )
    }
    // The S3 oracle cell: reduced == normalized, no complete block.
    let (enabled, iuse) = sets(&["t348dfoo"], &["t348dfoo", "t348dbar"]);
    assert_eq!(
        required_use_message("t348dfoo? ( t348dbar )", &enabled, &iuse),
        "\n  The following REQUIRED_USE flag constraints are unsatisfied:\n    t348dfoo? ( t348dbar )\n\n"
    );
    // A shrunk reduction appends the humanized complete expression.
    let (enabled, iuse) = sets(&["foo"], &["foo", "bar", "baz"]);
    assert_eq!(
        required_use_message("foo bar", &enabled, &iuse),
        "\n  The following REQUIRED_USE flag constraints are unsatisfied:\n    bar\n\
         \n  The above constraints are a subset of the following complete expression:\n    foo bar\n\n"
    );
    // Operators stay raw in the reduced line, humanized in the complete one.
    assert_eq!(
        required_use_message("foo ^^ ( bar baz )", &enabled, &iuse),
        "\n  The following REQUIRED_USE flag constraints are unsatisfied:\n    ^^ ( bar baz )\n\
         \n  The above constraints are a subset of the following complete expression:\n    foo exactly-one-of ( bar baz )\n\n"
    );
}

/// A hermetic standalone root for the #349 S4 config reads: one repo
/// (`profiles/repo_name`), one flag-less package with an md5-cache
/// entry, `make.profile` at the synthetic default, and a `make.conf`
/// carrying `DISTDIR`/`PORTAGE_TMPDIR`/`FEATURES`. Expected values from
/// Portage 3.0.82.2 (S0 cells `e01-full`, `e04-nofeatures`).
struct S4Root {
    tmp: std::path::PathBuf,
    cfg: std::path::PathBuf,
    ebuild: std::path::PathBuf,
}

fn s4_root(tag: &str, make_conf: &str) -> S4Root {
    let tmp = TempDir::new(&format!("ebuild-phases-test-s4-{tag}")).keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("repo");
    let prof = repo.join("profiles/default");
    let portage_dir = tmp.join("cfg/etc/portage");
    std::fs::create_dir_all(&prof).unwrap();
    std::fs::create_dir_all(&portage_dir).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "testrepo\n").unwrap();
    std::fs::write(
        prof.join("make.defaults"),
        "ARCH=\"amd64\"\nACCEPT_KEYWORDS=\"${ARCH}\"\nELIBC=\"glibc\"\n\
         USE_EXPAND=\"ELIBC\"\nUSE_EXPAND_IMPLICIT=\"ELIBC\"\n\
         USE_EXPAND_VALUES_ELIBC=\"glibc musl\"\n\
         IUSE_IMPLICIT=\"elibc_glibc elibc_musl\"\n\
         USE_EXPAND_UNPREFIXED=\"ARCH\"\n",
    )
    .unwrap();
    let pkg_dir = repo.join("dev-libs/s4envpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("s4envpkg-1.0.ebuild");
    std::fs::write(&ebuild, "EAPI=8\nSLOT=\"0\"\n").unwrap();
    let cache_dir = repo.join("metadata/md5-cache/dev-libs");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("s4envpkg-1.0"),
        "EAPI=8\nKEYWORDS=amd64\nSLOT=0\n\
         _md5_=0000000000000000000000000000000\n",
    )
    .unwrap();
    std::fs::write(
        portage_dir.join("repos.conf"),
        "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = repo\n",
    )
    .unwrap();
    // `repos.conf` locations resolve relative to the config root, so the
    // repo must also be visible there.
    let _ = std::fs::remove_dir_all(tmp.join("cfg/repo"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo, tmp.join("cfg/repo")).unwrap();
    std::fs::write(portage_dir.join("make.conf"), make_conf).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&prof, portage_dir.join("make.profile")).unwrap();
    S4Root {
        tmp: tmp.clone(),
        cfg: tmp.join("cfg"),
        ebuild,
    }
}

/// Save/restore a process-env key around a closure (process-global, so
/// every env-touching test does this).
fn with_env(key: &str, value: Option<&str>, f: impl FnOnce()) {
    let saved = std::env::var_os(key);
    unsafe {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
    f();
    unsafe {
        match saved {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}

/// `DISTDIR`/`PORTAGE_TMPDIR` resolve from `make.conf` when the calling
/// env is silent, and the env wins when set (S0 `e01`/`e05`).
#[test]
fn standalone_tmpdir_and_distdir_follow_the_config_with_env_on_top() {
    let root = s4_root(
        "dirs",
        "DISTDIR=\"/conf/dist\"\nPORTAGE_TMPDIR=\"/conf/tmp\"\nFEATURES=\"test\"\n",
    );
    let eroot = root.cfg.clone();
    let pkg_dir = root.ebuild.parent().unwrap().to_path_buf();
    with_env("DISTDIR", None, || {
        with_env("PORTAGE_TMPDIR", None, || {
            let config =
                resolve_standalone_config(&pkg_dir, &root.cfg, &eroot).expect("root resolves");
            assert_eq!(
                portage_repo::distdir_from_env_or_config(&config),
                std::path::PathBuf::from("/conf/dist")
            );
            assert_eq!(
                portage_repo::portage_tmpdir_from_env_or_config(&config),
                std::path::PathBuf::from("/conf/tmp")
            );
        });
    });
    with_env("DISTDIR", Some("/env/dist"), || {
        with_env("PORTAGE_TMPDIR", Some("/env/tmp"), || {
            let config =
                resolve_standalone_config(&pkg_dir, &root.cfg, &eroot).expect("root resolves");
            assert_eq!(
                portage_repo::distdir_from_env_or_config(&config),
                std::path::PathBuf::from("/env/dist")
            );
            assert_eq!(
                portage_repo::portage_tmpdir_from_env_or_config(&config),
                std::path::PathBuf::from("/env/tmp")
            );
        });
    });
    let _ = std::fs::remove_dir_all(&root.tmp);
}

/// The standalone base `USE` is the full effective set (a flag-less
/// package still sees `elibc_glibc`, S0 `e01`), and `ARCH` filters to
/// `""` (S0 `e01`).
#[test]
fn standalone_base_env_carries_portage_use_and_a_filtered_arch() {
    let root = s4_root("use", "FEATURES=\"test\"\n");
    let eroot = root.cfg.clone();
    let portage_tmpdir = root.tmp.join("pt");
    let env = compute_environment(&root.ebuild, &portage_tmpdir).expect("ebuild parses");
    with_env("USE", None, || {
        let (use_str, flags) = phase_standalone_base_env(&env, &root.cfg, &eroot, "setup", &[]);
        assert!(
            use_str.split_whitespace().any(|f| f == "elibc_glibc"),
            "full PORTAGE_USE, got {use_str:?}"
        );
        let arch = flags
            .iter()
            .find(|(k, _)| k == "ARCH")
            .map(|(_, v)| v.as_str());
        assert_eq!(arch, Some(""), "ARCH filters to empty, like Portage");
    });
    let _ = std::fs::remove_dir_all(&root.tmp);
}

/// End to end through `phase_env_vars` on the synthetic root: the
/// exported phase env carries the config tmpdir/builddir, the slashed
/// config root, the repo name, the full `USE` and the filtered `ARCH`
/// (S0 `e01-full` on both sides).
#[test]
fn phase_env_vars_exports_the_resolved_direct_path_shape() {
    let root = s4_root(
        "env",
        "DISTDIR=\"/conf/dist\"\nPORTAGE_TMPDIR=\"/conf/tmp\"\nFEATURES=\"test\"\n",
    );
    let eroot = root.cfg.clone();
    let portage_tmpdir = root.tmp.join("pt");
    let env = compute_environment(&root.ebuild, &portage_tmpdir).expect("ebuild parses");
    let bin_dir = root.tmp.join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");
    with_env("USE", None, || {
        let vars = phase_env_vars(PhaseEnvVarsArgs {
            env: &env,
            root: &eroot,
            ebuild_phase_value: "setup",
            debug: false,
            bin_dir: &bin_dir,
            helpers_dir: &helpers_dir,
            config_root: &root.cfg,
            extra_env: &[("DISTDIR".to_string(), "/conf/dist".to_string())],
        });
        let get = |key: &str| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(
            get("PORTAGE_TMPDIR"),
            Some(portage_tmpdir.to_str().unwrap())
        );
        assert_eq!(
            get("PORTAGE_CONFIGROOT"),
            Some(format!("{}/", root.cfg.display()).as_str())
        );
        assert_eq!(get("PORTAGE_REPO_NAME"), Some("testrepo"));
        let use_str = get("USE").unwrap_or("");
        assert!(
            use_str.split_whitespace().any(|f| f == "elibc_glibc"),
            "full PORTAGE_USE, got {use_str:?}"
        );
        assert_eq!(get("ARCH"), Some(""));
        let features = get("FEATURES").unwrap_or("");
        assert!(
            features.split_whitespace().any(|t| t == "test"),
            "resolved FEATURES, got {features:?}"
        );
        // The actual dir moves to `PORTAGE_ACTUAL_DISTDIR`; `DISTDIR`
        // becomes the per-package fake.
        assert_eq!(get("PORTAGE_ACTUAL_DISTDIR"), Some("/conf/dist"));
        assert_eq!(get("DISTDIR"), Some(env.fake_distdir().to_str().unwrap()));
    });
    let _ = std::fs::remove_dir_all(&root.tmp);
}

/// `bin/ebuild` drops the scheduler-only tokens for the direct path
/// (Portage: `bin/ebuild:305-309`); the `emerge` Scheduler gates on
/// `merge-wait`, so it keeps them. `noauto` is not dropped here (only
/// for `merge`, with the phase-loop semantics).
#[test]
fn discard_direct_ebuild_features_drops_the_scheduler_only_tokens() {
    assert_eq!(
        discard_direct_ebuild_features("sandbox merge-wait fail-clean test"),
        "sandbox test"
    );
    assert_eq!(discard_direct_ebuild_features("test"), "test");
    assert_eq!(discard_direct_ebuild_features(""), "");
    // `noauto` stays: dropping it belongs to the `merge` command path.
    assert_eq!(discard_direct_ebuild_features("noauto test"), "noauto test");
}

/// `PORTAGE_QUIET` is never set by the phase env itself: Portage sets it
/// only below the `--quiet` noise floor (`doebuild.py`, `noiselimit < 0`),
/// so the `__vecho` markers print on every other run. A quiet merge
/// threads `PORTAGE_QUIET=1` through the build env, which the last-wins
/// layering keeps.
#[test]
fn phase_env_vars_leaves_portage_quiet_unset_unless_the_run_is_quiet() {
    let root = s4_root("quiet", "FEATURES=\"test\"\n");
    let eroot = root.cfg.clone();
    let portage_tmpdir = root.tmp.join("pt");
    let env = compute_environment(&root.ebuild, &portage_tmpdir).expect("ebuild parses");
    let bin_dir = root.tmp.join("bin");
    let helpers_dir = bin_dir.join("ebuild-helpers");
    with_env("USE", None, || {
        let base = PhaseEnvVarsArgs {
            env: &env,
            root: &eroot,
            ebuild_phase_value: "setup",
            debug: false,
            bin_dir: &bin_dir,
            helpers_dir: &helpers_dir,
            config_root: &root.cfg,
            extra_env: &[],
        };
        let vars = phase_env_vars(base);
        assert!(
            vars.iter().all(|(k, _)| k != "PORTAGE_QUIET"),
            "a non-quiet run must not hush __vecho"
        );
        let vars = phase_env_vars(PhaseEnvVarsArgs {
            env: &env,
            root: &eroot,
            ebuild_phase_value: "setup",
            debug: false,
            bin_dir: &bin_dir,
            helpers_dir: &helpers_dir,
            config_root: &root.cfg,
            extra_env: &[("PORTAGE_QUIET".to_string(), "1".to_string())],
        });
        assert_eq!(
            vars.iter()
                .find(|(k, _)| k == "PORTAGE_QUIET")
                .map(|(_, v)| v.as_str()),
            Some("1"),
            "a quiet run threads PORTAGE_QUIET through"
        );
    });
    let _ = std::fs::remove_dir_all(&root.tmp);
}

/// Portage 3.0.82.2 `fetch.py` notice for a `RESTRICT=fetch` package, as
/// captured on stderr (`LLM/evidence/2026-10-19-b415/portage-cells.txt`).
#[test]
fn fetch_restriction_message_matches_portage_bytes() {
    assert_eq!(
        fetch_restriction_message("dev-util", "fetchrestrict-1"),
        "\n!!! dev-util/fetchrestrict-1 has fetch restriction turned on.\n\
         !!! This probably means that this ebuild's files must be downloaded\n\
         !!! manually.  See the comments in the ebuild for more information.\n\n"
    );
}
