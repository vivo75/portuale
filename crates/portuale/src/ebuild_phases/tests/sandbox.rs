use super::*;

#[test]
fn sandbox_wrapped_command_configures_the_real_addrconfig_workaround_addresses() {
    // Portage: process.py, _configure_loopback_interface.
    // Non-loopback-looking addresses so getaddrinfo with AI_ADDRCONFIG
    // still resolves a build that only talks to localhost.
    let iso = Isolation {
        net: true,
        ipc: false,
        mount: false,
        pid: false,
        fs_sandbox: false,
    };
    let cmd = sandbox_wrapped_command(
        Path::new("/bin/true"),
        "install",
        iso,
        "[cat/pf-1.0] bash",
        false,
    );
    let shim = cmd
        .get_args()
        .filter_map(|a| a.to_str())
        .find(|a| a.contains("exec"))
        .expect("the bash -c shim argument is present");
    assert!(shim.contains("ip link set lo up"));
    assert!(shim.contains("ip addr add 10.0.0.1/8 dev lo"));
    assert!(shim.contains("ip -6 addr add fd::1/8 dev lo"));
}

#[test]
fn phase_opt_name_matches_portages_three_way_suffix() {
    // Portage: lib/portage/package/ebuild/doebuild.py, spawn (:2243-2261):
    // `[{mycpv}]` plus ` bash` when free, ` fakeroot` under fakeroot,
    // ` sandbox` otherwise (portuale has no fakeroot wrapper, so the
    // middle suffix never appears here).
    assert_eq!(phase_opt_name("cat", "pf-1.0", false), "[cat/pf-1.0] bash");
    assert_eq!(
        phase_opt_name("cat", "pf-1.0", true),
        "[cat/pf-1.0] sandbox"
    );
    // Portage's `[{CATEGORY}/{PF}]` fallback when `mycpv` is None:
    // empty stays empty, never invented.
    assert_eq!(phase_opt_name("", "", false), "[/] bash");
}

#[test]
fn sandbox_wrapped_command_sets_argv0_to_opt_name() {
    // Portage: lib/portage/package/ebuild/doebuild.py, spawn (:2243-2261);
    // lib/portage/process.py, _exec (:1027-1036):
    // `myargs = [opt_name] + mycommand[1:]`. `Command::arg0` has no
    // getter, so the child dumps its own top-level cmdline (`$$` stays
    // the main shell even inside subshells) and the test reads argv[0]
    // back from it.
    let tmp = TempDir::new("portuale-test-sandbox-argv0");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("cmdline");
    let script = tmp.join("dump.sh");
    std::fs::write(
        &script,
        format!("cp /proc/$$/cmdline \"{}\"\n", out.display()),
    )
    .unwrap();
    let opt_name = "[cat/pf-1.0] bash";
    let mut cmd =
        sandbox_wrapped_command(&script, "compile", Isolation::default(), opt_name, false);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let status = cmd.status().expect("the wrapped command spawns");
    assert!(status.success(), "the wrapped command exits 0");
    let raw = std::fs::read(&out).expect("the child dumps its own cmdline");
    let argv0 = raw.split(|&b| b == 0).next().unwrap();
    assert_eq!(argv0, opt_name.as_bytes());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn the_unshare_shim_carries_the_opt_name_as_the_wrapped_argv0() {
    // Portage has no `unshare(1)` wrapper process (it unshares via
    // `unshare(2)` in the child before `execve`), so the opt name reaches
    // the inner phase process via `exec -a`; the shim's own `$0` stays
    // `portuale-sandbox`, and the outer `unshare` keeps its own name.
    let iso = Isolation {
        net: true,
        ..Isolation::default()
    };
    let cmd = sandbox_wrapped_command(
        Path::new("/bin/true"),
        "compile",
        iso,
        "[cat/pf-1.0] sandbox",
        false,
    );
    let args: Vec<String> = cmd
        .get_args()
        .filter_map(|a| a.to_str().map(String::from))
        .collect();
    assert!(
        args.contains(&"portuale-sandbox".to_string()),
        "shim $0 stays portuale-sandbox: {args:?}"
    );
    let shim = args
        .iter()
        .find(|a| a.contains("exec"))
        .expect("the bash -c shim argument is present");
    assert!(
        shim.contains("exec -a '[cat/pf-1.0] sandbox' \"$@\""),
        "unexpected shim: {shim}"
    );
}

/// `SANDBOX_ACTIVE` in the parent env forces the ` bash` suffix even when
/// the sandbox wrapper applies (Portage `spawn`: `free or SANDBOX_ACTIVE`
/// in `os.environ`).
/// A subprocess per invocation: mutating process-global env vars is
/// unsound from parallel test threads.
#[test]
fn sandbox_active_in_the_parent_env_forces_the_bash_suffix() {
    // Portage: lib/portage/package/ebuild/doebuild.py, spawn (:2243-2261).
    const MARKER: &str = "PORTUALE_SANDBOX_ACTIVE_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ebuild_phases::tests::sandbox::sandbox_active_in_the_parent_env_forces_the_bash_suffix",
                "--nocapture",
            ])
            .env("SANDBOX_ACTIVE", "1")
            .env(MARKER, "1")
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
    // Without `sandbox` there is no wrapper process to name (the
    // constructor degrades to plain `bash`); nothing to assert here.
    if sandbox_binary().is_none() {
        eprintln!("skipping: /usr/bin/sandbox not present on this host");
        return;
    }
    let tmp = TempDir::new("portuale-test-sandbox-active-argv0");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("cmdline");
    let script = tmp.join("dump.sh");
    // The opt name lives on the `sandbox` wrapper itself (Portage execs
    // the wrapper with `argv[0] = opt_name`; the wrapper respawns the
    // inner bash with a plain `bash`), so dump the parent's cmdline.
    std::fs::write(
        &script,
        format!("cp /proc/$PPID/cmdline \"{}\"\n", out.display()),
    )
    .unwrap();
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            fs_sandbox: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&tmp.join("phase.log")),
        context: "sandbox-active probe",
        interactive: false,
        droppriv: None,
    })
    .expect("the sandboxed spawn runs");
    assert_eq!(status, 0, "the sandboxed spawn exits 0");
    let raw = std::fs::read(&out).expect("the child dumps the wrapper cmdline");
    let argv0 = raw.split(|&b| b == 0).next().unwrap();
    // Without the `SANDBOX_ACTIVE` fold this is `[cat/pf-1.0] sandbox`.
    assert_eq!(argv0, b"[cat/pf-1.0] bash");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A demoted spawn runs as the portage user: `id -u` through
/// `spawn_bash_script` with `droppriv: Some(ids)` reports the resolved uid.
/// Root-gated (demotion needs root) and skips when the portage user is
/// unknown — the same degrade-to-no-demotion shape as the spawn.
/// Portage: doebuild.py, spawn (:2171-2181); process.py, _exec (:1265-1272).
#[test]
fn demoted_spawn_runs_as_the_portage_user() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    let uid = ids.uid;
    let tmp = TempDir::new("portuale-test-droppriv-spawn");
    let script = tmp.join("id.sh");
    std::fs::write(&script, "id -u\n").unwrap();
    let log = tmp.join("id.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation::default(),
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "droppriv probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the demoted spawn runs");
    assert_eq!(status, 0, "the demoted spawn exits 0");
    let out = std::fs::read_to_string(&log).expect("the child writes its uid");
    assert_eq!(
        out.trim().parse::<u32>().expect("the uid parses"),
        uid,
        "the child runs as the portage user"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A demoted spawn applies Portage's build umask: the child reports `0022`
/// and creates a file `644` and a directory `755` even though the caller's
/// mask is the hostile `002`. A control spawn without demotion keeps the
/// caller's mask, and the caller's mask is untouched afterwards — so the
/// `022` can only come from the child's `pre_exec`.
/// Root-gated (demotion needs root) and skips when the portage user is
/// unknown, like `demoted_spawn_runs_as_the_portage_user`.
/// Portage: doebuild.py, spawn (:2171-2180); process.py, _exec (:1265-1274).
#[test]
fn demoted_spawn_applies_the_portage_umask() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    let tmp = TempDir::new("portuale-test-droppriv-umask");
    // The demoted child creates entries here, so it needs write access.
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&*tmp, std::fs::Permissions::from_mode(0o777))
            .expect("chmod the probe dir world-writable");
    }
    let script = tmp.join("umask.sh");
    std::fs::write(
        &script,
        format!(
            "umask\ncd '{}'\ntouch f\nmkdir d\nstat -c '%a %n' f d\n",
            tmp.display()
        ),
    )
    .expect("write the umask probe script");
    let control_script = tmp.join("control-umask.sh");
    std::fs::write(&control_script, "umask\n").expect("write the control probe script");

    // Hostile caller mask with save/restore: a child inheriting it would
    // report `0002`, so only `pre_exec` can produce `0022`.
    let before = unsafe { libc::umask(0o002) };
    struct Restore(libc::mode_t);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: `umask(2)` cannot fail.
            unsafe { libc::umask(self.0) };
        }
    }
    let restore = Restore(before);

    let log = tmp.join("demoted.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation::default(),
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "droppriv umask probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the demoted spawn runs");
    assert_eq!(status, 0, "the demoted spawn exits 0");

    let control_log = tmp.join("control.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &control_script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation::default(),
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&control_log),
        context: "control umask probe",
        interactive: false,
        droppriv: None,
    })
    .expect("the control spawn runs");
    assert_eq!(status, 0, "the control spawn exits 0");

    // End of the hostile window: the parent mask must be back.
    drop(restore);
    let cur = unsafe { libc::umask(before) };
    assert_eq!(cur, before, "the caller's umask is restored");

    let out = std::fs::read_to_string(&log).expect("the demoted child writes its umask");
    let mut lines = out.lines();
    assert_eq!(
        lines.next(),
        Some("0022"),
        "the demoted child gets Portage's build umask, not the caller's"
    );
    assert_eq!(lines.next(), Some("644 f"), "created files are 644");
    assert_eq!(lines.next(), Some("755 d"), "created directories are 755");

    let out = std::fs::read_to_string(&control_log).expect("the control child writes its umask");
    assert_eq!(
        out.trim(),
        "0002",
        "a non-demoted spawn keeps the caller's umask"
    );
}

/// End-to-end: a real Bash `setup` phase runs under `[<cat>/<pf>] bash`,
/// proving the `CATEGORY`/`PF` threading from the spawn site, not just
/// the constructor. A scratch ebuild in a `TempDir` (no fixture-tree
/// change): `pkg_setup` dumps its own top-level shell cmdline, so there
/// is no pid hunting and no sleep.
#[test]
fn a_real_phase_runs_under_the_cat_pf_opt_name() {
    // Portage: lib/portage/package/ebuild/doebuild.py, spawn (:2243-2261);
    // lib/portage/process.py, _exec (:1027-1036).
    let tmp = TempDir::new("portuale-test-phase-argv0");
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("pkg/dev-libs/argv0pkg");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    let ebuild_path = pkg_dir.join("argv0pkg-1.0.ebuild");
    std::fs::write(
        &ebuild_path,
        "EAPI=8\nSLOT=\"0\"\nS=\"${WORKDIR}\"\n\
         pkg_setup() { cp /proc/$$/cmdline \"${T}/cmdline\" || die; }\n",
    )
    .unwrap();
    let portage_tmpdir = tmp.join("tmp");
    let status = run_commands(RunCommandsArgs {
        ebuild_path: &ebuild_path,
        commands: &["setup"],
        root: Path::new("/"),
        portage_tmpdir: &portage_tmpdir,
        distdir: &portage_tmpdir.join("distfiles"),
        debug: false,
        config_root: Path::new("/dev/null/no-config-root"),
        shell: ShellBackend::Bash,
        log_file: PhaseLog::Inherit,
        skip_noop_src_phases: false,
        skip_pretend: false,
        // Empty, not absent: the ambient process env must not flip the
        // expected ` bash` suffix from under this test.
        build_env: &[("FEATURES".to_string(), String::new())],
    })
    .expect("run_commands should not itself error");
    assert_eq!(status, 0, "setup should exit successfully");
    let raw = std::fs::read(portage_tmpdir.join("portage/dev-libs/argv0pkg-1.0/temp/cmdline"))
        .expect("pkg_setup dumps its own cmdline");
    let argv0 = raw.split(|&b| b == 0).next().unwrap();
    assert_eq!(argv0, b"[dev-libs/argv0pkg-1.0] bash");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn network_sandbox_exempt_matches_portage_doebuild_spawns_own_formula() {
    assert!(network_sandbox_exempt("unpack", "", "live"));
    assert!(!network_sandbox_exempt("compile", "", "live"));
    assert!(!network_sandbox_exempt("test", "", "live"));

    assert!(network_sandbox_exempt("test", "", "test_network"));
    assert!(!network_sandbox_exempt("unpack", "", "test_network"));

    assert!(network_sandbox_exempt("unpack", "network-sandbox", ""));
    assert!(network_sandbox_exempt("compile", "network-sandbox", ""));
    assert!(network_sandbox_exempt("test", "network-sandbox", ""));

    assert!(!network_sandbox_exempt("unpack", "", ""));
    assert!(!network_sandbox_exempt("test", "", ""));

    // USE-conditional groups are already reduced away upstream.
    assert!(!network_sandbox_exempt("unpack", "", "foo? ( live )"));
}

/// The phase-execution gates read the resolved `FEATURES` from
/// `extra_env`, never the raw process env.
#[test]
fn feature_gates_use_the_resolved_features_string() {
    assert!(!feature_token_present("buildpkg", "sandbox"));
    assert!(feature_token_present("sandbox usersandbox", "sandbox"));
    assert!(!network_sandbox_requested("distlocks"));
    assert!(network_sandbox_requested("network-sandbox"));
    assert!(!fs_sandbox_requested(""));
    assert!(fs_sandbox_requested("usersandbox"));
    assert_eq!(
        features_string(&[("FEATURES".to_string(), "".to_string())]),
        ""
    );
    assert_eq!(
        features_string(&[
            ("FEATURES".to_string(), "raw".to_string()),
            ("FEATURES".to_string(), "resolved".to_string()),
        ]),
        "resolved"
    );
}

#[test]
fn sandbox_name_tables_are_one_plain_name_per_line() {
    for table in [SANDBOXED_SRC_PHASES, ENVIRON_WHITELIST] {
        assert!(table.ends_with('\n'));
        for line in table.lines() {
            assert!(!line.is_empty() && line.trim() == line, "bad line {line:?}");
        }
    }
    assert_eq!(SANDBOXED_SRC_PHASES.lines().count(), 6);
    assert_eq!(ENVIRON_WHITELIST.lines().count(), 158);
    assert!(listed(SANDBOXED_SRC_PHASES, "compile"));
    assert!(!listed(SANDBOXED_SRC_PHASES, "setup"));
    assert!(environ_whitelisted("HOME") && environ_whitelisted("CCACHE_DIR"));
    assert!(!environ_whitelisted("EMERGE_DEFAULT_OPTS"));
}

#[test]
fn droppriv_mirrors_portage_actionmap_and_gate() {
    use super::phase_droppriv as drop;
    let f = "sandbox usersandbox userpriv network-sandbox";
    // Portage actionmap: only these five phases demote.
    for p in ["unpack", "prepare", "configure", "compile", "test"] {
        assert!(drop(p, f, "", true), "{p}");
    }
    for p in [
        "setup", "install", "pretend", "clean", "merge", "qmerge", "config",
    ] {
        assert!(!drop(p, f, "", true), "{p}");
    }
    // Portage doebuild.py:1795 + :2200-2203 gate.
    assert!(!drop("unpack", "sandbox userpriv", "userpriv", true));
    assert!(!drop("unpack", "sandbox userpriv", "nouserpriv", true));
    assert!(!drop("unpack", "sandbox -userpriv", "", true));
    assert!(!drop("unpack", f, "", false)); // secpass < 2: not root
}

/// The build-user identity comes from `getpwnam`/`getgrouplist`, matching
/// Portage's `portage_uid`/`portage_gid`/`userpriv_groups`; a user that does
/// not exist degrades to `None` (the caller then runs undemoted).
#[test]
fn portage_phase_ids_resolves_a_real_user_and_rejects_unknown_ones() {
    // Portage: data.py, portage_uid (:169), userpriv_groups (:224-247).
    let root = portage_phase_ids(Some("root")).expect("root exists on any Linux host");
    assert_eq!(root.uid, 0, "Portage's getpwnam uid");
    assert!(
        root.groups.contains(&root.gid),
        "getgrouplist includes the primary gid: {:?}",
        root.groups
    );
    assert!(
        portage_phase_ids(Some("portuale-no-such-build-user")).is_none(),
        "an unknown build user must degrade, not panic"
    );
}

/// Portage `special_env_vars.py:256` (bug 978685): `environ_whitelist_re`
/// is `^(CCACHE_|DISTCC_|QEMU_).*`, so the empty `USE_EXPAND` variables
/// `QEMU_SOFTMMU_TARGETS`/`QEMU_USER_TARGETS` reach a binary merge's saved
/// environment. Expected values from the host's source and the L1 bash merge.
#[test]
fn environ_whitelist_keeps_qemu_prefix_like_portage_3_0_83() {
    for key in [
        "QEMU_USER_TARGETS",
        "QEMU_SOFTMMU_TARGETS",
        "QEMU_LD_PREFIX",
    ] {
        assert!(environ_whitelisted(key), "{key}");
    }
    assert!(!environ_whitelisted("QEMUX") && !environ_whitelisted("XQEMU_FOO"));
}

/// Portage `special_env_vars.py` adds four names to `environ_whitelist`,
/// and the two descriptors also to `env_blacklist`.
#[test]
fn environ_whitelist_has_the_portage_3_0_83_additions() {
    for name in [
        "USERLAND",
        "LOCPATH",
        "PORTAGE_EBUILD_EXIT_FD",
        "PORTAGE_IPC_ALIVE_FD",
    ] {
        assert!(
            environ_whitelisted(name),
            "{name} must pass the calling-env filter"
        );
    }
    for name in ["PORTAGE_EBUILD_EXIT_FD", "PORTAGE_IPC_ALIVE_FD"] {
        assert!(portage_profile::ENV_BLACKLIST.contains(&name));
    }
}

#[test]
fn post_phase_perms_force_owner_modes() {
    // Portage: util/__init__.py, apply_recursive_permissions (:1266-1340) with
    // dirmode 0o700 / filemode 0o600 and masks 0, via apply_secpass_permissions.
    // Mask 0 only ORs the owner bits in, so the fixtures start at mode 0 to make
    // the exact 700/600 result observable. Ids are the current user's, so the
    // chown is a no-op and the test passes unprivileged; the root-only chown to
    // portage lives in the Task 4 e2e.
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new("portuale-test-post-phase-perms");
    let temp = tmp.join("temp");
    let home = tmp.join("homedir");
    let sub = temp.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file = sub.join("f");
    std::fs::write(&file, "x").unwrap();
    let top_file = temp.join("top");
    std::fs::write(&top_file, "y").unwrap();

    // A symlink to a file outside the tree must neither be followed nor have its
    // target's mode changed.
    let outside = tmp.join("outside");
    std::fs::write(&outside, "z").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::os::unix::fs::symlink(&outside, sub.join("link")).unwrap();

    // Children first: a mode-0 `sub` could not be traversed to reach `file`.
    for p in [&file, &top_file, &sub, &temp] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    // SAFETY: `geteuid`/`getegid` cannot fail.
    let ids = PhaseIds {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        groups: Vec::new(),
    };
    apply_userpriv_post_perms(&temp, &home, &ids);

    let mode = |p: &Path| std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777;
    assert!(home.is_dir(), "the homedir is created before the walk");
    assert_eq!(mode(&temp), 0o700);
    assert_eq!(mode(&sub), 0o700);
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&top_file), 0o600);
    assert_eq!(mode(&outside), 0o644, "symlink target must not be touched");
}

#[test]
fn pre_phase_dirs_become_portage_owned() {
    // Portage: prepare_build_dirs.py, prepare_build_dirs (:96-106) and
    // _prepare_workdir (:329-368). Top-level ensure + lchown + owner-rwx OR
    // (mask 0 adds bits, never strips). Ids are the current user's, so the
    // chown is a no-op and the test passes unprivileged; the root-owned
    // handoff to portage lives in Task 4's e2e. WORKDIR starts at 0o755 so
    // the OR-add is observable: a chmod-set-to-700 would strip its
    // group/other bits.
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = TempDir::new("portuale-test-pre-phase-perms");
    let builddir = tmp.join("portage").join("cat").join("pf-1");
    for d in [&builddir, &builddir.join("work"), &builddir.join("temp")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let env = Environment {
        ebuild_abs: tmp.join("pf-1.ebuild"),
        pkg_dir: tmp.to_path_buf(),
        category: "cat".to_string(),
        split: PackageSplit {
            p: "pf-1".to_string(),
            pn: "pf".to_string(),
            pv: "1".to_string(),
            pr: "r0".to_string(),
            pvr: "1".to_string(),
            pf: "pf-1".to_string(),
        },
        eapi: "8".to_string(),
        portage_tmpdir: tmp.to_path_buf(),
        portage_builddir: builddir.clone(),
        inherited: None,
    };

    // Deepest first: a mode-0 parent cannot be traversed to reach its child.
    // WORKDIR keeps 0o755, the group/other-bit carrier proving the OR-add.
    for d in [
        env.t().join("logging"),
        env.t(),
        env.home(),
        env.workdir(),
        builddir.clone(),
        builddir.parent().unwrap().to_path_buf(),
    ] {
        std::fs::create_dir_all(&d).unwrap();
    }
    for (d, mode) in [
        (env.t().join("logging"), 0o000),
        (env.t(), 0o000),
        (env.home(), 0o000),
        (env.workdir(), 0o755),
        (builddir.clone(), 0o000),
        (builddir.parent().unwrap().to_path_buf(), 0o000),
    ] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    // SAFETY: `geteuid`/`getegid` cannot fail.
    let ids = PhaseIds {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        groups: Vec::new(),
    };
    apply_userpriv_pre_perms(&ids, &env);

    use std::os::unix::fs::MetadataExt as _;
    for d in [
        builddir.parent().unwrap().to_path_buf(),
        builddir.clone(),
        env.t(),
        env.home(),
        env.t().join("logging"),
    ] {
        let meta = std::fs::symlink_metadata(&d).unwrap();
        assert_eq!(
            meta.uid(),
            ids.uid,
            "{} owned by the build user",
            d.display()
        );
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            0o700,
            "{} gains owner rwx",
            d.display()
        );
    }
    // OR-add, not chmod-set: the 0o755 starter keeps its group/other bits.
    let work_meta = std::fs::symlink_metadata(env.workdir()).unwrap();
    assert_eq!(work_meta.uid(), ids.uid, "workdir owned by the build user");
    assert_eq!(
        work_meta.permissions().mode() & 0o7777,
        0o755,
        "workdir keeps its group/other bits"
    );
}

/// A spine entry that is a symlink is never created through or followed —
/// and neither is anything beneath it (a skipped `${T}` also skips
/// `${T}/logging`): the same no-follow rule `create_directories` applies to
/// filesdir. (The ownership test above only feeds real dirs, so this focused
/// case is what covers the guard in `apply_userpriv_pre_perms`.)
#[test]
fn pre_phase_symlinked_spine_entry_is_skipped_not_followed() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let tmp = TempDir::new("portuale-test-pre-phase-symlink");
    let builddir = tmp.join("portage").join("cat").join("pf-1");
    let env = Environment {
        ebuild_abs: tmp.join("pf-1.ebuild"),
        pkg_dir: tmp.to_path_buf(),
        category: "cat".to_string(),
        split: PackageSplit {
            p: "pf-1".to_string(),
            pn: "pf".to_string(),
            pv: "1".to_string(),
            pr: "r0".to_string(),
            pvr: "1".to_string(),
            pf: "pf-1".to_string(),
        },
        eapi: "8".to_string(),
        portage_tmpdir: tmp.to_path_buf(),
        portage_builddir: builddir.clone(),
        inherited: None,
    };

    // The link target exists but has no `logging` child: anything created
    // through the link would show up here.
    let outside = tmp.join("outside-temp");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Every other spine entry is a real dir starting at mode 0; `${T}` itself
    // is the symlink, so the guard — not `create_dir_all` — is what skips it
    // and its `logging` child. (The link goes in before the mode-0 chmods:
    // creating it inside a mode-0 builddir would fail.)
    for d in [
        env.workdir(),
        env.home(),
        builddir.clone(),
        builddir.parent().unwrap().to_path_buf(),
    ] {
        std::fs::create_dir_all(&d).unwrap();
    }
    std::os::unix::fs::symlink(&outside, env.t()).unwrap();
    for d in [
        env.workdir(),
        env.home(),
        builddir.clone(),
        builddir.parent().unwrap().to_path_buf(),
    ] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    // SAFETY: `geteuid`/`getegid` cannot fail.
    let ids = PhaseIds {
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        groups: Vec::new(),
    };
    apply_userpriv_pre_perms(&ids, &env);

    // The symlinked entry is left alone; its target is untouched and nothing
    // is created through the link.
    let link = std::fs::symlink_metadata(env.t()).unwrap();
    assert!(
        link.file_type().is_symlink(),
        "the symlinked entry is not replaced"
    );
    assert_eq!(
        std::fs::read_link(env.t()).unwrap(),
        outside,
        "the symlink still points at its target"
    );
    let mode = |p: &Path| std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&outside), 0o755, "symlink target must not be touched");
    assert!(
        !outside.join("logging").exists(),
        "nothing is created through the symlink"
    );
    // The real entries still gain owner rwx.
    for d in [
        builddir.parent().unwrap().to_path_buf(),
        builddir.clone(),
        env.workdir(),
        env.home(),
    ] {
        let meta = std::fs::symlink_metadata(&d).unwrap();
        assert_eq!(
            meta.uid(),
            ids.uid,
            "{} owned by the build user",
            d.display()
        );
        assert_eq!(
            meta.permissions().mode() & 0o7777,
            0o700,
            "{} gains owner rwx",
            d.display()
        );
    }
}

/// Portage unshares as root with `unshare(2)` and no new userns
/// (process.py:1061), so a root phase must not get `--map-root-user`
/// (which maps only the caller, leaving portage-owned paths as overflow
/// uid). Non-root keeps `--map-root-user` first, since it has no other way
/// to unshare. Pure: the bool is passed in, not read from `geteuid`.
#[test]
fn unshare_flags_root_omits_userns() {
    // Portage: process.py, _exec (:1061).
    let iso = Isolation {
        net: true,
        ..Isolation::default()
    };
    let root = iso.unshare_flags(true);
    assert!(!root.contains(&"--map-root-user"), "{root:?}");
    assert!(root.contains(&"--net"), "{root:?}");
    let user = iso.unshare_flags(false);
    assert_eq!(user.first(), Some(&"--map-root-user"), "{user:?}");
    assert!(user.contains(&"--net"), "{user:?}");
}
