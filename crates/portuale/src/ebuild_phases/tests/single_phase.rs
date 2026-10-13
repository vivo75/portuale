use super::*;

/// `clean` removes a stale `.installed` marker, a populated image,
/// and `${T}`.
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

/// With `FEATURES=keepwork` `clean` still drops the stale
/// `.installed`/`image` but keeps `${T}` and `WORKDIR`.
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

/// Standalone `config`/`info` actually run the `pkg_config`/`pkg_info`
/// phase functions, with no `install` chain involved.
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

/// Standalone `preinst`/`postinst` actually run the `pkg_preinst`/
/// `pkg_postinst` phase functions with no dep chain, like Portage's
/// direct `_spawn_phase` for them (plan #348 S3 P2).
#[test]
fn run_single_phase_actually_runs_pkg_preinst_and_pkg_postinst() {
    let tmp = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "run_single_phase_actually_runs_pkg_preinst_and_pkg_postinst"
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    // A scratch ebuild (no repo checkout, no md5-cache): the standalone
    // REQUIRED_USE gate has nothing to resolve and stays out of the way.
    let pkg_dir = tmp.join("pkg/dev-libs/iipkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild_path = pkg_dir.join("iipkg-1.0.ebuild");
    std::fs::write(
        &ebuild_path,
        "EAPI=8\nSLOT=\"0\"\n\
         pkg_preinst() { touch \"${T}\"/pkg-preinst-ran || die; }\n\
         pkg_postinst() { touch \"${T}\"/pkg-postinst-ran || die; }\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("tmp");

    let preinst_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "preinst",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(preinst_status, 0);
    let postinst_status = run_single_phase(RunSinglePhaseArgs {
        ebuild_path: &ebuild_path,
        phase: "postinst",
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
        log_file: None,
    })
    .expect("run_single_phase should not itself error");
    assert_eq!(postinst_status, 0);

    let t_dir = portage_tmpdir.join("portage/dev-libs/iipkg-1.0/temp");
    assert!(
        t_dir.join("pkg-preinst-ran").is_file(),
        "pkg_preinst must actually run"
    );
    assert!(
        t_dir.join("pkg-postinst-ran").is_file(),
        "pkg_postinst must actually run"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Standalone `prerm`/`postrm` actually run the `pkg_prerm`/`pkg_postrm`
/// phase functions, with no `unmerge`/vdb step involved.
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
