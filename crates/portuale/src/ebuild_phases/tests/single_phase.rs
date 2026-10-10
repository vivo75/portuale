use super::*;

/// Backlog #42: the real `clean` phase (`bin/ebuild.sh clean` ->
/// `__dyn_clean`) is what real `_emerge/EbuildBuild._start_pre_clean`
/// runs before every build. Given a stale `.installed` marker and a
/// populated `image/` + `${T}`, it must remove all three -- exactly
/// the state that made a rebuild silently skip `install` and
/// repackage an already-`instprep`ped image (#38 S4).
#[test]
fn run_clean_removes_a_stale_installed_marker_image_and_temp() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-run-clean").keep();
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let builddir = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0");
    std::fs::create_dir_all(builddir.join("image/usr/share/phasepkg")).unwrap();
    std::fs::write(
        builddir.join("image/usr/share/phasepkg/hello.txt"),
        "stale\n",
    )
    .unwrap();
    std::fs::write(builddir.join(".installed"), []).unwrap();
    std::fs::create_dir_all(builddir.join("temp")).unwrap();
    std::fs::write(builddir.join("temp/build.log"), "stale log\n").unwrap();

    let status = run_clean(RunCleanArgs {
        ebuild_path: &ebuild_path,
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        build_env: &[],
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Bash,
        log_file: None,
    })
    .expect("run_clean should not itself error");
    assert_eq!(status, 0, "clean should exit successfully");
    assert!(
        !builddir.join(".installed").exists(),
        "a stale .installed marker must be removed"
    );
    assert!(
        !builddir.join("image").exists(),
        "a stale image must be removed"
    );
    assert!(
        !builddir.join("temp").exists(),
        "T must be removed without keeptemp/keepwork"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `FEATURES=keepwork` (real `__dyn_clean`'s own guard): `clean`
/// still drops the stale `.installed`/`image`, but keeps `${T}` and
/// `WORKDIR` -- the documented real way to inspect a build.
#[test]
fn run_clean_with_keepwork_keeps_temp_and_workdir_but_still_drops_the_marker() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-run-clean-keepwork").keep();
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let builddir = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0");
    std::fs::create_dir_all(builddir.join("image/usr/share/phasepkg")).unwrap();
    std::fs::create_dir_all(builddir.join("temp")).unwrap();
    std::fs::write(builddir.join("temp/build.log"), "kept\n").unwrap();
    std::fs::write(builddir.join(".installed"), []).unwrap();

    let keepwork = vec![("FEATURES".to_string(), "keepwork".to_string())];
    let status = run_clean(RunCleanArgs {
        ebuild_path: &ebuild_path,
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        build_env: &keepwork,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Bash,
        log_file: None,
    })
    .expect("run_clean should not itself error");
    assert_eq!(status, 0, "clean should exit successfully");
    assert!(!builddir.join(".installed").exists());
    assert!(!builddir.join("image").exists());
    assert!(
        builddir.join("temp/build.log").is_file(),
        "keepwork must keep T"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Real, end-to-end proof that standalone `config`/`info` (real
/// `ebuild.rs`'s own routing to `run_single_phase`, see
/// `is_standalone_phase_command`'s own doc comment) actually
/// runs the real `pkg_config`/`pkg_info` phase functions -- not just
/// that `run_single_phase` returns successfully. No `install` chain
/// involved at all, matching real standalone usage (a real admin
/// runs `ebuild <file> config` directly against an ebuild, with no
/// merge/vdb step in the same invocation).
#[test]
fn run_single_phase_actually_runs_pkg_config_and_pkg_info() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "run_single_phase_actually_runs_pkg_config_and_pkg_info"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let config_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "config",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(config_status, 0);
    let info_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "info",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(info_status, 0);

    let t_dir = portage_tmpdir.join("portage/dev-libs/standalonephasepkg-1.0/temp");
    assert!(
        t_dir.join("pkg-config-ran").is_file(),
        "pkg_config must actually run"
    );
    assert!(
        t_dir.join("pkg-info-ran").is_file(),
        "pkg_info must actually run"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Real, end-to-end proof that standalone `prerm`/`postrm` actually
/// run the real `pkg_prerm`/`pkg_postrm` phase functions -- same
/// shape as `run_single_phase_actually_runs_pkg_config_and_pkg_info`
/// above, but for the two standalone commands that also have a real,
/// separate internal use (`ebuild_unmerge::run_unmerge`, see
/// `is_standalone_phase_command`'s own doc comment for why
/// that internal use and this new standalone path are simply two
/// independent ways to reach the same real phase function). No
/// `unmerge`/vdb step involved at all here, matching real standalone
/// usage.
#[test]
fn run_single_phase_actually_runs_pkg_prerm_and_pkg_postrm() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/standalonephasepkg/standalonephasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "run_single_phase_actually_runs_pkg_prerm_and_pkg_postrm"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let prerm_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "prerm",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(prerm_status, 0);
    let postrm_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "postrm",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(postrm_status, 0);

    let t_dir = portage_tmpdir.join("portage/dev-libs/standalonephasepkg-1.0/temp");
    assert!(
        t_dir.join("pkg-prerm-ran").is_file(),
        "pkg_prerm must actually run"
    );
    assert!(
        t_dir.join("pkg-postrm-ran").is_file(),
        "pkg_postrm must actually run"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}
