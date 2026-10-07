use super::*;

#[test]
fn remove_bin_overlay_dir_is_idempotent() {
    // Double exit paths must never fail: remove, then tolerate missing.
    let dir = TempDir::new("portuale-bin-overlay-test").keep();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("f"), "x").unwrap();
    remove_bin_overlay_dir(&dir);
    assert!(!dir.exists());
    remove_bin_overlay_dir(&dir);
    assert!(!dir.exists());
}

/// A set override wins over the build tree; a wrong override errors
/// naming the variable and never falls through; an empty one is unset.
#[test]
fn resolve_bin_dir_override_beats_the_build_tree_and_never_falls_through() {
    let tmp = TempDir::new("resolve-bin-dir-order");
    let tree = tmp.join("tree");
    let tree_bin = fake_runtime(&tree);
    let other = fake_runtime(&tmp.join("other"));
    let none = tmp.join("none");

    for embedded in [false, true] {
        assert_eq!(
            resolve_bin_dir(Some(other.to_str().unwrap()), &tree, embedded).unwrap(),
            BinSource::Dir(other.clone())
        );
        assert_eq!(
            resolve_bin_dir(None, &tree, embedded).unwrap(),
            BinSource::Dir(tree_bin.clone())
        );
        assert_eq!(
            resolve_bin_dir(Some(""), &tree, embedded).unwrap(),
            BinSource::Dir(tree_bin.clone())
        );
    }

    let err = resolve_bin_dir(Some(none.to_str().unwrap()), &tree, true).unwrap_err();
    assert!(
        err.contains(BIN_DIR_VAR) && err.contains("ebuild.sh"),
        "{err}"
    );
    assert!(err.contains(none.to_str().unwrap()), "{err}");
    std::fs::create_dir_all(tmp.join("empty")).unwrap();
    assert!(resolve_bin_dir(Some(tmp.join("empty").to_str().unwrap()), &tree, true).is_err());
}

/// With neither source present the error carries what was sought,
/// where it looked, and how to fix it, with no program prefix.
#[test]
fn resolve_bin_dir_with_nothing_found_explains_itself() {
    let tmp = TempDir::new("resolve-bin-dir-missing");
    let gone = tmp.join("gone");
    let err = resolve_bin_dir(None, &gone, false).unwrap_err();
    assert!(err.contains("bin/ebuild.sh"), "{err}");
    assert!(err.contains(gone.to_str().unwrap()), "{err}");
    assert!(err.contains(BIN_DIR_VAR), "{err}");
    assert!(
        !err.starts_with("portuale:") && !err.starts_with("emerge:"),
        "{err}"
    );
    assert!(!err.contains('\n'), "one line: {err}");
}

/// With no override and no build tree the embedded copy is the
/// answer; a build tree that has the runtime still wins over it.
#[test]
fn resolve_bin_dir_falls_back_to_the_embedded_copy_only_when_nothing_is_on_disk() {
    let tmp = TempDir::new("resolve-bin-dir-embedded");
    let gone = tmp.join("gone");
    assert_eq!(
        resolve_bin_dir(None, &gone, true).unwrap(),
        BinSource::Embedded
    );
    assert_eq!(
        resolve_bin_dir(Some(""), &gone, true).unwrap(),
        BinSource::Embedded
    );
    let tree_bin = fake_runtime(&tmp.join("tree"));
    assert_eq!(
        resolve_bin_dir(None, &tmp.join("tree"), true).unwrap(),
        BinSource::Dir(tree_bin)
    );
}

/// The scratch base skips noexec directories; with nothing usable
/// the first candidate is returned so the caller's error names it.
#[test]
fn the_scratch_base_skips_noexec_directories() {
    let c = [
        PathBuf::from("/a"),
        PathBuf::from("/b"),
        PathBuf::from("/c"),
    ];
    assert_eq!(
        pick_scratch_base(&c, |d| d != Path::new("/a")),
        PathBuf::from("/b")
    );
    assert_eq!(pick_scratch_base(&c, |_| true), PathBuf::from("/a"));
    assert_eq!(pick_scratch_base(&c, |_| false), PathBuf::from("/a"));
    // A directory that cannot be inspected counts as unusable.
    assert!(is_noexec(Path::new("/nonexistent-dir-xyz")));
}

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
