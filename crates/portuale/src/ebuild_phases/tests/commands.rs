use super::*;

/// End-to-end proof: a real `install` run against the fixture ebuild
/// lands a file under `${D}` via the unmodified `bin/*.sh`, not a mock.
/// A fresh `PORTAGE_TMPDIR` per run (passed directly, never via the
/// environment) keeps this safe alongside parallel tests.
#[test]
fn install_lands_a_real_file_under_a_real_d() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_lands_a_real_file_under_a_real_d"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let installed =
        portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt");
    let contents = std::fs::read_to_string(&installed)
        .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
    assert_eq!(contents, "hello from phasepkg\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// An in-process `Brush` phase must not leak its umask into the host
/// process; the body runs in a re-executed child (`PORTUALE_UMASK_CHILD`).
#[test]
fn a_brush_phase_does_not_leak_its_umask_into_the_host_process() {
    if std::env::var_os("PORTUALE_UMASK_CHILD").is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ebuild_phases::tests::a_brush_phase_does_not_leak_its_umask_into_the_host_process",
                "--nocapture",
            ])
            .env("PORTUALE_UMASK_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "child failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!("umask-leak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    // SAFETY: `umask(2)` cannot fail; this is the single-test child.
    unsafe { libc::umask(0o027) };
    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0);
    let after = unsafe { libc::umask(0o027) };
    assert_eq!(after, 0o027, "the phase's umask leaked: {after:04o}");
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The install chain writes `build-info/BUILD_TIME` before any metadata
/// key, for the merge to copy (Portage: doebuild.py, _post_src_install_write_metadata).
#[test]
fn install_writes_the_real_build_time_file() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_writes_the_real_build_time_file"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let build_time = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/build-info/BUILD_TIME");
    let text = std::fs::read_to_string(&build_time)
        .unwrap_or_else(|e| panic!("{} should exist: {e}", build_time.display()));
    assert!(
        text.trim().parse::<u64>().is_ok(),
        "BUILD_TIME must be an epoch-seconds integer: {text:?}"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `ShellBackend::Bash` counterpart of the brush proof above: same
/// fixture, same assertions, identical result.
#[test]
fn install_lands_a_real_file_under_a_real_d_via_real_bash() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_lands_a_real_file_under_a_real_d_via_real_bash"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Bash,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let installed =
        portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/image/usr/share/phasepkg/hello.txt");
    let contents = std::fs::read_to_string(&installed)
        .unwrap_or_else(|e| panic!("{} should have been installed: {e}", installed.display()));
    assert_eq!(contents, "hello from phasepkg\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// End-to-end proof of `fetch_sources`' `A`/`AA` through the CLI path,
/// offline: a pre-seeded verified distfile takes the skip-fetch path.
/// `A` excludes the `test?` group; `AA` keeps every file regardless of USE.
#[test]
fn install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/verifiedfetchpkg/verifiedfetchpkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset"
    ));
    let distdir = TempDir::new(&format!(
        "ebuild-phases-test-distdir-{}-{}",
        std::process::id(),
        "install_computes_real_a_from_a_verified_distfile_and_leaves_aa_unset"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&distdir);
    std::fs::create_dir_all(&distdir).unwrap();
    std::fs::write(
        distdir.join("verifiedfetchpkg-1.0.tar.gz"),
        b"hello from verifiedfetchpkg\n",
    )
    .unwrap();

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &distdir,
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let marker = portage_tmpdir.join("portage/dev-libs/verifiedfetchpkg-1.0/temp/fetch-vars.txt");
    let observed = std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
    // `AA` is unset: Portage's `config.environ()` pops it for EAPI >= 4,
    // and the floor here is 5+.
    // The ebuild's own `echo "AA=${AA}"` therefore writes an empty
    // value, matching an EAPI-8 phase.
    assert_eq!(observed, "A=verifiedfetchpkg-1.0.tar.gz\nAA=\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&distdir);
}

/// End-to-end proof of `eclass_locations_value`: the fixture ebuild
/// inherits `pilotcheck.eclass` via `bin/ebuild.sh`'s `inherit()` and
/// calls a function it defines.
#[test]
fn install_really_inherits_a_real_eclass_and_calls_its_own_function() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/eclasspkg/eclasspkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_really_inherits_a_real_eclass_and_calls_its_own_function"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let marker = portage_tmpdir.join("portage/dev-libs/eclasspkg-1.0/temp/eclass-marker.txt");
    let observed = std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
    assert_eq!(observed, "hello from pilotcheck.eclass\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The saved `${T}/environment` carries no bash special variables, so
/// re-sourcing it prints no `cannot mutate readonly variable`.
#[test]
fn brush_phase_env_is_filtered_of_bash_special_variables() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "brush_phase_env_is_filtered_of_bash_special_variables"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let log = portage_tmpdir.join("brush.log");

    let status = run_commands_logged(RunCommandsLoggedArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        log_file: Some(&log),
        build_env: &[],
    })
    .expect("run_commands_logged should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let environment = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/temp/environment");
    let saved = std::fs::read_to_string(&environment)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", environment.display()));
    for name in ["BASHOPTS", "EUID", "PPID", "SHELLOPTS", "UID"] {
        assert!(
            !saved
                .lines()
                .any(|line| line.starts_with("declare") && line.contains(&format!(" {name}="))),
            "{name} must be filtered out of the saved environment"
        );
    }

    let logged = std::fs::read_to_string(&log)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", log.display()));
    assert!(
        !logged.contains("cannot mutate readonly variable"),
        "the phase log must not contain readonly-variable noise:\n{logged}"
    );
    assert!(
        !logged.contains("env: ''"),
        "the phase log must not contain an empty-$BASH `env` failure:\n{logged}"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Passing `debug: true` exports `PORTAGE_DEBUG=1` into the phase's
/// environment, proven via the value the fixture observed.
#[test]
fn debug_flag_exports_portage_debug() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/debugpkg/debugpkg-1.0.ebuild");

    for (debug, expected) in [(true, "1"), (false, "0")] {
        let portage_tmpdir = TempDir::new(&format!(
            "ebuild-phases-test-{}-{}-{debug}",
            std::process::id(),
            "debug_flag_exports_portage_debug"
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);

        let status = run_commands(RunCommandsArgs {
            ebuild_path: &ebuild_path,
            commands: &["install"],
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            distdir: &portage_tmpdir.join("distfiles"),
            debug,
            config_root: Path::new("/dev/null/no-config-root"),
            shell: ShellBackend::Brush,
            build_env: &[],
        })
        .expect("run_commands should not itself error");
        assert_eq!(status, 0);

        let marker =
            portage_tmpdir.join("portage/dev-libs/debugpkg-1.0/temp/portage-debug-value.txt");
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(observed, expected, "debug={debug}");

        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}

/// The fake distdir is prepared for a command other than `setup`
/// unless `FEATURES=noauto`.
#[test]
fn fake_distdir_is_prepared_only_when_portage_prepares_it() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let noauto = vec![("FEATURES".to_string(), "noauto".to_string())];
    for (commands, build_env, expect_distdir) in [
        (vec!["setup"], vec![], false),
        (vec!["setup"], noauto.clone(), false),
        (vec!["pretend"], noauto, false),
        (vec!["pretend"], vec![], true),
    ] {
        let portage_tmpdir = TempDir::new(&format!(
            "ebuild-phases-test-{}-fake-distdir-{}-{}",
            std::process::id(),
            commands[0],
            expect_distdir
        ));
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
        let status = run_commands(RunCommandsArgs {
            ebuild_path: &ebuild_path,
            commands: &commands,
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            distdir: &portage_tmpdir.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell: ShellBackend::Brush,
            build_env: &build_env,
        })
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "{commands:?} {build_env:?}");
        let distdir = portage_tmpdir.join("portage/dev-libs/phasepkg-1.0/distdir");
        assert_eq!(
            distdir.is_dir(),
            expect_distdir,
            "{commands:?} {build_env:?}"
        );
        let _ = std::fs::remove_dir_all(&portage_tmpdir);
    }
}

/// `pretend` alone still runs environment setup + `bin/ebuild.sh`
/// sourcing, even for an ebuild with no explicitly defined phases.
#[test]
fn pretend_alone_succeeds_with_no_explicit_phase_functions() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/phasepkg/phasepkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "pretend_alone_succeeds_with_no_explicit_phase_functions"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["pretend"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0);

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}
