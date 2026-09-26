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

    // No value anywhere in the chain: nothing resolves, so Portage's
    // `make.globals` default (`gpkg`) applies.
    std::fs::write(portage_dir.join("make.conf"), "# no BINPKG_FORMAT here\n").unwrap();
    let config = resolve_unmerge_config(&probe, &probe).expect("scratch chain resolves");
    assert!(
        !config.other_vars.contains_key("BINPKG_FORMAT"),
        "an unconfigured chain contributes no value"
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
