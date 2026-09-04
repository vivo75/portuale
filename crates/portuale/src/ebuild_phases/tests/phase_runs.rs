use super::*;

#[test]
fn features_test_passthrough_actually_runs_src_test() {
    // A subprocess per invocation: mutating process-global env vars is
    // unsound from parallel test threads.
    let tmp = TempDir::new("ebuild-phases-test-features_test_passthrough").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/srctestpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("srctestpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\nS=\"${WORKDIR}\"\nsrc_test() { touch \"${T}/test-ran\" || die; }\n",
    )
    .unwrap();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let marker = portage_tmpdir.join("portage/dev-libs/srctestpkg-1.0/temp/test-ran");

    // `CARGO_BIN_EXE_<name>` is unset for unit tests: the shared helper
    // derives the sibling binary and fails fast when it is missing or stale.
    let portuale_bin = crate::self_exe::built_portuale_bin();

    let run = |features: &str| {
        let _ = std::fs::remove_file(&marker);
        let status = std::process::Command::new(&portuale_bin)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("ROOT", &root)
            .env("PORTAGE_TMPDIR", &portage_tmpdir)
            .env("FEATURES", features)
            .args(["ebuild", ebuild.to_str().unwrap(), "test"])
            .status()
            .expect("portuale ebuild spawns");
        assert!(status.success(), "FEATURES={features:?}: {status:?}");
    };

    // Portage: phase-functions.sh, __dyn_test (`src_test` is a no-op
    // unless `FEATURES` contains `test`).
    run("");
    assert!(!marker.exists(), "src_test ran without FEATURES=test");

    run("test");
    assert!(marker.exists(), "src_test did not run with FEATURES=test");
}

/// With no checkout a phase still runs with no `PORTAGE_PYM_PATH`
/// complaint; `setup` reaches the variable twice, so it is the probe.
#[test]
fn ebuild_setup_without_a_checkout_has_no_pym_path_complaint() {
    // A subprocess per run: each gets its own environment.
    // `PORTUALE_BIN_DIR` is left unset so
    // the normal runtime (build tree or embedded) is used.
    let tmp = TempDir::new("ebuild-phases-test-setup-without-checkout").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/setuppkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("setuppkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nS=\"${WORKDIR}\"\n",
    )
    .unwrap();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let out = std::process::Command::new(portuale_exe())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("ROOT", &root)
        .env("PORTAGE_TMPDIR", &portage_tmpdir)
        .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
        .args(["ebuild", ebuild.to_str().unwrap(), "setup"])
        .output()
        .expect("portuale ebuild spawns");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "ebuild setup failed: {all}");
    for bad in ["PORTAGE_PYM_PATH does not exist", "null directory"] {
        assert!(!all.contains(bad), "{bad:?} in {all}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A phase run over an eclass scope larger than the pipe buffer does
/// not deadlock; a regression fails on a deadline instead of hanging.
#[test]
fn install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer() {
    let ebuild_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo/dev-libs/bigeclasspkg/bigeclasspkg-1.0.ebuild");
    let portage_tmpdir = TempDir::new(&format!(
        "ebuild-phases-test-{}-{}",
        std::process::id(),
        "install_does_not_deadlock_on_an_eclass_scope_larger_than_the_pipe_buffer"
    ));
    let _ = std::fs::remove_dir_all(&portage_tmpdir);

    let (tx, rx) = std::sync::mpsc::channel();
    let thread_ebuild_path = ebuild_path.clone();
    let thread_portage_tmpdir = portage_tmpdir.to_path_buf();
    std::thread::spawn(move || {
        let result = run_commands(RunCommandsArgs {
            ebuild_path: &thread_ebuild_path,
            commands: &["install"],
            root: Path::new("/"),
            portage_tmpdir: &thread_portage_tmpdir,
            distdir: &thread_portage_tmpdir.join("distfiles"),
            debug: false,
            config_root: Path::new("/dev/null/no-config-root"),
            shell: ShellBackend::Brush,
            build_env: &[],
        });
        let _ = tx.send(result);
    });
    // A generous deadline for parallel runs; a pipe-buffer deadlock
    // would hang indefinitely and still be caught.
    let status = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("run_commands should complete within the deadline, not deadlock")
        .expect("run_commands should not itself error");
    assert_eq!(status, 0, "install should exit successfully");

    let marker =
        portage_tmpdir.join("portage/dev-libs/bigeclasspkg-1.0/temp/bigfixture-marker.txt");
    let observed = std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("{} should have been written: {e}", marker.display()));
    assert_eq!(observed, "hello from bigfixture.eclass\n");

    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}
