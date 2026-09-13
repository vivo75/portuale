use super::*;

/// `extra_env` `FEATURES` is exported last so it wins over the
/// process-env base.
#[test]
fn phase_setup_script_exports_extra_env_features_last() {
    let ebuild = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phaseenvpkg/phaseenvpkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-setup-script").keep();
    let env = compute_environment(&ebuild, &portage_tmpdir).unwrap();
    let script = phase_setup_script(PhaseSetupScriptArgs {
        env: &env,
        root: Path::new("/"),
        ebuild_phase_value: "install",
        debug: false,
        bin_dir: bin_dir().unwrap(),
        helpers_dir: &bin_dir().unwrap().join("ebuild-helpers"),
        config_root: Path::new("/dev/null/no-config-root"),
        extra_env: &[("FEATURES".to_string(), "resolved one".to_string())],
    });
    let features: Vec<&str> = script
        .lines()
        .filter(|l| l.starts_with("export FEATURES="))
        .collect();
    assert!(!features.is_empty(), "no FEATURES export in script");
    assert_eq!(
        *features.last().unwrap(),
        "export FEATURES='resolved one'",
        "the extra_env FEATURES must be exported last to win"
    );
}

#[test]
fn repo_root_for_finds_the_nearest_ancestor_repo_root() {
    let tmp = TempDir::new("ebuild-phases-test-repo_root_for_finds").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("myrepo");
    let pkg_dir = repo.join("dev-libs/foo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
    assert_eq!(repo_root_for(&pkg_dir), Some(repo));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn repo_root_for_is_none_when_no_ancestor_has_one() {
    let tmp = TempDir::new("ebuild-phases-test-repo_root_for_none").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("dev-libs/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(repo_root_for(&pkg_dir), None);
    let _ = std::fs::remove_dir_all(&tmp);
}
