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

/// A source-less ebuild with no `S` fails `install` with Portage's
/// `The source directory '${S}' doesn't exist`.
/// Portage: phase-functions.sh, __dyn_install.
#[test]
fn install_dies_like_portage_when_s_is_missing() {
    let tmp = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_dies_like_portage_when_s_is_missing"
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/nosrcpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild_path = pkg_dir.join("nosrcpkg-1.0.ebuild");
    std::fs::write(
        &ebuild_path,
        "EAPI=8\nDESCRIPTION=\"fixture: no sources, no S\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n\nsrc_install() {\n\tinsinto /usr/share/${PN}\n}\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("tmp");
    let log_file = tmp.join("build.log");

    let status = run_commands_logged(RunCommandsLoggedArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::default(),
        log_file: Some(&log_file),
        build_env: &[],
    })
    .expect("run_commands_logged should not itself error");
    assert_ne!(status, 0, "install must fail when ${{S}} does not exist");
    let log = std::fs::read_to_string(&log_file)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", log_file.display()));
    assert!(
        log.contains("The source directory") && log.contains("doesn't exist"),
        "install log should carry real's missing-S message, got:\n{log}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
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

/// End-to-end proof that the `install` post-phase QA runs: the
/// `95empty-dirs` check strips a genuinely empty dir for EAPI 8+
/// while a `keepdir`'d one survives.
#[test]
fn install_runs_the_portage_post_install_qa_check_and_strips_a_genuinely_empty_dir() {
    let tmp =
        TempDir::new("ebuild-phases-test-install_runs_the_portage_post_install_qa_check").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let pkg_dir = repo_root.join("dev-libs/qacheckpkg");
    std::fs::create_dir_all(repo_root.join("profiles")).unwrap();
    std::fs::write(repo_root.join("profiles/repo_name"), "qachecktest\n").unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("qacheckpkg-1.0.ebuild"),
        "EAPI=8\n\
         DESCRIPTION=\"fixture: real post-install QA check strips a genuinely empty dir\"\n\
         SLOT=\"0\"\n\
         KEYWORDS=\"amd64\"\n\
         S=\"${WORKDIR}\"\n\
         src_install() {\n\
         \tdodir /usr/lib/reallyempty\n\
         \tkeepdir /usr/lib/keptempty\n\
         }\n",
    )
    .unwrap();

    let ebuild_path = pkg_dir.join("qacheckpkg-1.0.ebuild");
    let portage_tmpdir = tmp.join("portage_tmpdir");

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

    let image_dir = portage_tmpdir.join("portage/dev-libs/qacheckpkg-1.0/image");
    assert!(
        !image_dir.join("usr/lib/reallyempty").exists(),
        "a genuinely empty dodir'd directory must be stripped by the real post-install QA check"
    );
    assert!(
        image_dir.join("usr/lib/keptempty").is_dir(),
        "a keepdir'd directory (real ebuild-author idiom) must survive"
    );

    let _ = std::fs::remove_dir_all(&tmp);
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

/// A saved `${T}/environment` that does not parse fails the next phase
/// loudly instead of running `default` against a half-loaded env.
/// Portage: bin/ebuild.sh, ebuild environment sourcing.
#[test]
fn a_corrupt_saved_environment_fails_the_next_phase_in_both_backends() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/heredocpkg/heredocpkg-1.0.ebuild");
    let tmp = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "a_corrupt_saved_environment_fails_the_next_phase_in_both_backends"
    ));
    let _ = std::fs::remove_dir_all(&tmp);

    for (shell, name) in [(ShellBackend::Bash, "bash"), (ShellBackend::Brush, "brush")] {
        let portage_tmpdir = tmp.join(name);
        let unpack_status = run_commands(RunCommandsArgs {
            ebuild_path: &ebuild_path,
            commands: &["unpack"],
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            distdir: &portage_tmpdir.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell,
            build_env: &[],
        })
        .expect("run_commands should not itself error");
        assert_eq!(unpack_status, 0, "{name}: unpack should exit successfully");

        // The post-phase save writes `${T}/environment`; break it with
        // a genuine syntax error (an unterminated quote -- an
        // unterminated here-document is only a warning to bash, not a
        // parse error, so it would not fail the bash control).
        let environment = portage_tmpdir.join("portage/dev-libs/heredocpkg-1.0/temp/environment");
        assert!(
            environment.is_file(),
            "{name}: the unpack phase must save {environment:?}"
        );
        std::fs::write(&environment, "f() { echo \"unterminated\n").unwrap();

        let log = tmp.join(format!("{name}.log"));
        let compile_status = run_single_phase(RunSinglePhaseArgs {
            ebuild_path: &ebuild_path,
            phase: "compile",
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell,
            build_env: &[],
            log_file: Some(&log),
        })
        .expect("run_single_phase should not itself error");
        assert_ne!(
            compile_status, 0,
            "{name}: a corrupt saved environment must fail the phase"
        );
        let logged = std::fs::read_to_string(&log)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", log.display()));
        assert!(
            logged.contains("error sourcing environment"),
            "{name}: expected real ebuild.sh's own die in the phase log, got:\n{logged}"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
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

/// End-to-end proof that the masters chain unblocks an `inherit()` of
/// an eclass existing only in the overlay's master repo.
#[test]
fn install_inherits_a_real_eclass_that_only_exists_in_the_overlays_own_master_repo() {
    let tmp = TempDir::new("ebuild-phases-test-eclass_masters_e2e").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let main = tmp.join("main");
    let overlay = tmp.join("overlay");
    std::fs::create_dir_all(main.join("profiles")).unwrap();
    std::fs::create_dir_all(main.join("eclass")).unwrap();
    std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
    std::fs::write(
        main.join("eclass/mastershared.eclass"),
        "mastershared_hello() {\n\techo \"hello from mastershared.eclass\"\n}\n",
    )
    .unwrap();

    let pkg_dir = overlay.join("dev-libs/overlaypkg");
    std::fs::create_dir_all(overlay.join("profiles")).unwrap();
    std::fs::write(overlay.join("profiles/repo_name"), "overlay\n").unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("overlaypkg-1.0.ebuild"),
        "EAPI=8\n\
         DESCRIPTION=\"fixture: real cross-repo masters-chain eclass inherit\"\n\
         SLOT=\"0\"\n\
         KEYWORDS=\"amd64\"\n\
         S=\"${WORKDIR}\"\n\
         inherit mastershared\n\
         src_install() {\n\
         \tmastershared_hello > \"${T}/eclass-marker.txt\" || die\n\
         }\n",
    )
    .unwrap();

    std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
    std::fs::write(
        tmp.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n\
             [main]\nlocation = {}\n\n\
             [overlay]\nlocation = {}\nmasters = main\n",
            main.display(),
            overlay.display(),
        ),
    )
    .unwrap();

    let ebuild_path = pkg_dir.join("overlaypkg-1.0.ebuild");
    let portage_tmpdir = tmp.join("portage_tmpdir");

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: &tmp,
        shell: ShellBackend::Brush,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let marker = portage_tmpdir.join("portage/dev-libs/overlaypkg-1.0/temp/eclass-marker.txt");
    let observed = std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
    assert_eq!(observed, "hello from mastershared.eclass\n");

    let _ = std::fs::remove_dir_all(&tmp);
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

/// Phases export `PORTAGE_ACTUAL_DISTDIR` as `DISTDIR` for `doins.py
/// --distdir`; without it absolute symlinks dereference and fail.
/// Portage: config.py, config.environ.
#[test]
fn phases_export_portage_actual_distdir_so_doins_keeps_absolute_symlinks() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/actualdistdirpkg/actualdistdirpkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-actual-distdir",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let distdir = portage_tmpdir.join("distfiles");

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
    assert_eq!(status, 0);

    let builddir = portage_tmpdir.join("portage/dev-libs/actualdistdirpkg-1.0");
    let observed = std::fs::read_to_string(builddir.join("temp/portage-actual-distdir.txt"))
        .expect("the install phase should record PORTAGE_ACTUAL_DISTDIR");
    assert_eq!(observed, distdir.display().to_string());
    let link = builddir.join("image/usr/share/actualdistdirpkg/payload/abs-link");
    assert_eq!(
        std::fs::read_link(&link).expect("abs-link should be installed as a symlink"),
        Path::new("/usr/share/actualdistdirpkg/not-installed-yet")
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Every phase sees `DISTDIR` as symlinks of its own `A` only, with the
/// directory itself in `PORTAGE_ACTUAL_DISTDIR`; foreign distfiles hide.
#[test]
fn phases_see_the_per_package_fake_distdir() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/fakedistdirpkg/fakedistdirpkg-1.0.ebuild");
    let name = format!("ebuild-phases-test-{}-fake-distdir", std::process::id());
    let portage_tmpdir = TempDir::new(&name);
    let distdir = TempDir::new(&format!("{name}-distfiles"));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&distdir);
    std::fs::create_dir_all(&distdir).unwrap();
    std::fs::write(
        distdir.join("fakedistdirpkg-1.0.tar.gz"),
        b"hello from fakedistdirpkg\n",
    )
    .unwrap();
    // Present in the distdir but not in this package's SRC_URI.
    std::fs::write(distdir.join("frs-1.0.tar.gz"), b"unrelated\n").unwrap();

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

    let builddir = portage_tmpdir.join("portage/dev-libs/fakedistdirpkg-1.0");
    let observed = std::fs::read_to_string(builddir.join("temp/fake-distdir.txt"))
        .expect("the install phase should record its DISTDIR view");
    assert_eq!(
        observed,
        format!(
            "DISTDIR={}\nPORTAGE_ACTUAL_DISTDIR={}\nLINK={}\nUNLISTED=absent\n",
            builddir.join("distdir").display(),
            distdir.display(),
            distdir.join("fakedistdirpkg-1.0.tar.gz").display(),
        )
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&distdir);
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
