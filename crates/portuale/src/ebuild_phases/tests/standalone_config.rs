use super::*;

#[test]
fn restrict_mirror_from_restrict_evaluates_conditionals_against_the_empty_use_set() {
    assert!(restrict_mirror_from_restrict("mirror"));
    assert!(restrict_mirror_from_restrict("fetch mirror"));
    // deprecated negative spelling still counts (real fetch.py:880)
    assert!(restrict_mirror_from_restrict("nomirror"));
    // no mirror restriction
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
    // A conditional `RESTRICT` drops its gated tokens on the empty
    // set but keeps them on the config `USE` set -- the `depend`
    // phase's own reduction (real `doebuild(mydo="depend")` runs
    // with the `setcpv` config `USE`, so ebuild.sh's own
    // `contains_word … "${PORTAGE_RESTRICT}"` checks, e.g. the
    // `strip`/`nostrip` `DEBUGBUILD` gate, see the configured
    // tokens). No fixture change: a throwaway repo carries the
    // conditional entry, while the config `USE` comes from the
    // fixture tree (`make.conf` sets `confflag`).
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
    // The fixture config's own `USE` carries `confflag` (its
    // `make.conf`), so the `depend` reduction keeps `strip` while
    // still dropping the `other?` group.
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
    // Real `config.py::_flatten` (`:1681-1688`):
    // `" ".join(sorted(set(use_reduce(..., flat=True))))` -- the
    // #45 ncurses case: `RESTRICT="!test? ( test ) test"` reduces
    // to two `test` tokens with `test` off, and the set collapses
    // them (real's saved value is `"test"`).
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

/// #37 S2 step 6: the `depend` phase (real `doebuild(mydo="depend")`,
/// the `--regen`/metadata path) never gets the standalone base env --
/// `phase_standalone_base_env` short-circuits before any config load,
/// so S2's resolved-config threading cannot leak into it. A real repo
/// checkout with a resolvable package is used precisely so removing
/// the guard would fail this test rather than silently pass.
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

/// Real per-package `PORTAGE_TMPDIR` (backlog #99, S0 cells D/E):
/// the matched env-file value re-derives the tmpdir when the calling
/// environment does not carry the key itself; a matched directory
/// that does not exist is real's `_check_temp_dir` failure byte for
/// byte (`doebuild.py:1682`); the process value wins otherwise (#101
/// precedence); an empty matched value blanks to absent.
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
    // No entry at all: the run tmpdir stands.
    let out =
        resolve_entry_portage_tmpdir(&[], cpv, run, None).expect("no entry keeps the run tmpdir");
    assert_eq!(out, run);
    // Matched, directory exists: re-derived.
    let probe = TempDir::new("ebuild-phases-test-per_package_portage_tmpdir").keep();
    let out = resolve_entry_portage_tmpdir(
        &files(probe.to_str().expect("probe path is UTF-8")),
        cpv,
        run,
        None,
    )
    .expect("matched dir resolves");
    assert_eq!(out, probe);
    // A later matching file wins (scalar semantics).
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
    // Matched, directory missing: real's `_check_temp_dir` failure.
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
    // The calling environment wins over the env file (#101).
    let out = resolve_entry_portage_tmpdir(
        &files(probe.to_str().expect("probe path is UTF-8")),
        cpv,
        run,
        Some(run),
    )
    .expect("process tmpdir wins");
    assert_eq!(out, run);
    // Empty matched value blanks to absent: the run tmpdir stands.
    let out = resolve_entry_portage_tmpdir(&files(""), cpv, run, None)
        .expect("empty match keeps the run tmpdir");
    assert_eq!(out, run);
    std::fs::remove_dir_all(&probe).ok();
}

/// Backlog #147 S3: the standalone compressor resolver reads the
/// twin fixture's committed `package.env` match
/// (`dev-libs/penvcmppkg penv-compress`, `BINPKG_COMPRESS=gzip`)
/// through the ebuild's own md5-cache identity -- `None` for an
/// unmatched neighbour (`dev-libs/newpkg`).
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

/// Backlog #180: the standalone chain-scalar reader serves every
/// global binpkg scalar (`PKGDIR`, `BINPKG_COMPRESS*`,
/// `PORTAGE_BZIP2_COMMAND`, `BINPKG_FORMAT`) below the calling env.
/// The fixture `make.conf` sets
/// `PKGDIR="${PORTAGE_CONFIGROOT}/pkgdir"`, so the file side must
/// resolve to the fixture's own pkgdir; an unconfigured key
/// resolves to `None` (the caller keeps real `make.globals`'s own
/// default); and `BINPKG_FORMAT` agrees with the dedicated
/// `resolve_standalone_binpkg_format` wrapper by construction.
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

/// Backlog #173 review: `execute_unmerge`'s `FEATURES=unmerge-backup`
/// quickpkg resolves `BINPKG_FORMAT` through this chain (calling env
/// over `make.conf`/profile/`make.globals`, real `config`
/// precedence), so a `make.conf` value reaches the backup artefact
/// (a `.tbz2` for `xpak`). A full quickpkg run is too heavy for a
/// unit test; this pins the resolver the backup path uses instead --
/// the scratch chain value plus the production lookup shape over it.
/// The calling-env-beats-file half lives in portage-profile's own
/// `with_test_env` test (that hook is `cfg(test)`-local to that
/// crate); here only the file side is asserted, plus the
/// env-over-chain production expression when the ambient process
/// env is itself silent (otherwise the ambient value would rightly
/// win and the assertion would be testing the developer's shell).
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
    // The full production expression (calling env over the chain):
    // only meaningful when the ambient env is silent on the key.
    if std::env::var_os("BINPKG_FORMAT").is_none() {
        assert_eq!(
            crate::ebuild_package::resolve_binpkg_format(|key| {
                portage_profile::env_over_config_scalar(&config, key)
            }),
            "xpak"
        );
    }

    // No value anywhere in the chain: nothing resolves, so the real
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
    // outside any real repo checkout.
    std::fs::remove_file(portage_dir.join("repos.conf")).unwrap();
    assert!(
        resolve_unmerge_config(&probe, &probe).is_none(),
        "without repos.conf there is no chain to read"
    );
    std::fs::remove_dir_all(&probe).ok();
}
