use super::*;

#[test]
fn remove_bin_overlay_dir_is_idempotent() {
    // Backlog #88: the `atexit` handler's contract -- removes a
    // populated overlay, then silently tolerates the missing dir
    // (double exit paths, or an overlay a previous run already
    // reclaimed, must never fail).
    let dir = TempDir::new("portuale-bin-overlay-test").keep();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("f"), "x").unwrap();
    remove_bin_overlay_dir(&dir);
    assert!(!dir.exists());
    remove_bin_overlay_dir(&dir);
    assert!(!dir.exists());
}

/// Backlog #322, `resolve_bin_dir`'s order: a set override wins over
/// the build tree; an override that is wrong is an error naming the
/// variable and never falls through; an empty one is unset.
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

    // A wrong override is refused even though the build tree is fine,
    // and even though an embedded copy could stand in.
    let err = resolve_bin_dir(Some(none.to_str().unwrap()), &tree, true).unwrap_err();
    assert!(
        err.contains(BIN_DIR_VAR) && err.contains("ebuild.sh"),
        "{err}"
    );
    assert!(err.contains(none.to_str().unwrap()), "{err}");
    // A directory without ebuild.sh is wrong too.
    std::fs::create_dir_all(tmp.join("empty")).unwrap();
    assert!(resolve_bin_dir(Some(tmp.join("empty").to_str().unwrap()), &tree, true).is_err());
}

/// Backlog #322: neither source present is a message with the three
/// facts the user needs (what, where it looked, how to fix it) and no
/// program prefix (callers add their own).
#[test]
fn resolve_bin_dir_with_nothing_found_explains_itself() {
    let tmp = TempDir::new("resolve-bin-dir-missing");
    let gone = tmp.join("gone");
    // No embedded copy: nothing on disk is an error that explains itself.
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

/// Backlog #322 S2.3: with no override and no build tree, the embedded
/// copy is the answer (never an error); a build tree that has the
/// runtime still wins over it.
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

/// Backlog #322, end to end: a run that needs the phase runtime but
/// is pointed at a directory without one exits 1 with one message --
/// no panic, no abort (the release profile aborts on panic, which is
/// what produced the core dump) -- and `mrg --remote-binpkg` does so
/// before touching the client (here the local-transport ROOT).
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
    // Nothing usable: the first candidate, so the caller's error names it.
    assert_eq!(pick_scratch_base(&c, |_| false), PathBuf::from("/a"));
    // The real probe treats a directory it cannot inspect as unusable.
    assert!(is_noexec(Path::new("/nonexistent-dir-xyz")));
}

/// Backlog #326 S1: the `PORTAGE_PYM_PATH` / `PORTAGE_PYTHONPATH`
/// decision factored out of `phase_env_vars` / `invoke_dyn_package` --
/// a checkout whose `lib/portage` is a dir yields that `lib`, anything
/// else yields the `/` fallback (resp. no `PORTAGE_PYTHONPATH`). No
/// process-env mutation: the checkout path is passed in directly.
#[test]
fn pym_path_and_pythonpath_values_follow_the_checkout() {
    let tmp = TempDir::new("ebuild-phases-test-pym-path-values");
    // A checkout with `lib/portage/` -> both return the lib path.
    let with = tmp.join("with");
    std::fs::create_dir_all(with.join("lib/portage")).unwrap();
    let lib = with.join("lib").display().to_string();
    assert_eq!(pym_path_value(&with), lib);
    assert_eq!(pythonpath_value(&with), Some(lib));
    // A temp dir without it -> "/" and None.
    let without = tmp.join("without");
    std::fs::create_dir_all(&without).unwrap();
    assert_eq!(pym_path_value(&without), "/");
    assert_eq!(pythonpath_value(&without), None);
    // A dir with `lib/` but no `lib/portage` -> "/" and None.
    let bare_lib = tmp.join("bare-lib");
    std::fs::create_dir_all(bare_lib.join("lib")).unwrap();
    assert_eq!(pym_path_value(&bare_lib), "/");
    assert_eq!(pythonpath_value(&bare_lib), None);
    // A nonexistent path -> "/" and None.
    let gone = tmp.join("gone");
    assert!(!gone.exists());
    assert_eq!(pym_path_value(&gone), "/");
    assert_eq!(pythonpath_value(&gone), None);
}

/// #37 S2 step 2: `extra_env` carries the resolved `FEATURES` and is
/// appended last, so it must win over `phase_env_vars`' own
/// `phase_features_value()` base (the raw process env). Verified as
/// brush *script composition* here; the real-phase execution proof
/// is `emerge_build::tests::source_merge_with_resolved_config_
/// threads_use_and_slot_into_the_phase` (which asserts the
/// `FEATURES=` marker under both backends).
#[test]
fn phase_setup_script_exports_extra_env_features_last() {
    let ebuild = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phaseenvpkg/phaseenvpkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-setup-script").keep();
    let env = compute_environment(&ebuild, &portage_tmpdir).unwrap();
    let script = phase_setup_script(
        &env,
        Path::new("/"),
        "install",
        false,
        bin_dir().unwrap(),
        &bin_dir().unwrap().join("ebuild-helpers"),
        Path::new("/dev/null/no-config-root"),
        &[("FEATURES".to_string(), "resolved one".to_string())],
    );
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
