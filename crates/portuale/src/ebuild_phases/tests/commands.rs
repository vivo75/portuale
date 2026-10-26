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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
        log_file: PhaseLog::Capture(&log_file),
        skip_noop_src_phases: false,
        skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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

/// End-to-end proof of the `binpkg-docompress` path: with
/// `PORTAGE_COMPRESS=bzip2` and `FEATURES=binpkg-docompress`, big docs
/// compress, small ones don't, and dangling links are repaired.
/// Uses the `bash` backend, like a live `emerge`.
#[test]
fn install_qa_check_docompress_compresses_docs_and_repairs_symlinks() {
    if std::process::Command::new("bzip2")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: bzip2 not available on this host");
        return;
    }
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/doccompresspkg/doccompresspkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-docompress").keep();
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let extra_env = vec![
        ("PORTAGE_COMPRESS".to_string(), "bzip2".to_string()),
        ("FEATURES".to_string(), "binpkg-docompress".to_string()),
    ];
    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Bash,
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        build_env: &extra_env,
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let docdir = portage_tmpdir
        .join("portage/dev-libs/doccompresspkg-1.0/image/usr/share/doc/doccompresspkg-1.0");
    assert!(
        docdir.join("BIG.txt.bz2").is_file(),
        "a doc above PORTAGE_DOCOMPRESS_SIZE_LIMIT must be compressed"
    );
    assert!(
        !docdir.join("BIG.txt").exists(),
        "ecompress must remove the uncompressed original"
    );
    assert!(
        docdir.join("small.txt").is_file(),
        "a doc below the size limit must stay uncompressed"
    );
    assert_eq!(
        std::fs::read_link(docdir.join("link-to-big.txt.bz2")).unwrap(),
        Path::new("BIG.txt.bz2"),
        "a symlink into a compressed doc must be repaired and suffixed"
    );

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The merge-time `instprep` phase complements the `install_qa_check`
/// gate and is idempotent through `.instprepped`.
#[test]
fn run_instprep_applies_only_the_complement_of_the_install_qa_gate() {
    if std::process::Command::new("bzip2")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: bzip2 not available on this host");
        return;
    }
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/doccompresspkg/doccompresspkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new("ebuild-phases-test-instprep").keep();
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let no_root = Path::new("/dev/null/no-config-root");
    let env_with = |features: &str| {
        vec![
            ("PORTAGE_COMPRESS".to_string(), "bzip2".to_string()),
            // `binpkg-dostrip` stays on: this test is about compression.
            ("FEATURES".to_string(), format!("binpkg-dostrip {features}")),
        ]
    };

    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: no_root,
        shell: ShellBackend::Bash,
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        build_env: &env_with("-binpkg-docompress"),
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");
    let builddir = portage_tmpdir.join("portage/dev-libs/doccompresspkg-1.0");
    let docdir = builddir.join("image/usr/share/doc/doccompresspkg-1.0");
    assert!(
        docdir.join("BIG.txt").is_file(),
        "install_qa_check must not compress without binpkg-docompress"
    );

    let instprep = |features: &str| {
        run_instprep(RunInstprepArgs {
            ebuild_path: &ebuild_path,
            saved_env_bz2: None,
            seed: false,
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            build_env: &env_with(features),
            debug: false,
            config_root: no_root,
            shell: ShellBackend::Bash,
            log_file: PhaseLog::Inherit,
            // No saved env here, so no repo push either.
            repo_name: None,
        })
        .expect("run_instprep should not itself error")
    };

    assert_eq!(instprep("binpkg-docompress"), 0);
    assert!(
        docdir.join("BIG.txt").is_file(),
        "instprep must not compress when binpkg-docompress is on"
    );
    assert!(builddir.join(".instprepped").is_file());

    std::fs::remove_file(builddir.join(".instprepped")).unwrap();
    assert_eq!(instprep("-binpkg-docompress"), 0);
    assert!(docdir.join("BIG.txt.bz2").is_file());
    assert!(!docdir.join("BIG.txt").exists());
    assert!(docdir.join("small.txt").is_file());
    assert_eq!(
        std::fs::read_link(docdir.join("link-to-big.txt.bz2")).unwrap(),
        Path::new("BIG.txt.bz2")
    );
    assert!(builddir.join(".instprepped").is_file());

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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        // The merge path runs the post-`install` QA sequence; the direct
        // `ebuild` path never does.
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

/// The post-`install` QA spawn carries no `EBUILD_PHASE` (Portage's
/// `MiscFunctionsProcess._spawn` pops it), so a repo-local check
/// calling `use test` on an ebuild without that flag passes instead of dying
/// with `USE Flag 'test' not in IUSE`. The probe check records the phase var
/// it saw plus which `use test` arm it took, under `${T}`; the run must exit
/// 0 with `EBUILD_PHASE` unset and exactly one arm marker (QA really ran).
/// `doebuild()` reaches the same `_PostPhaseCommands` through its synchronous
/// `EbuildPhase`, so the direct `ebuild` path and the merge path behave alike
/// here (a host `ebuild ... install` runs `95empty-dirs` the same way).
#[test]
fn install_qa_spawn_pops_ebuild_phase_so_use_test_does_not_die() {
    let tmp = TempDir::new("ebuild-phases-test-516-noiuse-qa").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let pkg_dir = repo_root.join("dev-libs/noiuseqa");
    std::fs::create_dir_all(repo_root.join("profiles")).unwrap();
    std::fs::write(repo_root.join("profiles/repo_name"), "noiuseqatest\n").unwrap();
    std::fs::create_dir_all(repo_root.join("metadata/install-qa-check.d")).unwrap();
    std::fs::write(
        repo_root.join("metadata/install-qa-check.d/65probe-noiuse"),
        "# probe: EBUILD_PHASE must be unset in the QA spawn\n\
         probe_noiuse_check() {\n\
         \techo \"EBUILD_PHASE=[${EBUILD_PHASE-unset}]\" > \"${T}/qa-ebuild-phase\"\n\
         \tif use test; then\n\
         \t\t: > \"${T}/qa-probe-ran\"\n\
         \telse\n\
         \t\t: > \"${T}/qa-probe-skip\"\n\
         \tfi\n\
         }\n\
         probe_noiuse_check\n\
         : # guarantee successful exit\n",
    )
    .unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("noiuseqa-1.0.ebuild"),
        "EAPI=8\n\
         DESCRIPTION=\"fixture: no IUSE, probe post-install QA gating\"\n\
         SLOT=\"0\"\n\
         KEYWORDS=\"amd64\"\n\
         S=\"${WORKDIR}\"\n\
         src_install() {\n\
         \tdodir /usr/share/noiuseqa\n\
         }\n",
    )
    .unwrap();

    let ebuild_path = pkg_dir.join("noiuseqa-1.0.ebuild");
    let portage_tmpdir = tmp.join("portage-tmpdir");
    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["install"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Brush,
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(
        status, 0,
        "install must not die in `use test` without EBUILD_PHASE"
    );
    let t_dir = portage_tmpdir.join("portage/dev-libs/noiuseqa-1.0/temp");
    assert_eq!(
        std::fs::read_to_string(t_dir.join("qa-ebuild-phase")).unwrap(),
        "EBUILD_PHASE=[unset]\n",
        "the QA spawn carries no EBUILD_PHASE"
    );
    let ran = t_dir.join("qa-probe-ran").is_file();
    let skipped = t_dir.join("qa-probe-skip").is_file();
    assert!(
        ran ^ skipped,
        "the QA check ran exactly once (ran={ran}, skipped={skipped})"
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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

/// End to end in a phase: under `FEATURES=xattr` installs reach
/// `ebuild-helpers/xattr/install` first and keep `user.*` xattr;
/// without `xattr` they lose it.
#[test]
fn features_xattr_routes_phase_installs_through_the_xattr_wrapper() {
    use std::os::unix::ffi::OsStrExt;
    if !Path::new("/usr/bin/setfattr").exists() {
        eprintln!("skipped: no setfattr");
        return;
    }
    let tmp = TempDir::new("ebuild-phases-test-xattr-install").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/xattrinstpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("xattrinstpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\nS=\"${WORKDIR}\"\n\
         src_unpack() {\n\
         \tfor f in viaexe viainstall; do\n\
         \t\tprintf '#!/bin/sh\\n' > \"$f\" || die\n\
         \t\tsetfattr -n user.portuale329 -v \"$f\" \"$f\" || die\n\
         \tdone\n}\n\
         src_install() {\n\
         \texport PORTAGE_INSTALL_XATTR_IMPLEMENTATION=python\n\
         \texeinto /usr/libexec/xa; doexe viaexe\n\
         \tinstall -D -m 0755 viainstall \"${ED}/usr/libexec/xa/viainstall\" || die\n}\n",
    )
    .unwrap();
    let probe = tmp.join("probe");
    std::fs::write(&probe, b"x").unwrap();
    if std::process::Command::new("setfattr")
        .args(["-n", "user.probe", "-v", "1"])
        .arg(&probe)
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        eprintln!("skipped: no user xattrs on {}", tmp.display());
        return;
    }
    for (i, (features, expect)) in [("xattr", true), ("", false)].into_iter().enumerate() {
        let portage_tmpdir = tmp.join(format!("tmp{i}"));
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let status = run_commands(RunCommandsArgs {
            ebuild_path: &ebuild,
            commands: &["install"],
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            distdir: &tmp.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell: ShellBackend::Brush,
            log_file: PhaseLog::Inherit,
            skip_noop_src_phases: false,
            skip_pretend: false,
            build_env: &[("FEATURES".to_string(), features.to_string())],
        })
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "install should succeed (FEATURES={features:?})");
        let image = portage_tmpdir.join("portage/dev-libs/xattrinstpkg-1.0/image");
        for name in ["viaexe", "viainstall"] {
            let file = image.join("usr/libexec/xa").join(name);
            let got =
                crate::helpers::xattr_get(file.as_os_str().as_bytes(), b"user.portuale329").ok();
            let want = expect.then(|| name.as_bytes().to_vec());
            assert_eq!(got, want, "{name} with FEATURES={features:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
            log_file: PhaseLog::Inherit,
            skip_noop_src_phases: false,
            skip_pretend: false,
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
            log_file: PhaseLog::Capture(&log),
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
        log_file: PhaseLog::Capture(&log),
        skip_noop_src_phases: false,
        skip_pretend: false,
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

/// Declaration builtins assignment-expand an expanded name in both
/// backends; the fixture dies unless the expanded export arrived.
#[test]
fn expanded_name_export_assigns_in_both_backends() {
    fn image_file_set(image: &Path) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("{} should be readable: {e}", dir.display()))
            {
                let path = entry
                    .unwrap_or_else(|e| panic!("dir entry should read: {e}"))
                    .path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    out.push(
                        path.strip_prefix(base)
                            .unwrap_or_else(|e| panic!("path should be under image: {e}"))
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(image, image, &mut out);
        out.sort();
        out
    }

    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/exportexpandpkg/exportexpandpkg-1.0.ebuild");
    let tmp = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "expanded_name_export_assigns_in_both_backends"
    ));
    let _ = std::fs::remove_dir_all(&tmp);

    let mut file_sets = Vec::new();
    for (shell, name) in [(ShellBackend::Bash, "bash"), (ShellBackend::Brush, "brush")] {
        let portage_tmpdir = tmp.join(name);
        let status = run_commands(RunCommandsArgs {
            ebuild_path: &ebuild_path,
            commands: &["install"],
            root: Path::new("/"),
            portage_tmpdir: &portage_tmpdir,
            distdir: &portage_tmpdir.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell,
            log_file: PhaseLog::Inherit,
            skip_noop_src_phases: false,
            skip_pretend: false,
            build_env: &[],
        })
        .expect("run_commands should not itself error");
        assert_eq!(status, 0, "{name}: install should exit successfully");

        let marker = portage_tmpdir.join(
            "portage/dev-libs/exportexpandpkg-1.0/image/usr/share/exportexpandpkg/decl-assign-check.txt",
        );
        let observed = std::fs::read_to_string(&marker)
            .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
        assert_eq!(
            observed, "decl-assign-ok\n",
            "{name}: the expanded export must arrive"
        );

        file_sets.push((
            name,
            image_file_set(&portage_tmpdir.join("portage/dev-libs/exportexpandpkg-1.0/image")),
        ));
    }

    assert!(
        !file_sets[1].1.is_empty(),
        "the brush run produced an empty image"
    );
    assert_eq!(
        file_sets[1].1, file_sets[0].1,
        "brush and bash image file sets differ"
    );

    let _ = std::fs::remove_dir_all(&tmp);
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
            log_file: PhaseLog::Inherit,
            skip_noop_src_phases: false,
            skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
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
            log_file: PhaseLog::Inherit,
            skip_noop_src_phases: false,
            skip_pretend: false,
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
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        build_env: &[],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0);

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// `FEATURES=compress-build-logs` pump round-trip: bytes land
/// gzip-encoded (one member per call, appended) and decode losslessly.
/// Portage: EbuildPhase, _open_log.
#[test]
fn open_log_file_gzip_pump_round_trips() {
    use std::io::{Read, Write};
    let dir = TempDir::new("ebuild-phases-gzlog").keep();
    let _ = std::fs::remove_dir_all(&dir);
    let gz = dir.join("build.log.gz");
    let plain = dir.join("build.log");

    for (path, stdout_text, stderr_text) in [
        (&gz, "phase one out\n", "phase one err\n"),
        (&gz, "phase two out\n", ""),
    ] {
        let mut sink = open_log_file(path).expect("open_log_file succeeds");
        assert!(sink.pump.is_some(), "{path:?} must pump");
        sink.out.write_all(stdout_text.as_bytes()).unwrap();
        sink.err.write_all(stderr_text.as_bytes()).unwrap();
        // Closing the write ends (then joining) finishes the
        // member -- the same drop order the phase call sites use.
        drop(sink);
    }
    let mut sink = open_log_file(&plain).expect("open_log_file succeeds");
    assert!(sink.pump.is_none());
    sink.out.write_all(b"plain\n").unwrap();
    drop(sink);

    let mut decoded = String::new();
    flate2::read::MultiGzDecoder::new(std::fs::File::open(&gz).expect("gz log exists"))
        .read_to_string(&mut decoded)
        .expect("gz log decodes");
    // Both members, both streams -- stdout/stderr chunk order
    // across the two pipes was never deterministic (Portage appends
    // both fds to one gzip stream too), so compare unordered.
    let mut lines: Vec<&str> = decoded.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        vec!["phase one err", "phase one out", "phase two out"],
    );
    assert_eq!(std::fs::read_to_string(&plain).unwrap(), "plain\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// S5 end-to-end: a stamped `unpack` prints Portage's skip pair on stdout
/// and the phase body does not run again. One subprocess per invocation
/// (the `ebuild_test_forces_src_test_without_features_test` pattern in
/// `phase_runs.rs`): output assertions need a real process, and the binary
/// path also covers `ebuild.rs`'s per-command loop.
/// Portage 3.0.82.2, S0 cells `s01-unpack`/`s02-unpack-again` (full argv
/// `ebuild t348-b-phases-8.ebuild unpack`, both rc 0).
#[test]
fn stamped_unpack_prints_the_skip_pair_and_does_not_rerun_the_body() {
    let tmp = TempDir::new("ebuild-phases-test-stamped-unpack-skips");
    let pkg_dir = tmp.join("pkg/dev-libs/stamptestpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("stamptestpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\nS=\"${WORKDIR}\"\n\
         src_unpack() { echo ran >> \"${T}/unpack-ran\" || die; }\n",
    )
    .unwrap();
    let root = tmp.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let portage_tmpdir = tmp.join("tmp");
    let builddir = portage_tmpdir.join("portage/dev-libs/stamptestpkg-1.0");
    let marker = builddir.join("temp/unpack-ran");
    let binary = portuale_exe();
    let run = || {
        std::process::Command::new(&binary)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("ROOT", &root)
            .env("PORTAGE_TMPDIR", &portage_tmpdir)
            .args(["ebuild", ebuild.to_str().unwrap(), "unpack"])
            .output()
            .expect("portuale ebuild spawns")
    };
    let ran_lines = || {
        std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let first = run();
    assert!(
        first.status.success(),
        "first unpack: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(ran_lines(), 1, "the body runs once");
    let second = run();
    assert!(
        second.status.success(),
        "stamped unpack: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains(">>> WORKDIR is up-to-date, keeping..."),
        "the WORKDIR line comes first, got:\n{stdout}"
    );
    assert!(
        stdout.contains(
            ">>> It appears that 'unpack' has already executed for \
             'stamptestpkg-1.0'; skipping."
        ),
        "the skip pair, got:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            ">>> Remove '{}' to force unpack.",
            builddir.join(".unpacked").display()
        )),
        "the force line names the stamp, got:\n{stdout}"
    );
    assert_eq!(ran_lines(), 1, "the stamped body must not run again");
}

/// The foreground tee writes every child byte to our stdout and to the
/// log file (Portage's `_spawn_phase` logging pipe merges both streams
/// onto the terminal and into `${T}/build.log`). The stdout half lands
/// on the test runner's captured output; the file half is asserted here.
#[test]
fn open_tee_log_writes_to_the_file_what_the_child_writes() {
    use std::io::Write as _;
    let tmp = TempDir::new("ebuild-phases-test-tee").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let log = tmp.join("build.log");
    let (mut child_end, pump) = open_tee_log(&log).expect("tee opens");
    child_end.write_all(b">>> Unpacking source...\n").unwrap();
    drop(child_end);
    drop(pump);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        ">>> Unpacking source...\n"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `PhaseLog::capture` maps a background log path to `Capture` and its
/// absence to `Inherit` (never to a silent third shape).
#[test]
fn phase_log_capture_maps_an_absent_path_to_inherit() {
    let path = Path::new("/tmp/build.log");
    assert!(matches!(
        PhaseLog::capture(Some(path)),
        PhaseLog::Capture(p) if p == path
    ));
    assert!(matches!(PhaseLog::capture(None), PhaseLog::Inherit));
}

/// A direct `unpack` tees Portage's markers to stdout and writes
/// `${T}/build.log` (S0 cell `m01-unpack`, rc 0): `>>> Unpacking
/// source...` and `>>> Source unpacked in ...` on stdout, the same
/// bytes in the log, nothing marker-shaped on stderr.
///
/// Portage 3.0.82.2, full argv `ebuild
/// t348-b-phases-8.ebuild unpack` (S0 cell `m01-unpack-portage.stdout`,
/// 124 B on stdout, config warnings only on stderr).
#[test]
fn foreground_unpack_tees_portages_markers_to_stdout_and_build_log() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let ebuild_path = fixtures.join("repo-348/app-misc/t348-b-phases/t348-b-phases-8.ebuild");
    let tmp = TempDir::new("ebuild-phases-test-s6-unpack").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let cfg = tmp.join("cfg/etc/portage");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = testrepo\n\n\
             [testrepo]\nlocation = {}\n\n\
             [t348]\nlocation = {}/repo-348\npriority = 60\n",
            fixtures.join("repo").display(),
            fixtures.display(),
        ),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        fixtures.join("repo/profiles/default"),
        cfg.join("make.profile"),
    )
    .unwrap();
    let portage_tmpdir = tmp.join("tmp");
    let distdir = tmp.join("dist");
    let output = std::process::Command::new(portuale_exe())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("PORTAGE_CONFIGROOT", tmp.join("cfg"))
        .env("PORTAGE_TMPDIR", &portage_tmpdir)
        .env("DISTDIR", &distdir)
        .env(
            "FEATURES",
            "-sandbox -usersandbox -userpriv -network-sandbox -ipc-sandbox -pid-sandbox -news",
        )
        .args(["ebuild", &ebuild_path.display().to_string(), "unpack"])
        .output()
        .expect("portuale ebuild spawns");
    assert!(
        output.status.success(),
        "unpack: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let builddir = portage_tmpdir.join("portage/app-misc/t348-b-phases-8");
    assert_eq!(
        stdout,
        format!(
            ">>> Unpacking source...\n>>> Source unpacked in {}/work\n",
            builddir.display()
        ),
        "Portage's unpack markers, got:\n{stdout}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(">>>"),
        "no markers on stderr"
    );
    let logged =
        std::fs::read_to_string(builddir.join("temp/build.log")).expect("build.log written");
    assert_eq!(logged, stdout, "the log carries the terminal stream");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The merge-path no-op src-phase skip (backlog #456,
/// `EbuildExecuter._can_skip_source_phases`): `dev-libs/schedleaf-a`
/// ships no `SRC_URI` and defines only `src_install`
/// (`DEFINED_PHASES=install` in its md5-cache entry), so a merge run
/// must skip `unpack`/`prepare`/`configure`/`compile`/`test` outright.
#[test]
fn skip_fires_for_sourceless_install_only_ebuild() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/schedleaf-a/schedleaf-a-1.0.ebuild");
    let portage_tmpdir = TempDir::new("skip-sourceless");
    let env = compute_environment(&ebuild_path, &portage_tmpdir).expect("env computes");
    assert!(
        can_skip_source_phases("", "", &env, true),
        "empty A, no skippable DEFINED_PHASES, no live PROPERTIES, no noauto: skip"
    );
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// The skip stays off when any gate fails: a fetched file, a defined
/// `src_compile` (`dev-libs/unsetcflagspkg`), `FEATURES=noauto`, or a
/// `live` property (`dev-libs/propertiespkg`).
#[test]
fn skip_stays_off_when_any_gate_fails() {
    let fixture = |pkg: &str| {
        let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../fixtures/repo/dev-libs/{pkg}/{pkg}-1.0.ebuild"
        ));
        let portage_tmpdir = TempDir::new(&format!("skip-gate-{pkg}"));
        let env = compute_environment(&ebuild_path, &portage_tmpdir).expect("env computes");
        (env, portage_tmpdir)
    };
    // A non-empty `A` means something to unpack.
    let (env, tmp) = fixture("schedleaf-a");
    assert!(!can_skip_source_phases("", "", &env, false));
    let _ = std::fs::remove_dir_all(&tmp);
    // `src_compile` is defined (`DEFINED_PHASES` carries `compile`).
    let (env, tmp) = fixture("unsetcflagspkg");
    assert!(!can_skip_source_phases("", "", &env, true));
    let _ = std::fs::remove_dir_all(&tmp);
    // Manual `ebuild` mode relies on running each phase.
    let (env, tmp) = fixture("schedleaf-a");
    assert!(!can_skip_source_phases("noauto", "", &env, true));
    let _ = std::fs::remove_dir_all(&tmp);
    // Live ebuilds fetch their source in `src_unpack`.
    let (env, tmp) = fixture("propertiespkg");
    assert!(!can_skip_source_phases("", "", &env, true));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `pkg_setup` records `.src_patches` when `PATCHES` is set; the default
/// `src_prepare` applies it, so the skip must not fire afterwards.
#[test]
fn skip_stays_off_once_setup_records_src_patches() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/schedleaf-a/schedleaf-a-1.0.ebuild");
    let portage_tmpdir = TempDir::new("skip-patches");
    let env = compute_environment(&ebuild_path, &portage_tmpdir).expect("env computes");
    assert!(can_skip_source_phases("", "", &env, true));
    std::fs::create_dir_all(env.portage_builddir()).unwrap();
    std::fs::write(env.portage_builddir().join(".src_patches"), "").unwrap();
    assert!(
        !can_skip_source_phases("", "", &env, true),
        "a recorded PATCHES set keeps src_prepare"
    );
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}
