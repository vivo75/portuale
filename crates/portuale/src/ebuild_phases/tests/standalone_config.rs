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
