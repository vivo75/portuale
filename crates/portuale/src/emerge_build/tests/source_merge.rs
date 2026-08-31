use super::*;

/// `merge_one_source_entry` pre-cleans before every build: with a stale
/// `.installed` marker and a tampered `${D}` left by an earlier build (kept via
/// `noclean`), the next run must discard them and rebuild. Expected value from
/// Portage source.
#[test]
fn source_merge_pre_cleans_a_stale_installed_image() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let entry = source_entry(
        "packagepkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );

    // First merge with noclean: the build state survives.
    let options = ebuild_merge::MergeOptions {
        features: "noclean".to_string(),
        ..ebuild_merge::MergeOptions::default()
    };
    merge_one_source_entry(MergeOneSourceEntryArgs {
        entry: &entry,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        buildpkg: None,
        progress: mrg_director::MergeProgress::single(),
        display: &StatusDisplay::for_tests(),
    })
    .expect("first merge succeeds");
    let builddir = portage_tmpdir.join("portage/dev-libs/packagepkg-1.0");
    assert!(
        builddir.join(".installed").exists(),
        "noclean must keep the first build's state"
    );

    // Tamper: replace the image file with a stale one, keeping the
    // `.installed` marker -- the state the missing pre-clean treated as
    // "already built".
    let image = builddir.join("image/usr/share/packagepkg");
    std::fs::remove_file(image.join("hello.txt")).unwrap();
    std::fs::write(image.join("stale.txt"), "stale\n").unwrap();

    // Second merge, no noclean: the pre-clean drops the stale state, so
    // `install` really re-runs.
    let options = ebuild_merge::MergeOptions {
        features: "sandbox".to_string(),
        ..ebuild_merge::MergeOptions::default()
    };
    merge_one_source_entry(MergeOneSourceEntryArgs {
        entry: &entry,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        buildpkg: None,
        progress: mrg_director::MergeProgress::single(),
        display: &StatusDisplay::for_tests(),
    })
    .expect("second merge succeeds");
    assert!(root.join("usr/share/packagepkg/hello.txt").is_file());
    assert!(
        !root.join("usr/share/packagepkg/stale.txt").exists(),
        "the stale image must not be merged"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}
