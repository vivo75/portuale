use super::*;

#[test]
fn capture_log_also_captures_pkg_preinst_and_pkg_postinst_output() {
    // `pkg_preinst`/`pkg_postinst` run separately from `install`, so they
    // need the log file threaded to them too; otherwise their output leaks
    // to the terminal despite `capture_log`. The fixture's hooks echo a
    // marker each for this test to find in the captured log.
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    let entries = vec![source_entry(
        "hookoutputpkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    )];
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        // The `${T}/build.log` this test reads is removed by the post-merge
        // clean; keep it via `FEATURES=noclean` (the clean itself is pinned
        // separately).
        features: "noclean".to_string(),
        ..ebuild_merge::MergeOptions::default()
    };
    run_source_merge(RunSourceMergeArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        jobs: 1,
        load_average: None,
        capture_log: true, // capture_log
        mode: StatusMode::for_tests(),
    })
    .expect("source merge succeeds");

    let log = build_log_path(&portage_tmpdir, "dev-libs", "hookoutputpkg", "1.0", "");
    let log_text = fs::read_to_string(&log).unwrap_or_else(|e| panic!("{}: {e}", log.display()));
    assert!(
        log_text.contains("HOOKOUTPUTPKG-PREINST-MARKER"),
        "pkg_preinst output missing from the captured log:\n{log_text}"
    );
    assert!(
        log_text.contains("HOOKOUTPUTPKG-POSTINST-MARKER"),
        "pkg_postinst output missing from the captured log:\n{log_text}"
    );

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}
