use super::*;

/// End-to-end proof: a real `install` run against the real fixture
/// ebuild (`fixtures/repo/dev-libs/phasepkg`, whose own
/// `src_install` calls real `insinto`/`doins`) actually lands a real
/// file under a real `${D}`, via the real, unmodified `bin/*.sh`
/// this module drives -- not a mock of any kind. A fresh, uniquely
/// named `PORTAGE_TMPDIR` per test run (passed directly to
/// `run_commands`, never via the environment -- see its own doc
/// comment for why) keeps this safe to run alongside other tests in
/// the same process.
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

/// #308: an in-process `Brush` phase must not leave its umask behind.
/// `bin/ebuild.sh` runs `umask 022` and brush's builtin is the
/// process-wide `umask(2)`; real runs each phase in its own process.
/// The test moves the umask, which would itself race every parallel
/// test's file modes, so the body runs in a re-executed child of this
/// test binary (`PORTUALE_UMASK_CHILD`) and the parent only checks it.
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

/// Backlog #190 (S0 repro): real `bin/phase-functions.sh`'s
/// `__dyn_install` -- like its `prepare`/`configure`/`compile`/
/// `test` twins (`phase-functions.sh:444,480,512,546,638`) -- dies
/// with `The source directory '${S}' doesn't exist` when `S` is not
/// a directory. The default is `S=${WORKDIR}/${P}`
/// (`bin/ebuild.sh:500`); EAPI 8 has no `${WORKDIR}` fallback
/// (`bin/eapi.sh:8`: `___eapi_has_S_WORKDIR_fallback` is EAPI 0-3
/// only), and with `A` empty but `src_install` defined,
/// `__has_phase_defined_up_to install` is true, so neither
/// fallback fires and the phase dies. A source-less ebuild with no
/// `S` must therefore fail the `install` run with real's message.
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

/// Backlog #47: real `_post_src_install_write_metadata`
/// (`doebuild.py:2727-2732`) writes `build-info/BUILD_TIME` (epoch
/// seconds) before any metadata key, and the vdb copy +
/// `_consolidate_to_metadata_file` carry it. The install chain must
/// leave that file behind for the merge to copy.
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

/// Real, end-to-end proof of the `binpkg-docompress` transform path
/// (#38 S1): with `PORTAGE_COMPRESS=bzip2` and `FEATURES=binpkg-
/// docompress` in the phase env, real, unmodified `install_qa_check`
/// (`misc-functions.sh:147-152`) runs real `bin/ecompress` over
/// `${D}` -- the >128 B doc (the `PORTAGE_DOCOMPRESS_SIZE_LIMIT`)
/// becomes `BIG.txt.bz2`, the smaller one is untouched, and the
/// dangling link into the now-compressed doc is repaired (and
/// suffixed) by `ecompress`'s own `fix_symlinks`. Uses the default
/// `bash` backend (`ShellBackend::Bash`), the same one a live
/// `emerge` runs.
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

/// #38 S4: the merge-time `instprep` phase (`misc-functions.sh:
/// 265-308`) is the complement of the `install_qa_check` gate. With
/// `binpkg-docompress` absent the install chain leaves `BIG.txt`
/// plain; `run_instprep` with the token present is a no-op (real's
/// `! contains_word` gate), and without it compresses the doc and
/// repairs the link, then marks `.instprepped` so a re-run skips.
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
            log_file: None,
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

/// Real, end-to-end proof that `_post_phase_cmds["install"]`
/// (`EbuildPhase.py:424`/`442-461`) actually runs now: real,
/// unmodified `bin/misc-functions.sh install_qa_check`'s own real
/// `95empty-dirs` QA check (`bin/install-qa-check.d/95empty-dirs`)
/// strips a genuinely empty directory from the install image for
/// any EAPI 8+ ebuild (real `___eapi_has_strict_keepdir`,
/// unconditional, not gated on any `FEATURES` flag) -- a bare
/// `dodir` with nothing ever installed into it must be gone from
/// `${D}` by the time `install` finishes, while a `keepdir`'d one
/// (the real ebuild-author idiom this QA check's own message
/// recommends) survives untouched.
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

/// `ShellBackend::Bash` counterpart of `install_lands_a_real_file_
/// under_a_real_d` above -- same fixture, same assertions, proving
/// the real `bash <bin_dir>/ebuild.sh <phase>` subprocess backend
/// (`run_one_phase_bash`) produces an identical real result to the
/// embedded-brush backend, not just that it runs without erroring.
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

/// Real, end-to-end proof of `fetch_sources`'s own `A`/`AA`
/// computation through the real CLI path, deterministic and
/// offline: `dev-libs/verifiedfetchpkg`'s own real, checked-in
/// `Manifest` entry matches a payload file this test pre-seeds into
/// `DISTDIR` (the real, valid BLAKE2b-512/SHA-512 digests of the
/// literal bytes `"hello from verifiedfetchpkg\n"`, confirmed via
/// the real `b2sum`/`sha512sum` system tools) -- so the "already
/// verified" skip-fetch path fires and no real network access is
/// attempted at all, while still exercising the full real SRC_URI
/// grammar: an arrow-rename (`-> verifiedfetchpkg-1.0.tar.gz`) and a
/// `test?` USE-conditional group that must stay excluded from `A`
/// (portuale's own always-empty USE set) but still appear in
/// `AA` (real PMS's own "every file regardless of USE" definition).
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
    // `AA` is unset: real `config.environ()` pops it for every
    // EAPI >= 4 (`config.py:3331-3333`); the EAPI floor here is 5+.
    // The ebuild's own `echo "AA=${AA}"` therefore writes an empty
    // value, matching a real EAPI-8 phase.
    assert_eq!(observed, "A=verifiedfetchpkg-1.0.tar.gz\nAA=\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
    let _ = std::fs::remove_dir_all(&distdir);
}

/// #329 end to end in a phase: under `FEATURES=xattr` a phase's
/// `doexe` and plain `install` reach `ebuild-helpers/xattr/install`
/// first, and with `PORTAGE_INSTALL_XATTR_IMPLEMENTATION=python` (set
/// by the ebuild; real does not whitelist it) the wrapper runs
/// `install.py` through `portuale-python`, portuale's native one. The
/// image files keep the source's `user.*` xattr; without `xattr`
/// they lose it. Real Portage does the same in the bed image (pmtest
/// `differential-test-bed/scripts/329-xattr.sh`, both with and
/// without `-xattr`).
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

/// Real, end-to-end proof of `eclass_locations_value`: `dev-libs/
/// eclasspkg` really `inherit`s a real (if fixture-only) eclass,
/// `pilotcheck.eclass`, via real, unmodified `bin/ebuild.sh`'s own
/// `inherit()` function -- previously portuale never populated
/// `PORTAGE_ECLASS_LOCATIONS` at all, so this would have `die`d
/// immediately with `"pilotcheck.eclass could not be found by
/// inherit()"`. `src_install` calls a real function the eclass
/// defines (`pilotcheck_hello`), proving the eclass's own content
/// -- not just its own existence -- is really usable afterward.
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

/// B2: a saved `${T}/environment` that does not parse must fail the
/// next phase loudly -- real `bin/ebuild.sh:580`'s own `source
/// "${T}"/environment || die "error sourcing environment"` -- never
/// silently continue and run the `default` phase function against a
/// half-loaded environment (the #38 G3 smoke's empty image with rc 0).
/// `run_single_phase` runs exactly one phase, with no `actionmap_deps`
/// chain that would re-save a fresh environment first: the same shape
/// a real builddir resume has when portage starts the next phase.
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

/// B3: real `bin/phase-functions.sh`'s `__filter_readonly_variables`
/// lists bash's special variables by running `env -i -- "${BASH}" -c
/// …`, so the embedded brush shell must carry a real `$BASH` -- and
/// brush's own brace expansion must produce that list's fields even
/// under the function's own `local IFS` (the two brush-side fixes are
/// both `brush-pin.md`'s tracked work). When either is missing, the
/// saved `${T}/environment` carries `BASHOPTS`/`EUID`/`PPID`/
/// `SHELLOPTS`/`UID`, and re-sourcing it prints `cannot mutate
/// readonly variable` (plus `env: ''` when `$BASH` itself is empty).
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

/// B4: declaration builtins must assignment-expand an expanded name
/// (brush fix 06, backlog #94). `export ${var}=value` silently
/// no-op'd under brush, so `_tc-getPROG`-shaped code saw empty
/// variables; the fixture's `src_install` dies unless the expanded
/// export arrived. Both backends must install it and agree on the
/// installed file set, not merely on the exit code.
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

/// Real, end-to-end proof that the masters-chain fix actually
/// unblocks a real `inherit()` call real `PORTAGE_ECLASS_LOCATIONS`
/// resolution alone couldn't reach before: an overlay ebuild
/// inheriting an eclass that only exists in its own master repo,
/// never redeclared locally -- exactly the real gap this module's
/// own doc comment (before this slice) named as out of scope.
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

/// Real, not simulated (task #56): passing `debug: true` really
/// exports `PORTAGE_DEBUG=1` into the phase's own environment (see
/// `run_one_phase`'s own setup block) -- proven here by having the
/// fixture's own `src_install` record the value it actually observed,
/// rather than asserting on captured `set -x` trace output (which
/// would need redirecting the whole test process's stdout/stderr, a
/// much heavier and flakier mechanism for the same underlying claim).
/// Real `bin/ebuild.sh:479`'s own `[[ ${PORTAGE_DEBUG} == 1 ]]` guard
/// is what turns this exported value into the real `set -x` xtrace a
/// human running `ebuild <file> install --debug` directly would see;
/// that guard itself is real, unmodified bash portuale doesn't
/// reimplement, so proving the export is correct is sufficient here.
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

/// Real `config.environ()` (`lib/portage/package/ebuild/config.py:3403-3409`)
/// exports `PORTAGE_ACTUAL_DISTDIR` as the real `DISTDIR`, and
/// `bin/ebuild-helpers/doins:80-82` passes it as `doins.py --distdir`.
/// Without it `--distdir` defaults to `""`, which `_parse_args` turns
/// into `b"/"`, so `doins -r` dereferences every absolute symlink.
/// A dangling one (porttest/helper-doins's `abs-link`, found by the
/// #326 Z nopy leg) then fails with `FileNotFoundError`, while real
/// Portage installs the link as a link.
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

/// Real gives every phase `DISTDIR=${PORTAGE_BUILDDIR}/distdir` -- one
/// symlink per file of the package's own `A` (`_prepare_fake_distdir`,
/// `prepare_build_dirs.py:518-549`) -- and the real directory in
/// `PORTAGE_ACTUAL_DISTDIR` (`config.py:3403-3409`). A distfile that is
/// in the real distdir but not in this package's `A` is not visible
/// through `DISTDIR`. Expected values: real Portage's source above, and
/// real `ebuild --skip-manifest <fixture> install` (Portage 3.0.82.2 on
/// the dev host, same two distfiles) writing the same four lines.
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
    // In the real distdir, not in this package's SRC_URI.
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

/// Real `doebuild.py:1463-1469`: the fake distdir is prepared for a
/// command other than `setup` unless `FEATURES=noauto`; a
/// `setup`-only chain, or any chain under `noauto` that has no
/// `install`/`unpack`, creates none.
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

/// `pretend` alone (the shortest real prerequisite chain -- see
/// `phase_prerequisites`) still exercises the full real
/// environment-setup + `bin/ebuild.sh`-sourcing + `__ebuild_main`
/// path without needing `src_install` at all, proving the slice
/// works even for an ebuild with no explicitly defined phases (every
/// phase function `phasepkg` doesn't define comes from real EAPI
/// defaults, ported here for free -- see this module's own doc
/// comment).
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

/// `FEATURES=compress-build-logs` pump round-trip (real
/// `EbuildPhase._open_log`'s `gzip.GzipFile(mode="ab")`): bytes
/// written to the sink's two handles land gzip-encoded in the
/// `.gz` file -- one member per `open_log_file` call, so a second
/// call appends a second member -- and decode back losslessly. A
/// plain path still writes through unencoded.
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
    // across the two pipes was never deterministic (real appends
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
