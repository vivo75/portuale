use super::*;

#[test]
fn ensure_portage_logdir_symlink_is_a_noop_without_a_logdir() {
    let tmp = tempdir();
    let t_dir = tmp.join("t");
    fs::create_dir_all(&t_dir).unwrap();
    let log_path = t_dir.join("build.log");
    fs::write(&log_path, "already here").unwrap();
    ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path: &log_path,
        builddir: &tmp.join("builddir"),
        category: "dev-libs",
        pf: "foo-1.0",
        logdir: None,
        sep: ":",
        split_log: false,
        compress: false,
    });
    // Real file untouched -- no PORTAGE_LOGDIR means no symlink.
    assert_eq!(fs::read_to_string(&log_path).unwrap(), "already here");
    assert!(
        fs::symlink_metadata(&log_path)
            .unwrap()
            .file_type()
            .is_file()
    );
}

#[test]
fn ensure_portage_logdir_symlink_points_t_build_log_at_the_real_logdir() {
    let tmp = tempdir();
    let builddir = tmp.join("builddir");
    let t_dir = builddir.join("temp");
    fs::create_dir_all(&t_dir).unwrap();
    let log_path = t_dir.join("build.log");
    let logdir = tmp.join("logdir");

    ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path: &log_path,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        logdir: Some(&logdir),
        sep: ":",
        split_log: false,
        compress: false,
    });

    let target = fs::read_link(&log_path).expect("build.log must be a symlink");
    assert!(
        target.starts_with(&logdir),
        "{target:?} should live under {logdir:?}"
    );
    assert_eq!(target.parent().unwrap(), logdir);
    let name = target.file_name().unwrap().to_str().unwrap();
    // Real "<CATEGORY><sep><PF><sep><logid_time>.log".
    assert!(name.starts_with("dev-libs:foo-1.0:"), "{name}");
    assert!(name.ends_with(".log"), "{name}");
    // Writing through the symlink lands in the logdir file.
    std::fs::write(&log_path, b"hello").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "hello");

    // Idempotent: calling again doesn't churn the symlink or drop
    // the timestamp (the .logid marker file makes it stable).
    let before = fs::read_link(&log_path).unwrap();
    ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path: &log_path,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        logdir: Some(&logdir),
        sep: ":",
        split_log: false,
        compress: false,
    });
    assert_eq!(fs::read_link(&log_path).unwrap(), before);
}

#[test]
fn ensure_portage_logdir_symlink_honors_split_log_and_the_separator() {
    let tmp = tempdir();
    let builddir = tmp.join("builddir");
    let t_dir = builddir.join("temp");
    fs::create_dir_all(&t_dir).unwrap();
    let log_path = t_dir.join("build.log");
    let logdir = tmp.join("logdir");

    ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
        tmpdir_log_path: &log_path,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        logdir: Some(&logdir),
        sep: "-",
        split_log: true, // split_log
        compress: false,
    });

    let target = fs::read_link(&log_path).expect("build.log must be a symlink");
    // Real "<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log".
    assert_eq!(
        target.parent().unwrap(),
        logdir.join("build").join("dev-libs")
    );
    let name = target.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("foo-1.0-"), "{name}");
    assert!(!name.contains(':'), "{name} should use the '-' separator");
}

#[test]
fn ensure_portage_logdir_symlink_gains_the_gz_suffix_when_compressed() {
    // With `compress-build-logs` the log file is `<...>.log.gz` in both
    // the flat and `split-log` layouts.
    for split_log in [false, true] {
        let tmp = tempdir();
        let builddir = tmp.join("builddir");
        let t_dir = builddir.join("temp");
        fs::create_dir_all(&t_dir).unwrap();
        let log_path = t_dir.join("build.log.gz");
        let logdir = tmp.join("logdir");

        ensure_portage_logdir_symlink(EnsurePortageLogdirSymlinkArgs {
            tmpdir_log_path: &log_path,
            builddir: &builddir,
            category: "dev-libs",
            pf: "foo-1.0",
            logdir: Some(&logdir),
            sep: ":",
            split_log,
            compress: true,
        });

        let target = fs::read_link(&log_path).expect("build.log.gz must be a symlink");
        let name = target.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(".log.gz"), "{name}");
        if split_log {
            // `<logdir>/build/<CATEGORY>/<PF><sep><logid_time>.log.gz`.
            assert!(name.starts_with("foo-1.0:"), "{name}");
            assert_eq!(
                target.parent().unwrap(),
                logdir.join("build").join("dev-libs")
            );
        } else {
            // `<logdir>/<CATEGORY><sep><PF><sep><logid_time>.log.gz`.
            assert!(name.starts_with("dev-libs:foo-1.0:"), "{name}");
            assert_eq!(target.parent().unwrap(), logdir);
        }
    }
}

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

/// Without `PORT_LOGDIR` the foreground log is `${T}/build.log`, and a
/// stale symlink from an older logdir run is removed rather than
/// followed. Portage `prepare_build_dirs.py` (`PORTAGE_LOG_FILE`).
#[test]
fn foreground_log_path_is_t_build_log_without_a_logdir() {
    let tmp = tempdir();
    let builddir = tmp.join("builddir");
    let t_dir = builddir.join("temp");
    std::fs::create_dir_all(&t_dir).unwrap();
    // A leftover symlink must not divert the log outside the build dir.
    let elsewhere = tmp.join("elsewhere.log");
    std::fs::write(&elsewhere, "stale").unwrap();
    std::os::unix::fs::symlink(&elsewhere, t_dir.join("build.log")).unwrap();

    let resolved = foreground_log_path(ForegroundLogArgs {
        t_dir: &t_dir,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        features: "",
        logdir: None,
        sep: ":",
    });
    assert_eq!(resolved, t_dir.join("build.log"));
    assert!(
        !std::fs::symlink_metadata(t_dir.join("build.log")).is_ok_and(|m| m.is_symlink()),
        "stale symlink must be gone"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// With `PORT_LOGDIR` the foreground log is the real plog path under the
/// logdir (what Portage exports as `PORTAGE_LOG_FILE`), with
/// `${T}/build.log` left as a symlink to it. S0 cell `m07-port-logdir`.
#[test]
fn foreground_log_path_returns_the_real_plog_path_under_port_logdir() {
    let tmp = tempdir();
    let builddir = tmp.join("builddir");
    let t_dir = builddir.join("temp");
    std::fs::create_dir_all(&t_dir).unwrap();
    let logdir = tmp.join("plogs");

    let resolved = foreground_log_path(ForegroundLogArgs {
        t_dir: &t_dir,
        builddir: &builddir,
        category: "app-misc",
        pf: "t348-b-envnoeapply-7",
        features: "test",
        logdir: Some(&logdir),
        sep: ":",
    });
    assert!(
        resolved.starts_with(&logdir),
        "{resolved:?} should live under {logdir:?}"
    );
    let name = resolved.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("app-misc:t348-b-envnoeapply-7:"), "{name}");
    assert!(name.ends_with(".log"), "{name}");
    let link = std::fs::read_link(t_dir.join("build.log")).expect("symlink placed");
    assert_eq!(link, resolved);

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Under `compress-build-logs` the foreground log gains the `.gz`
/// suffix in both placements, like `build_log_path`.
#[test]
fn foreground_log_path_gains_the_gz_suffix_when_compressed() {
    let tmp = tempdir();
    let builddir = tmp.join("builddir");
    let t_dir = builddir.join("temp");
    std::fs::create_dir_all(&t_dir).unwrap();
    let logdir = tmp.join("plogs");

    let plain = foreground_log_path(ForegroundLogArgs {
        t_dir: &t_dir,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        features: "compress-build-logs",
        logdir: None,
        sep: ":",
    });
    assert_eq!(plain, t_dir.join("build.log.gz"));

    let under_dir = foreground_log_path(ForegroundLogArgs {
        t_dir: &t_dir,
        builddir: &builddir,
        category: "dev-libs",
        pf: "foo-1.0",
        features: "compress-build-logs",
        logdir: Some(&logdir),
        sep: ":",
    });
    assert!(
        under_dir.starts_with(&logdir),
        "{under_dir:?} should live under {logdir:?}"
    );
    assert!(
        under_dir
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with(".log.gz"),
        "{under_dir:?}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
