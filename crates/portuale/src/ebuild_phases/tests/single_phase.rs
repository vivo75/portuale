use super::*;

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
