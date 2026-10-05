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

/// `has_version`/`best_version` in a phase go through the tracked
/// `bin/portageq-wrapper` shim to the native `portuale portageq`.
#[test]
fn phase_has_version_uses_native_portageq() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let fx = fx.canonicalize().unwrap();
    let tmp = TempDir::new("ebuild-phases-test-native-portageq").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/hvpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let out = tmp.join("out");
    let ebuild = pkg_dir.join("hvpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        format!(
            "EAPI=8\nSLOT=\"0\"\npkg_pretend() {{\n\
             \thas_version -r dev-libs/samepkg && echo yes >> {o} || echo no >> {o}\n\
             \thas_version -r nonexistent/package && echo yes >> {o} || echo no >> {o}\n\
             \techo \"best=$(best_version -r dev-libs/samepkg)\" >> {o}\n\
             \techo \"bin=${{PORTUALE_BIN}}\" >> {o}\n}}\n",
            o = out.display()
        ),
    )
    .unwrap();
    let root = fx.clone();
    let portage_tmpdir = tmp.join("tmp");

    let portuale_bin = crate::self_exe::built_portuale_bin();

    // Only the sqlite block below adds variants.
    #[cfg_attr(not(feature = "vdb-sqlite"), allow(unused_mut))]
    let mut variants: Vec<Vec<(&str, String)>> = vec![vec![]];
    #[cfg(feature = "vdb-sqlite")]
    {
        let db = tmp.join("vdb.sqlite");
        let src = portage_vdb::FilesDb::new(&fx);
        let dst = portage_vdb::SqliteDb::open(&db).unwrap();
        portage_vdb::copy_all(&src, &dst, false).unwrap();
        // An empty ROOT-owned database: samepkg is only "installed" on
        // the files tree, so a native helper reading the wrong backend
        // would answer differently.
        variants.push(vec![
            ("PORTUALE_VDB_BACKEND", "sqlite".to_string()),
            ("PORTUALE_VDB_PATH", db.display().to_string()),
            ("PORTUALE_VDB_ROOT", fx.display().to_string()),
        ]);
        let empty = tmp.join("empty.sqlite");
        drop(portage_vdb::SqliteDb::open(&empty).unwrap());
        variants.push(vec![
            ("PORTUALE_VDB_BACKEND", "sqlite".to_string()),
            ("PORTUALE_VDB_PATH", empty.display().to_string()),
            ("PORTUALE_VDB_ROOT", fx.display().to_string()),
        ]);
    }
    for (i, vdb) in variants.iter().enumerate() {
        let _ = std::fs::remove_file(&out);
        // Fresh builddir per variant: a finished phase leaves its
        // `.pretended` marker behind and is then skipped.
        let portage_tmpdir = portage_tmpdir.join(i.to_string());
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let mut c = std::process::Command::new(&portuale_bin);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("ROOT", &root)
            .env("PORTAGE_TMPDIR", &portage_tmpdir)
            .args(["ebuild", ebuild.to_str().unwrap(), "pretend"]);
        for (k, v) in vdb {
            c.env(k, v);
        }
        let o = c.output().expect("portuale ebuild spawns");
        assert!(
            o.status.success(),
            "variant {i}: {:?}\n{}",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        );
        let got = std::fs::read_to_string(&out).unwrap_or_else(|e| {
            panic!(
                "variant {i}: no output ({e})\n{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
        });
        let lines: Vec<&str> = got.lines().collect();
        let empty_db = i == 2;
        assert_eq!(lines[0], if empty_db { "no" } else { "yes" }, "{got}");
        assert_eq!(lines[1], "no", "{got}");
        if empty_db {
            assert_eq!(lines[2], "best=", "{got}");
        } else {
            assert!(lines[2].starts_with("best=dev-libs/samepkg-"), "{got}");
        }
        assert_eq!(lines[3], format!("bin={}", portuale_bin.display()), "{got}");
    }
}

/// Replacing the running binary's file mid-run must not break the
/// later phases.
#[test]
fn replacing_the_running_binary_mid_run_keeps_later_phases_working() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let fx = fx.canonicalize().unwrap();
    let tmp = TempDir::new("ebuild-phases-test-self-replace").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/selfreplacepkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let out = tmp.join("out");
    std::fs::write(
        pkg_dir.join("selfreplacepkg-1.0.ebuild"),
        format!(
            "EAPI=8\nSLOT=\"0\"\nS=\"${{WORKDIR}}\"\n\
             pkg_setup() {{\n\
             \tcp \"${{PORTUALE_BIN}}\" \"${{PORTUALE_BIN}}.new\" || die\n\
             \tmv -f \"${{PORTUALE_BIN}}.new\" \"${{PORTUALE_BIN}}\" || die\n}}\n\
             src_unpack() {{\n\
             \techo \"bin=${{PORTUALE_BIN}}\" >> {o}\n\
             \thas_version -r dev-libs/samepkg && echo yes >> {o} || echo no >> {o}\n}}\n",
            o = out.display()
        ),
    )
    .unwrap();

    let release = crate::self_exe::built_portuale_bin();
    let bin = tmp.join("bin/portuale");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    // An external `cp`, not `std::fs::copy`: a forked sibling holding the
    // copy fd open would fail exec with ETXTBSY.
    let cp = std::process::Command::new("cp")
        .arg(&release)
        .arg(&bin)
        .status()
        .unwrap();
    assert!(cp.success());

    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let o = std::process::Command::new(&bin)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("ROOT", &fx)
        .env("PORTAGE_TMPDIR", &portage_tmpdir)
        .args([
            "ebuild",
            pkg_dir.join("selfreplacepkg-1.0.ebuild").to_str().unwrap(),
            "unpack",
        ])
        .output()
        .expect("portuale ebuild spawns");
    let got = std::fs::read_to_string(&out).unwrap_or_default();
    assert!(
        o.status.success(),
        "{:?}\n{got}\n{}",
        o.status,
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        got,
        format!("bin={}\nyes\n", bin.display()),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// `PORTAGE_IPC_DAEMON=1` in the calling environment must not reach
/// the phase, so `has_version` keeps going through `portageq-wrapper`.
#[test]
fn portage_ipc_daemon_in_the_calling_env_never_reaches_a_phase() {
    let tmp = TempDir::new("ebuild-phases-test-no-ipc-daemon").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/ipcpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("ipcpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\npkg_setup() {\n\
         \techo \"ipc=${PORTAGE_IPC_DAEMON:-unset}\"\n\
         \thas_version sys-apps/definitely-nonexistent-xyz && echo \"rc=0\" || echo \"rc=1\"\n\
         }\n",
    )
    .unwrap();
    let portuale_bin = crate::self_exe::built_portuale_bin();
    // `has_version` without `-r` reads the vdb at ROOT: an existing
    // but empty tree answers 1 (a missing one exits 64).
    std::fs::create_dir_all(tmp.join("root/var/db/pkg")).unwrap();
    let out = std::process::Command::new(&portuale_bin)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("ROOT", tmp.join("root"))
        .env("PORTAGE_TMPDIR", tmp.join("tmp"))
        .env("PORTAGE_IPC_DAEMON", "1")
        .args(["ebuild", ebuild.to_str().unwrap(), "setup"])
        .output()
        .expect("portuale ebuild spawns");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(all.contains("ipc=unset"), "{all}");
    assert!(all.contains("rc=1"), "{all}");
    assert!(!all.contains("ebuild-ipc"), "{all}");
}

/// `$PORTUALE_BIN __helper ping` from inside a phase prints the fixed
/// token, proving it is a working binary.
#[test]
fn portuale_bin_runs_helper_ping_inside_a_phase() {
    let tmp = TempDir::new("ebuild-phases-test-ping").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/pingpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("pingpkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\npkg_setup() {\n\
         \t\"${PORTUALE_BIN}\" __helper ping\n\
         }\n",
    )
    .unwrap();
    let portuale_bin = crate::self_exe::built_portuale_bin();
    let out = std::process::Command::new(&portuale_bin)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("ROOT", tmp.join("root"))
        .env("PORTAGE_TMPDIR", tmp.join("tmp"))
        .args(["ebuild", ebuild.to_str().unwrap(), "setup"])
        .output()
        .expect("portuale ebuild spawns");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(all.contains(crate::helpers::PING_TOKEN), "{all}");
}

/// `PORTAGE_PYTHON` in the phase env is the `portuale-python` shim, or
/// `/usr/bin/python` with `PORTUALE_PYTHON_HELPERS` set (also forwarded).
#[test]
fn portage_python_is_the_shim_unless_real_mode() {
    let tmp = TempDir::new("ebuild-phases-test-portage-python").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/pypkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("pypkg-1.0.ebuild");
    std::fs::write(
        &ebuild,
        "EAPI=8\nSLOT=\"0\"\npkg_setup() {\n\
         \techo \"py=${PORTAGE_PYTHON}\"\n\
         \techo \"helpers=${PORTUALE_PYTHON_HELPERS:-unset}\"\n\
         }\n",
    )
    .unwrap();
    let portuale_bin = crate::self_exe::built_portuale_bin();
    let run = |helpers: Option<&str>, tag: &str| {
        // Fresh builddir per run: a finished phase leaves its marker
        // behind and is then skipped (so the second run would print
        // nothing).
        let portage_tmpdir = tmp.join(format!("tmp-{tag}"));
        let mut c = std::process::Command::new(&portuale_bin);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("ROOT", tmp.join("root"))
            .env("PORTAGE_TMPDIR", &portage_tmpdir)
            .args(["ebuild", ebuild.to_str().unwrap(), "setup"]);
        if let Some(h) = helpers {
            c.env("PORTUALE_PYTHON_HELPERS", h);
        }
        c.output().expect("portuale ebuild spawns")
    };
    let out = run(None, "native");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.lines()
            .any(|l| l.starts_with("py=") && l.ends_with("/portuale-python")),
        "{all}"
    );
    assert!(all.lines().any(|l| l == "helpers=unset"), "{all}");
    let out = run(Some("real"), "real");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(all.lines().any(|l| l == "py=/usr/bin/python"), "{all}");
    assert!(all.lines().any(|l| l == "helpers=real"), "{all}");
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
