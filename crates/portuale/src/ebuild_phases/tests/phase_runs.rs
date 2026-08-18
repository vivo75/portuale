use super::*;

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
