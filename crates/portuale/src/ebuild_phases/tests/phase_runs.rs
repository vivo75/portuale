use super::*;

#[test]
fn a_missing_phase_runtime_is_an_error_not_a_panic_for_remote_and_ebuild() {
    let tmp = TempDir::new("missing-phase-runtime-e2e");
    let far = tmp.join("far");
    std::fs::create_dir_all(far.join("var/db/pkg")).unwrap();
    let gpkg = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar");
    let nowhere = tmp.join("nowhere");
    let assert_clean_failure = |what: &str, out: &std::process::Output| {
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(1), "{what}: {all}");
        assert!(
            all.contains(BIN_DIR_VAR) && all.contains("ebuild.sh"),
            "{what}: {all}"
        );
        for bad in ["panicked", "backtrace", "RUST_BACKTRACE"] {
            assert!(!all.contains(bad), "{what}: {bad:?} in {all}");
        }
    };

    let out = std::process::Command::new(portuale_exe())
        .env(BIN_DIR_VAR, &nowhere)
        .args(["mrg", "--getbinpkgonly", "--remote-hostname=localhost"])
        .args(["--remote-transport=local"])
        .arg(format!("--remote-root={}", far.display()))
        .arg(format!("--remote-binpkg={}", gpkg.display()))
        .output()
        .unwrap();
    assert_clean_failure("mrg --remote-binpkg", &out);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("Remote preflight"),
        "the check must run before the client is contacted"
    );
    let touched: Vec<_> = std::fs::read_dir(far.join("var/db/pkg")).unwrap().collect();
    assert!(
        touched.is_empty(),
        "nothing may reach the client ROOT: {touched:?}"
    );

    let pkg_dir = tmp.join("pkg/dev-libs/p");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("p-1.0.ebuild");
    std::fs::write(&ebuild, "EAPI=8\nSLOT=\"0\"\n").unwrap();
    let out = std::process::Command::new(portuale_exe())
        .env(BIN_DIR_VAR, &nowhere)
        .env("ROOT", far.to_str().unwrap())
        .env("PORTAGE_TMPDIR", tmp.join("pt"))
        .args(["ebuild", ebuild.to_str().unwrap(), "setup"])
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(out.status.code(), Some(0), "{all}");
    assert_ne!(out.status.code(), Some(134), "must not abort: {all}");
    assert!(all.contains(BIN_DIR_VAR), "{all}");
    assert!(!all.contains("panicked"), "{all}");
}

#[test]
fn features_test_passthrough_actually_runs_src_test() {
    // A real subprocess invocation, not `std::env::set_var` --
    // `run_commands`'s own doc comment already establishes that
    // mutating process-global env vars is unsound from parallel
    // test threads. Spawning the real `portuale` binary gives each
    // invocation its own independent environment, exactly the way
    // real portage's own `FEATURES` behavior is env-var-scoped.
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

    // `CARGO_BIN_EXE_<name>` is only set for `tests/` integration
    // targets, not for a unit test compiled into the crate's own
    // binary -- derive the sibling `portuale` binary's path from
    // this very test binary's own path instead (`target/<profile>/
    // deps/portuale-<hash>` -> `target/<profile>/portuale`).
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");

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

    // Real `__dyn_test` (`phase-functions.sh:559`): `src_test` is a
    // no-op ("Test phase [not enabled]") unless `FEATURES` contains
    // `test` -- previously always the case here regardless of the
    // real, outer `FEATURES` the invoker actually asked for, since
    // the phase's own exported `FEATURES` was unconditionally
    // blanked (`phase_features_value`'s own doc comment).
    run("");
    assert!(!marker.exists(), "src_test ran without FEATURES=test");

    run("test");
    assert!(marker.exists(), "src_test did not run with FEATURES=test");
}

/// feat#157 S6 / #151: `has_version`/`best_version` in a real phase
/// go through the tracked `bin/portageq-wrapper` shim to the native
/// `portuale portageq`, on the files tree and (with the feature) on a
/// sqlite database, whether or not `3rdparty/portage` is present.
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

    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");

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

/// #331: replacing the running binary's file mid-run must not break
/// the later phases. `pkg_setup` atomically replaces `PORTUALE_BIN`
/// (a private copy of the release binary, the way `cargo build` or a
/// merge of a new portuale does), then `src_unpack` calls
/// `has_version` through `portageq-wrapper`. Before the fix the
/// second phase got `PORTUALE_BIN=<path> (deleted)` from
/// `current_exe()` and the shim died with `No such file or
/// directory`. Real Portage has no self-exec: its helpers are files
/// under `PORTAGE_BIN_PATH` that a portage merge replaces in place,
/// and later phases run the new files, which is what a path resolved
/// once gives too.
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

    let mut release = std::env::current_exe().expect("current test exe");
    release.pop();
    if release.ends_with("deps") {
        release.pop();
    }
    release.push("portuale");
    let bin = tmp.join("bin/portuale");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    // An external `cp`, not `std::fs::copy`: a sibling test thread that
    // forks while this process holds the copy open for writing would
    // keep that fd until its exec, and exec'ing the copy then fails
    // with ETXTBSY.
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

/// #326 S2/D6: `PORTAGE_IPC_DAEMON=1` in the calling environment must
/// not reach the phase, so `has_version` keeps going through
/// `portageq-wrapper` (`phase-helpers.sh:911-915`) and answers for
/// real. (Without the drop, the phase would take the `ebuild-ipc`
/// branch and die on its 127.)
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
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");
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

/// #326 S2/D8: `$PORTUALE_BIN __helper ping` from inside a phase
/// prints the fixed token (proves `PORTUALE_BIN` is a working
/// binary, never the libtest harness).
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
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");
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

/// #326 S2.3/D2: `PORTAGE_PYTHON` in the phase env is the
/// `portuale-python` shim in native mode and `/usr/bin/python` with
/// `PORTUALE_PYTHON_HELPERS=real` (which is also forwarded).
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
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");
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

/// #326 S2 relocated e2e (the #322 shape): the phase runtime comes
/// from the embedded copy (`PORTUALE_BIN_DIR` at an extracted tree,
/// which is what a relocated binary extracts; in-tree an unset
/// `PORTUALE_BIN_DIR` would resolve to the live tree instead) with
/// `PORTUALE_PORTAGE_CHECKOUT=/nonexistent`, so every helper the
/// phases reach is native or vendored. `unpack` of an odd-mode
/// tarball must leave the real-Portage oracle modes (the
/// `helper-unpack` probe shape), and `docompress` must compress
/// through the vendored `ecompress-file`.
#[test]
fn relocated_unpack_and_docompress_use_only_native_or_vendored_helpers() {
    let tmp = TempDir::new("ebuild-phases-test-relocated-helpers").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let bin = tmp.join("rt/bin");
    crate::embedded_runtime::extract("bin", &bin).unwrap();
    let pkg_dir = tmp.join("pkg/dev-libs/relocpkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild = pkg_dir.join("relocpkg-1.0.ebuild");
    // No `dodoc`/`doins`: those route through `doins.py`, which has
    // no native port yet (a later slice); `cp` keeps this probe on
    // the S2 helpers only, like the pmtest `helper-unpack` and
    // `helper-docompress` probes.
    std::fs::write(
        &ebuild,
        "EAPI=8\nDESCRIPTION=\"test\"\nSLOT=\"0\"\nS=\"${WORKDIR}\"\n\
         src_unpack() {\n\
         \tlocal t=\"${T}/odd-src\"\n\
         \tmkdir -p \"${t}/sub0700\" \"${t}/sub0775\" || die\n\
         \tprintf 'mode0600\\n' > \"${t}/file0600\" || die\n\
         \tchmod 0600 \"${t}/file0600\" || die\n\
         \tprintf '#!/bin/sh\\necho hi\\n' > \"${t}/file0777\" || die\n\
         \tchmod 0777 \"${t}/file0777\" || die\n\
         \tchmod 0700 \"${t}/sub0700\" || die\n\
         \tchmod 0775 \"${t}/sub0775\" || die\n\
         \tprintf 'nested\\n' > \"${t}/sub0700/nested0640\" || die\n\
         \tchmod 0640 \"${t}/sub0700/nested0640\" || die\n\
         \ttar -cf \"${T}/odd.tar\" -C \"${t}\" file0600 file0777 sub0700 sub0775 || die\n\
         \tcd \"${WORKDIR}\" || die\n\
         \tunpack \"${T}/odd.tar\"\n\
         \tstat -c '%a %n' file0600 file0777 sub0700 sub0700/nested0640 sub0775 \\\n\
         \t\t> \"${T}/modes.txt\" || die\n\
         }\n\
         src_install() {\n\
         \tlocal src=\"${WORKDIR}/docsrc\"\n\
         \tmkdir -p \"${src}\" || die\n\
         \tlocal i=1\n\
         \twhile [[ ${i} -le 80 ]]; do\n\
         \t\tprintf 'payload line %04d abcdefghijklmnopqrstuvwxyz\\n' \"${i}\" \\\n\
         \t\t\t>> \"${src}/BIG-doc.txt\" || die\n\
         \t\ti=$(( i + 1 ))\n\
         \tdone\n\
         \tmkdir -p \"${ED}/usr/share/doc/${PF}\" || die\n\
         \tcp \"${src}/BIG-doc.txt\" \"${ED}/usr/share/doc/${PF}/\" || die\n\
         \tdocompress /usr/share/doc/${PF}\n\
         }\n",
    )
    .unwrap();
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");
    let portage_tmpdir = tmp.join("tmp");
    let run = |phase: &str| {
        std::process::Command::new(&portuale_bin)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("ROOT", tmp.join("root"))
            .env("PORTAGE_TMPDIR", &portage_tmpdir)
            .env("PORTAGE_COMPRESS", "bzip2")
            .env("FEATURES", "binpkg-docompress")
            .env(crate::ebuild_phases::BIN_DIR_VAR, &bin)
            .env("PORTUALE_PORTAGE_CHECKOUT", "/nonexistent")
            .args(["ebuild", ebuild.to_str().unwrap(), phase])
            .output()
            .expect("portuale ebuild spawns")
    };
    fn find(root: &Path, name: &str) -> PathBuf {
        let mut hits = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                if e.file_name().to_string_lossy() == name {
                    hits.push(e.path());
                } else if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(e.path());
                }
            }
        }
        assert_eq!(hits.len(), 1, "expected one {name}: {hits:?}");
        hits.pop().unwrap()
    }
    let out = run("unpack");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    // A missing `chmod-lite` is silent in real (`find:` on stderr,
    // exit unexamined) and leaves odd modes: both halves are pinned.
    assert!(!all.contains("No such file"), "{all}");
    let modes = std::fs::read_to_string(find(&portage_tmpdir, "modes.txt")).unwrap();
    assert_eq!(
        modes.lines().collect::<Vec<_>>(),
        [
            "644 file0600",
            "755 file0777",
            "755 sub0700",
            "644 sub0700/nested0640",
            "755 sub0775",
        ],
        "{modes}"
    );
    let out = run("install");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0), "{all}");
    // A missing `ecompress-file` is fatal in real (`__helpers_die`);
    // pin both the absence of its error and the compressed result.
    assert!(!all.contains("No such file"), "{all}");
    assert!(!all.contains("ecompress-file"), "{all}");
    let bz2 = find(&portage_tmpdir, "BIG-doc.txt.bz2");
    assert!(!bz2.parent().unwrap().join("BIG-doc.txt").exists());
}

/// Backlog #326 S1, env-level: with no Portage checkout, a real phase
/// still runs without any `PORTAGE_PYM_PATH` complaint -- without
/// `PORTAGE_PYM_PATH does not exist` or `null directory`.
///
/// `clean` would be the obvious probe (real `__dyn_clean`,
/// `bin/phase-functions.sh:316`, `cd`s into it unconditionally at
/// `:331-332` upstream / `:401-402` vendored), but standalone
/// `portuale ebuild … clean` is a recognized-but-unimplemented dry-run
/// stub (`ebuild.rs`'s own test names it) that never executes a phase.
/// `setup` is implemented and reaches the same variable twice: real
/// `bin/ebuild.sh`'s own top-level cwd choice, and the end-of-phase
/// env-save `cd "${PORTAGE_PYM_PATH}"` (vendored
/// `bin/phase-functions.sh:1260`, upstream `:1189`).
#[test]
fn ebuild_setup_without_a_checkout_has_no_pym_path_complaint() {
    // A real subprocess invocation, not `std::env::set_var` (see
    // `features_test_passthrough_actually_runs_src_test`): each run
    // gets its own environment. `PORTUALE_BIN_DIR` is left unset so
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

/// Regression test for a real upstream brush bug (fixed in the pinned
/// fork, see LLM/what-this-proves.md's eclass section for the full writeup):
/// `bigfixture.eclass` defines ~400 functions so that real
/// `bin/phase-functions.sh`'s own post-phase `__save_ebuild_env |
/// __filter_readonly_variables` pipe (both sides real shell
/// functions) carries well over the OS pipe buffer size (~64KiB on
/// Linux) worth of `declare -f` output. Before the fix, brush ran a
/// function used as a non-last pipeline stage inline rather than as
/// a background task, so the pipeline-spawning loop blocked on
/// `__save_ebuild_env` fully returning before even spawning
/// `__filter_readonly_variables` to drain it -- a real, reproducible
/// deadlock, not a slow completion (confirmed against real
/// app-arch/xz-utils and sys-fs/fuse before the fix, which both
/// inherit the real `multilib` eclass family). Run on a background
/// thread with a hard deadline so a regression here fails this test
/// outright instead of hanging the whole suite.
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
        let result = run_commands(
            &thread_ebuild_path,
            &["install"],
            Path::new("/"),
            &thread_portage_tmpdir,
            &thread_portage_tmpdir.join("distfiles"),
            false,
            Path::new("/dev/null/no-config-root"),
            ShellBackend::Brush,
            &[],
        );
        let _ = tx.send(result);
    });
    // A generous deadline: this drives a full brush phase chain (many
    // subprocesses), so under a heavily parallel `cargo test` it can
    // legitimately take tens of seconds -- a real pipe-buffer
    // *deadlock* would still hang indefinitely and be caught.
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
