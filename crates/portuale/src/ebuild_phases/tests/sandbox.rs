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

#[cfg(target_os = "linux")]
#[test]
fn clone_flags_mirror_portages_unshare_bits() {
    use super::{Isolation, clone_flags_for};
    // Portage process.py:702-712 — net adds NEWUTS, pid adds NEWNS.
    let iso = Isolation {
        net: true,
        ipc: true,
        mount: true,
        pid: true,
        fs_sandbox: false,
    };
    let f = clone_flags_for(&iso);
    assert_ne!(f & libc::CLONE_NEWNET, 0);
    assert_ne!(f & libc::CLONE_NEWUTS, 0);
    assert_ne!(f & libc::CLONE_NEWIPC, 0);
    assert_ne!(f & libc::CLONE_NEWNS, 0);
    assert_ne!(f & libc::CLONE_NEWPID, 0);
    assert_eq!(
        f & libc::CLONE_NEWUSER,
        0,
        "never a userns on the demoted path"
    );
    assert_eq!(clone_flags_for(&Isolation::default()), 0);
}

// Cross-platform (no namespace APIs referenced — warn text only):
#[test]
fn degrade_message_names_the_demoted_phase() {
    let m = unshare_degrade_message("sandbox network-sandbox", "compile", true);
    assert!(
        m.contains("network-sandbox"),
        "names the requested isolation"
    );
    assert!(m.contains("compile"), "names the degraded phase");
    assert!(m.contains("portage"), "states DAC confinement survives");
    let plain = unshare_degrade_message("sandbox network-sandbox", "compile", false);
    assert!(
        !plain.contains("portage"),
        "non-demoted message claims nothing about ids"
    );
}

/// A demoted phase keeps `pid` alongside the other namespaces: the demoted
/// `pre_exec` fork dance (#635) re-expresses Portage's PID-1 re-exec
/// (`pid-ns-init`, process.py:1111-1145) without re-execing, so pid-sandbox
/// applies instead of dropping. Replicates the rule through the real
/// `phase_isolation` with a minimal Environment (outside any repo checkout,
/// so no RESTRICT/PROPERTIES exemption fires): demoted + pid-sandbox
/// requested ⇒ pid stays true with net/ipc/mount as requested.
#[test]
fn demoted_phase_isolation_keeps_pid_with_net() {
    let tmp = TempDir::new("portuale-test-demoted-pid-drop");
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
        portage_builddir: builddir,
        inherited: None,
    };
    let features = "sandbox userpriv network-sandbox ipc-sandbox mount-sandbox pid-sandbox";
    let iso = phase_isolation(&env, "compile", features, true, false);
    assert!(
        iso.pid,
        "pid-sandbox is kept for demoted phases (the fork dance reaps)"
    );
    assert!(iso.net, "net-sandbox is kept for demoted phases");
    assert!(iso.ipc, "ipc-sandbox is kept for demoted phases");
    assert!(iso.mount, "mount-sandbox is kept for demoted phases");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A demoted phase with network isolation has no egress: no default route in
/// `/proc/net/route` AND the S0 connect probe fails. The unisolated control
/// (same demoted spawn, no namespaces) shows the host route table, proving
/// the test observes the namespace, not the host. A connect that succeeds or
/// hangs (timeout rc 124 — packets left the host) counts as egress; only an
/// immediate failure counts as isolated.
/// Root-gated (demotion needs root) and skips when the portage user is
/// unknown, like `demoted_spawn_runs_as_the_portage_user`.
/// Portage: process.py, _exec (:693-714, :1084). Grounding: S0
/// `s0-egress-probe.log` (empty route table in a netns, NOEGRESS to
/// 9.9.9.9:80) against `s0-host-routes.log` (default route via eth0).
#[test]
fn demoted_phase_with_network_sandbox_has_no_egress() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    // Serial against the injected-failure hook (see `LOOPBACK_HOOK_LOCK`).
    let _hook = LOOPBACK_HOOK_LOCK.lock().unwrap();
    let tmp = TempDir::new("portuale-test-demoted-egress");
    let script = tmp.join("egress.sh");
    std::fs::write(
        &script,
        "if awk '$2 == \"00000000\" {found=1} END {exit !found}' /proc/net/route; then\n\
         echo \"EGRESS=1 reason=default-route-present\"\n\
         cat /proc/net/route\n\
         exit 1\n\
         fi\n\
         timeout 10 bash -c 'exec 3<>/dev/tcp/9.9.9.9/80' 2>/dev/null\n\
         rc=$?\n\
         if [ \"$rc\" -eq 0 ] || [ \"$rc\" -eq 124 ]; then\n\
         echo \"EGRESS=1 reason=connect-rc-$rc\"\n\
         exit 1\n\
         fi\n\
         echo \"EGRESS=0\"\n\
         exit 0\n",
    )
    .unwrap();
    let control_script = tmp.join("control-egress.sh");
    std::fs::write(
        &control_script,
        "if awk '$2 == \"00000000\" {found=1} END {exit !found}' /proc/net/route; then\n\
         echo \"CONTROL=host-routes-visible\"\n\
         exit 0\n\
         else\n\
         echo \"CONTROL=0 reason=no-default-route-without-isolation\"\n\
         exit 1\n\
         fi\n",
    )
    .unwrap();
    let iso = Isolation {
        net: true,
        ..Isolation::default()
    };
    let log = tmp.join("egress.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso,
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "egress probe",
        interactive: false,
        // Cloned: the control spawn below moves the original.
        droppriv: Some(PhaseIds {
            uid: ids.uid,
            gid: ids.gid,
            groups: ids.groups.clone(),
        }),
    })
    .expect("the isolated spawn runs");
    assert_eq!(status, 0, "the namespaced phase has no egress");
    let out = std::fs::read_to_string(&log).expect("the child writes its verdict");
    assert!(
        out.lines().any(|l| l == "EGRESS=0"),
        "unexpected probe output:\n{out}"
    );

    let control_log = tmp.join("control.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &control_script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation::default(),
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&control_log),
        context: "egress control probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the control spawn runs");
    assert_eq!(status, 0, "the unisolated spawn sees the host routes");
    let out = std::fs::read_to_string(&control_log).expect("the control writes its verdict");
    assert!(
        out.lines().any(|l| l == "CONTROL=host-routes-visible"),
        "unexpected control output:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A demoted+pid-sandboxed phase runs as PID 1 with a local `/proc` and the
/// portage uid: the demoted `pre_exec` fork dance (#635) re-expresses
/// Portage's `pid-ns-init` re-exec shape (process.py:1111-1145) without the
/// re-exec — the grandchild IS pid 1 by fork order, so it execs directly
/// while the middle supervises (forwards, reaps, re-raises). The multi-fork
/// script (two sequential forks plus a fork while a `sleep 5` sibling lives)
/// pins the ENOMEM fatal shape gone: without a reaper the first fork becomes
/// init, its exit tears the ns down, and later forks fail (proven in
/// `task-2-pid-gap.log`).
/// Root-gated and portage-user-gated like the egress test above.
/// Portage: process.py, _exec (:693-714, :1084); pid-ns-init.
#[test]
fn demoted_pid_ns_has_init_reaping_and_local_proc() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    let uid = ids.uid;
    let tmp = TempDir::new("portuale-test-demoted-pidns");
    let script = tmp.join("init.sh");
    std::fs::write(
        &script,
        "id -u\n\
         bash -c 'echo one' || exit 10\n\
         bash -c 'echo two' || exit 11\n\
         sleep 5 &\n\
         bash -c 'echo three-while-sibling-alive' || exit 12\n\
         wait || exit 13\n\
         readlink /proc/self/ns/pid\n\
         cat /proc/1/comm\n",
    )
    .unwrap();
    // Mount rides with pid per the flag table; the fresh `/proc` mount needs
    // the new mount ns.
    let iso = Isolation {
        pid: true,
        mount: true,
        ..Isolation::default()
    };
    let log = tmp.join("init.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso,
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "pid-init probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the pid-namespaced spawn runs");
    assert_eq!(
        status, 0,
        "sequential + sibling-alive forks all succeed (no ENOMEM)"
    );
    let out = std::fs::read_to_string(&log).expect("the child writes its verdict");
    let mut lines = out.lines();
    assert_eq!(
        lines
            .next()
            .unwrap_or("")
            .parse::<u32>()
            .expect("the uid parses"),
        uid,
        "the phase still demotes to the portage user"
    );
    let tail: Vec<&str> = lines.collect();
    assert!(
        tail.len() >= 5,
        "the phase prints its forks, ns and init comm:\n{out}"
    );
    assert_eq!(tail[0], "one");
    assert_eq!(tail[1], "two");
    assert_eq!(tail[2], "three-while-sibling-alive");
    let host_ns = std::fs::read_link("/proc/self/ns/pid").expect("the host pid ns reads");
    assert_ne!(
        tail[3],
        host_ns.to_string_lossy(),
        "the phase lives in its own pid ns"
    );
    assert_eq!(tail[4], "bash", "pid 1 of the fresh ns is the phase shell");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Direct children of `pid`, unioned over every thread's `children` file.
/// Used to find the pid-ns middle, which never execs (so it has no argv to
/// spot) but stays our direct child while it supervises.
fn task_children_of(pid: i32) -> Vec<i32> {
    let mut kids = Vec::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return kids;
    };
    for task in tasks.flatten() {
        let Ok(list) = std::fs::read_to_string(task.path().join("children")) else {
            continue;
        };
        kids.extend(
            list.split_whitespace()
                .filter_map(|p| p.parse::<i32>().ok()),
        );
    }
    kids.sort_unstable();
    kids.dedup();
    kids
}

/// The mount-ns id of `pid` (`None` when it just exited).
fn mount_ns(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/ns/mnt"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// This process's mount-ns id.
fn host_mnt() -> String {
    std::fs::read_link("/proc/self/ns/mnt")
        .expect("the host mount ns reads")
        .to_string_lossy()
        .into_owned()
}

/// Whether one of `pid`'s children is pid 1 of a pid ns: its `NSpid` status
/// line then holds the outer pid plus the inner 1. Only the dance's
/// grandchild — or pre-fix the phase's first fork — matches.
fn has_nsinit_child(pid: i32) -> bool {
    task_children_of(pid).iter().any(|&kid| {
        std::fs::read_to_string(format!("/proc/{kid}/status"))
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    line.strip_prefix("NSpid:").map(|rest| {
                        let ids: Vec<&str> = rest.split_whitespace().collect();
                        ids.len() > 1 && ids.last().is_some_and(|&n| n == "1")
                    })
                })
            })
            .unwrap_or(false)
    })
}

/// A TERM to the pid-ns middle reaches the phase: the middle blocks the
/// forward set pre-fork and `sigtimedwait`-forwards each caught signal to the
/// grandchild (process.py:1132; parent-side forward by pid, :961-962),
/// re-expressed without Python. Proved behaviourally because
/// `spawn_bash_script` returns `Result<i32>` (signal death maps to 1 either
/// way): the phase traps TERM into `exit 7`, so a forwarding middle yields
/// rc 7 while a swallowing one yields a join timeout plus an orphan.
/// The supervisor pid comes from the session id (`ps -o sid=`): sessions are
/// not namespaced, the middle owns its session via its own `setsid`, and
/// `$PPID` is unusable — the middle lives outside the new pid ns, so from
/// inside `getppid()` reports 0. Two gates stand before the kill: the target
/// is not the phase itself (pre-fix there is no middle, so the sid IS the
/// phase pid), and the target lives in a fresh mount ns (never the harness).
/// Root-gated and portage-user-gated like the tests above.
/// Portage: process.py, _exec (:1111-1190); pid-ns-init:36-43, :121-149.
#[test]
fn demoted_pid_ns_forwards_fatal_signals() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    let tmp = TempDir::new("portuale-test-demoted-pidfwd");
    // The demoted phase itself writes its pid files here, so the directory
    // needs write access (mirrors the umask probe above).
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&*tmp, std::fs::Permissions::from_mode(0o777))
            .expect("chmod the probe dir world-writable");
    }
    let self_file = tmp.join("self.pid");
    let ns_file = tmp.join("ns");
    let script = tmp.join("trap.sh");
    // The 1 s sleep loop (not a single `sleep 60`): bash defers a trapped
    // signal arriving during a foreground external command until that command
    // completes, so one long sleep would answer only at its natural expiry.
    std::fs::write(
        &script,
        format!(
            "trap 'exit 7' TERM\n\
             echo $$ > '{}'\n\
             readlink /proc/self/ns/pid > '{}'\n\
             while true; do sleep 1; done\n",
            self_file.display(),
            ns_file.display(),
        ),
    )
    .unwrap();
    let log = tmp.join("trap.log");
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let status = spawn_bash_script(SpawnBashScriptArgs {
            script: &script,
            argv_word: "compile",
            vars: Vec::new(),
            iso: Isolation {
                pid: true,
                mount: true,
                ..Isolation::default()
            },
            category: "cat",
            pf: "pf-1.0",
            log_file: PhaseLog::Capture(&log),
            context: "pid-forward probe",
            interactive: false,
            droppriv: Some(ids),
        })
        .expect("the pid-namespaced spawn runs");
        let _ = tx.send(status);
    });
    // Join with a timeout: the trap answers within about a second of the
    // forwarded TERM (one `sleep 1` at most), so 30 s is generous. On failure
    // the whole dance family dies by pid sweep below, so no detached
    // portage-owned sleeper outlives the test.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let (slf, ns) = loop {
        let read = (
            std::fs::read_to_string(&self_file),
            std::fs::read_to_string(&ns_file),
        );
        // Non-empty: the shell creates each file (empty) before the producing
        // command runs, so bare readability races a partial write.
        if let (Ok(slf), Ok(ns)) = read
            && !(slf.trim().is_empty() || ns.trim().is_empty())
        {
            break (slf, ns);
        }
        if std::time::Instant::now() > deadline {
            panic!("the phase never wrote its pid files");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    // The phase's children enter the new pid ns (their first fork becomes
    // init); this documents that the unshare happened, but it cannot gate
    // the kill below — pre-fix it passes too, while the signal target stays
    // in the host ns.
    let host_ns = std::fs::read_link("/proc/self/ns/pid").expect("the host pid ns reads");
    assert_ne!(
        ns.trim(),
        host_ns.to_string_lossy(),
        "the phase's children enter the new pid ns"
    );
    let slf: i32 = slf.trim().parse().expect("the phase pid parses");
    // The supervisor is this process's child: it never execs (no argv to
    // spot), so identify it positively — a fresh mount ns plus a child that
    // is pid 1 of a pid ns (the dance's grandchild; pre-fix the phase's first
    // fork, which is why gate one below still trips there). Neither `$PPID`
    // nor the session id can cross the pid ns from inside: the middle lives
    // outside it, so both read back 0 there.
    let middle = loop {
        let mut found = None;
        for pid in task_children_of(std::process::id() as i32) {
            if mount_ns(pid).is_some_and(|m| m != host_mnt()) && has_nsinit_child(pid) {
                found = Some(pid);
                break;
            }
        }
        if let Some(pid) = found {
            break pid;
        }
        if std::time::Instant::now() > deadline {
            panic!("no supervising middle appeared among this process's children");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    // Gate one: the target is not the phase itself. Pre-fix there is no
    // middle, so the session id IS the phase pid and signalling it would
    // merely kill the phase directly — a vacuous rc-7 pass. Post-fix the
    // middle supervises from outside the pid ns, hence outside `$$`'s domain.
    assert_ne!(
        middle, slf,
        "safety gate: signal the supervisor, never the phase itself"
    );
    // Gate two: the target lives in a fresh mount ns (NEWNS rides with pid
    // per the flag table) — never the harness or its session leader. A
    // pid-ns sample from inside the phase cannot gate this: pre-fix the
    // children already report the new pid ns while the target stays home.
    // (`middle` was found through exactly this check, so this re-reads it
    // right before the kill rather than trusting the scan.)
    let middle_mnt = mount_ns(middle).expect("the supervisor's mount ns reads");
    assert_ne!(
        middle_mnt,
        host_mnt(),
        "safety gate: signal only a supervisor in its own mount ns, never the harness"
    );
    // SAFETY: both gates above proved the target is the namespaced
    // supervisor, a child this test's spawn created.
    unsafe {
        libc::kill(middle, libc::SIGTERM);
    }
    match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(status) => {
            handle.join().expect("the spawn thread joins");
            assert_eq!(
                status, 7,
                "the trap converted the forwarded TERM into the phase's own exit"
            );
        }
        Err(_) => {
            // A swallowing middle leaves the sleeper alive: SIGKILL the whole
            // dance family by pid. Group-kill cannot do this — `kill(-pgid)`
            // only reaches the caller's own pid ns, so it would stop at the
            // middle — and `slf` is pid 1 of the new ns (host pid 1 from
            // here), so only host-side pids discovered down from the middle
            // may be signalled.
            // SAFETY: every pid here comes from this test's own spawn subtree
            // (or is the gated supervisor itself).
            for _ in 0..3 {
                unsafe {
                    libc::kill(middle, libc::SIGKILL);
                    for kid in task_children_of(middle) {
                        libc::kill(kid, libc::SIGKILL);
                        for grandkid in task_children_of(kid) {
                            libc::kill(grandkid, libc::SIGKILL);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let status = rx.recv().expect("the spawn reaps after the kills");
            handle.join().expect("the spawn thread joins");
            panic!("the forwarded TERM never landed (reaped {status})");
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// NON-ISOLATING control: a demoted spawn with `usersandbox` in FEATURES
/// stays demoted (`id -u` is still the portage uid), so the `usersandbox`
/// path + the #627 `free` rule (demoted without `usersandbox` drops the
/// wrapper) compose with demotion with and without namespaces. Deliberately
/// requests no namespaces: pre-fix the demoted shim demoted-then-unshared,
/// so any namespace request failed the whole spawn (exit 1 — the bug Task 1
/// fixed); only the namespace-free combo passes on both sides of the fix.
/// It guards the combo, not the confinement.
/// Root-gated and portage-user-gated like the tests above.
/// Portage: doebuild.py, spawn (:2233-2261, the `free` rule).
#[test]
fn demoted_usersandbox_spawn_stays_demoted() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    let uid = ids.uid;
    let tmp = TempDir::new("portuale-test-demoted-usersandbox");
    let script = tmp.join("id.sh");
    std::fs::write(&script, "id -u\n").unwrap();
    let log = tmp.join("id.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: vec![(
            "FEATURES".to_string(),
            "sandbox usersandbox userpriv network-sandbox".to_string(),
        )],
        iso: Isolation {
            fs_sandbox: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "usersandbox probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the usersandbox spawn runs");
    assert_eq!(status, 0, "the usersandbox spawn exits 0");
    let out = std::fs::read_to_string(&log).expect("the child writes its uid");
    assert_eq!(
        out.trim().parse::<u32>().expect("the uid parses"),
        uid,
        "the child still runs as the portage user"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The demoted-netns loopback builders mirror Portage's netlink bytes exactly
/// (`nlmsghdr "=IHHII"`, `ifinfomsg`/`ifaddrmsg` native-aligned, `rtattr "HH"`).
/// Byte-identical to CPython's `Struct.pack` on this host (checked against the
/// `netlink.py` oracle while writing); the asserts pin header fields and
/// address bytes, not padding. Sizes differ from the first sketch (v4 32 not
/// 40, v6 44 not 52): `create_rtattr`/`create_nlmsg` concatenate with no
/// alignment padding, so the layout demands the smaller totals.
/// Portage: lib/portage/util/netlink.py, structs (:30-34),
/// set_link_up (:102-106), add_address (:108-114).
#[cfg(target_os = "linux")]
#[test]
fn lo_netlink_messages_mirror_portage_bytes() {
    fn u16ne(b: &[u8], off: usize) -> u16 {
        u16::from_ne_bytes([b[off], b[off + 1]])
    }
    fn u32ne(b: &[u8], off: usize) -> u32 {
        u32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
    }

    let (get, ng) = lo_getlink_req();
    assert_eq!(ng, 38, "nlmsghdr (16) + ifinfomsg (16) + rtattr (6)");
    assert_eq!(u32ne(&get, 0), ng as u32, "nlmsg_len covers the datagram");
    assert_eq!(u16ne(&get, 4), 18, "RTM_GETLINK");
    assert_eq!(u16ne(&get, 6), 0x1, "NLM_F_REQUEST");
    assert_eq!(u32ne(&get, 8), 1, "seq 1");
    assert_eq!(u32ne(&get, 12), 0, "pid 0");
    assert_eq!(get[16], 0, "AF_UNSPEC");
    assert_eq!(u16ne(&get, 32), 6, "rta_len");
    assert_eq!(u16ne(&get, 34), 3, "IFLA_IFNAME");
    assert_eq!(&get[36..38], b"lo", "ifname lo");

    let (up, n) = lo_link_up_msg(1);
    assert_eq!(n, 32, "nlmsghdr (16) + ifinfomsg (16)");
    assert_eq!(u32ne(&up, 0), n as u32, "nlmsg_len covers the datagram");
    assert_eq!(u16ne(&up, 4), 16, "RTM_NEWLINK");
    assert_eq!(u16ne(&up, 6), 0x5, "NLM_F_REQUEST|NLM_F_ACK");
    assert_eq!(u32ne(&up, 8), 1, "seq 1");
    assert_eq!(u32ne(&up, 12), 0, "pid 0");
    assert_eq!(up[16], 0, "AF_UNSPEC");
    assert_eq!(u16ne(&up, 18), 0, "ifi_type 0");
    assert_eq!(u32ne(&up, 20), 1, "ifindex");
    assert_eq!(u32ne(&up, 24), 1, "ifi_flags IFF_UP");
    assert_eq!(u32ne(&up, 28), 1, "ifi_change IFF_UP");

    let (v4, n4) = lo_add_v4_msg(1);
    assert_eq!(n4, 32, "nlmsghdr (16) + ifaddrmsg (8) + rtattr (8)");
    assert_eq!(u32ne(&v4, 0), n4 as u32, "nlmsg_len covers the datagram");
    assert_eq!(u16ne(&v4, 4), 20, "RTM_NEWADDR");
    assert_eq!(u16ne(&v4, 6), 0x605, "REQ|ACK|EXCL|CREATE");
    assert_eq!(v4[16], 2, "AF_INET");
    assert_eq!(v4[17], 8, "prefixlen 8");
    assert_eq!(u32ne(&v4, 20), 1, "ifindex");
    assert_eq!(u16ne(&v4, 24), 8, "rta_len");
    assert_eq!(u16ne(&v4, 26), 2, "IFA_LOCAL");
    assert_eq!(
        &v4[28..32],
        &[10, 0, 0, 1],
        "the AI_ADDRCONFIG workaround address"
    );

    let (v6, n6) = lo_add_v6_msg(1);
    assert_eq!(n6, 44, "nlmsghdr (16) + ifaddrmsg (8) + rtattr (20)");
    assert_eq!(u32ne(&v6, 0), n6 as u32, "nlmsg_len covers the datagram");
    assert_eq!(u16ne(&v6, 4), 20, "RTM_NEWADDR");
    assert_eq!(u16ne(&v6, 6), 0x605, "REQ|ACK|EXCL|CREATE");
    assert_eq!(v6[16], 10, "AF_INET6");
    assert_eq!(v6[17], 8, "prefixlen 8");
    assert_eq!(u32ne(&v6, 20), 1, "ifindex");
    assert_eq!(u16ne(&v6, 24), 20, "rta_len");
    assert_eq!(u16ne(&v6, 26), 2, "IFA_LOCAL");
    assert_eq!(
        &v6[28..44],
        &[0x00, 0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01],
        "fd::1"
    );
}

/// Probe script shared by the two loopback spawn tests: the child binds and
/// listens on 0.0.0.0, then completes a real TCP round-trip (connect, accept,
/// one message each way) against 127.0.0.1 AND 10.0.0.1. Binding all-local
/// (not 127.0.0.1) is load-bearing: a 127.0.0.1-bound listener RSTs a
/// 10.0.0.1 connect (ECONNREFUSED, reproduced standalone), so only 0.0.0.0
/// lets the 10.0.0.1 leg prove the address is configured — with no default
/// route a non-local 10.0.0.1 fails ENETUNREACH instead. `assert_pid1` adds
/// the PID-1 check for the pid-ns variant. The tail re-asserts the #630
/// no-egress property (no default route, external connect fails), which must
/// survive lo-up.
fn loopback_probe_script(assert_pid1: bool) -> String {
    let pid1 = if assert_pid1 {
        "if [ \"$$\" != 1 ]; then echo \"PID1=0 got-$$\"; exit 1; fi\necho \"PID1=1\"\n"
    } else {
        ""
    };
    // NOTE: no `\`-continuations below: they strip leading whitespace and
    // would de-indent the Python block (IndentationError). Every `\n` and
    // every Python indent space is an explicit escape.
    format!(
        "{pid1}\
        if awk '$2 == \"00000000\" {{found=1}} END {{exit !found}}' /proc/net/route; then\n\
        echo \"ISOLATED=0 reason=default-route-present\"\n\
        exit 1\n\
        fi\n\
        python3 - <<'PYEOF'\n\
        import socket\n\
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n\
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)\n\
        srv.bind(('0.0.0.0', 0))\n\
        srv.listen(1)\n\
        port = srv.getsockname()[1]\n\
        srv.settimeout(15)\n\
        for addr in ('127.0.0.1', '10.0.0.1'):\n\
        \x20   c = socket.create_connection((addr, port), timeout=15)\n\
        \x20   conn, _ = srv.accept()\n\
        \x20   conn.sendall(b'ping')\n\
        \x20   data = c.recv(4)\n\
        \x20   assert data == b'ping', (addr, data)\n\
        \x20   c.close()\n\
        \x20   conn.close()\n\
        print('LOOPBACK=1')\n\
        PYEOF\n\
        if [ \"$?\" -ne 0 ]; then\n\
        echo \"LOOPBACK=0 reason=python-probe-failed\"\n\
        exit 1\n\
        fi\n\
        timeout 10 bash -c 'exec 3<>/dev/tcp/9.9.9.9/80' 2>/dev/null\n\
        rc=$?\n\
        if [ \"$rc\" -eq 0 ] || [ \"$rc\" -eq 124 ]; then\n\
        echo \"ISOLATED=0 reason=connect-rc-$rc\"\n\
        exit 1\n\
        fi\n\
        echo \"ISOLATED=1\"\n\
        exit 0\n"
    )
}

/// A demoted phase in a fresh netns reaches localhost over TCP: listener +
/// connect in the same spawn against 127.0.0.1 and 10.0.0.1, with the #630
/// no-egress property surviving lo-up. Per the S0 ruling, flags (0x9 in BOTH
/// states) and bind (succeeds either way) are NOT discriminators — only the
/// connect is, so this test asserts connects and nothing else would pass it.
/// Root-gated (demotion needs root) and skips when the portage user is
/// unknown, like `demoted_spawn_runs_as_the_portage_user`.
/// Portage: process.py, _exec (:693-714, :1084),
/// _configure_loopback_interface (:838-871); netlink.py (:18-114).
/// Grounding: S0 `s0-lo-down-probe.log` (CONNECT-FAIL ENETUNREACH, lo down)
/// vs `s0-portage-lo-oracle.log` (CONNECT-OK after Portage configures lo).
#[test]
fn demoted_netns_loopback_accepts_localhost() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    // Serial against the injected-failure hook (see `LOOPBACK_HOOK_LOCK`).
    let _hook = LOOPBACK_HOOK_LOCK.lock().unwrap();
    let tmp = TempDir::new("portuale-test-demoted-loopback");
    let script = tmp.join("loopback.sh");
    std::fs::write(&script, loopback_probe_script(false)).unwrap();
    let log = tmp.join("loopback.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            net: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "loopback probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the isolated spawn runs");
    assert_eq!(status, 0, "localhost connects and egress stays blocked");
    let out = std::fs::read_to_string(&log).expect("the child writes its verdict");
    assert!(
        out.lines().any(|l| l == "LOOPBACK=1"),
        "real TCP round-trip over lo:\n{out}"
    );
    assert!(
        out.lines().any(|l| l == "ISOLATED=1"),
        "no default route, no egress:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The same localhost connect, executed as PID 1: lo was configured pre-fork
/// in `pre_exec` (the netns is shared, so the pid-1 grandchild inherits it).
/// Root-gated and portage-user-gated like the net-only test above.
/// Portage: process.py, _exec (:693-714, :1084, :1111-1145 pid-ns-init);
/// _configure_loopback_interface (:838-871).
#[test]
fn demoted_pid_netns_loopback_works_in_pid1() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    // Serial against the injected-failure hook (see `LOOPBACK_HOOK_LOCK`).
    let _hook = LOOPBACK_HOOK_LOCK.lock().unwrap();
    let tmp = TempDir::new("portuale-test-demoted-pid-loopback");
    let script = tmp.join("pid-loopback.sh");
    std::fs::write(&script, loopback_probe_script(true)).unwrap();
    let log = tmp.join("pid-loopback.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            net: true,
            pid: true,
            mount: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "pid loopback probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the pid-namespaced spawn runs");
    assert_eq!(status, 0, "pid 1 connects over lo and egress stays blocked");
    let out = std::fs::read_to_string(&log).expect("the child writes its verdict");
    assert!(
        out.lines().any(|l| l == "PID1=1"),
        "the phase runs as pid 1:\n{out}"
    );
    assert!(
        out.lines().any(|l| l == "LOOPBACK=1"),
        "real TCP round-trip over lo:\n{out}"
    );
    assert!(
        out.lines().any(|l| l == "ISOLATED=1"),
        "no default route, no egress:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Serializes the `PORTUALE_TEST_LOOPBACK_FAIL` hook against every other
/// demoted+net spawn in this module: the hook is process-global and read by
/// the parent at spawn time, so a parallel loopback/egress probe would
/// otherwise observe the injected failure mid-flight. Root runs go serial
/// (`--test-threads=1`) per the `demoted_` convention; the lock holds for
/// the parallel full-suite case.
static LOOPBACK_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Saves `PORTUALE_TEST_LOOPBACK_FAIL` on entry and restores it on drop, so
/// an assertion panic cannot leak the injected failure into another test.
struct RestoreLoopbackFail {
    saved: Option<std::ffi::OsString>,
}

impl RestoreLoopbackFail {
    fn set(value: Option<&str>) -> Self {
        let saved = std::env::var_os("PORTUALE_TEST_LOOPBACK_FAIL");
        // SAFETY: the caller holds `LOOPBACK_HOOK_LOCK`; no other test in
        // this binary touches the hook while it is held.
        unsafe {
            match value {
                Some(v) => std::env::set_var("PORTUALE_TEST_LOOPBACK_FAIL", v),
                None => std::env::remove_var("PORTUALE_TEST_LOOPBACK_FAIL"),
            }
        }
        Self { saved }
    }
}

impl Drop for RestoreLoopbackFail {
    fn drop(&mut self) {
        // SAFETY: same locked env mutation as `set` (restoring the value).
        unsafe {
            match self.saved.take() {
                Some(value) => std::env::set_var("PORTUALE_TEST_LOOPBACK_FAIL", value),
                None => std::env::remove_var("PORTUALE_TEST_LOOPBACK_FAIL"),
            }
        }
    }
}

/// An injected loopback-setup failure warns in the phase log while the phase
/// still runs: with `PORTUALE_TEST_LOOPBACK_FAIL=linkup` the child configures
/// nothing (no 10.0.0.1 local route in fib_trie — proving the v4 request
/// never ran after the link-up failure, i.e. abort-rest), rc stays 0
/// (best-effort), and the `Capture` log carries Portage's warning as its
/// first line (written child-side from `pre_exec` straight to fd 2, before
/// the phase itself emits anything). The clean control then shows
/// the warning absent, localhost connecting, and fib_trie holding the
/// 10.0.0.1 pair — the control that makes the injected absence meaningful
/// rather than vacuous.
/// Root-gated (demotion needs root) and skips when the portage user is
/// unknown, like `demoted_spawn_runs_as_the_portage_user`.
/// Portage: process.py, _configure_loopback_interface (:838-871).
/// Grounding: S0 `s0-fib-trie-probe.log` (0 hits in a fresh netns; the
/// `|-- <addr>` + `host LOCAL` local-route shape from the host); S0
/// `s0-fd2-preexec.log` (fd 2 in `pre_exec` is the phase stderr).
#[cfg(target_os = "linux")]
#[test]
fn demoted_loopback_failure_warns_and_continues() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    // Serial: the hook is process-global (see `LOOPBACK_HOOK_LOCK`).
    let _hook = LOOPBACK_HOOK_LOCK.lock().unwrap();
    let tmp = TempDir::new("portuale-test-demoted-loopback-warn");
    let script = tmp.join("no-lo.sh");
    std::fs::write(
        &script,
        r#"if grep -A1 -E '^[[:space:]]*[|]-- 10\.0\.0\.1$' /proc/net/fib_trie | grep -q 'host LOCAL'; then
echo "V4_ROUTE=1 reason=abort-rest-violated"
exit 1
fi
echo "V4_ROUTE=0"
exit 0
"#,
    )
    .unwrap();
    let _env = RestoreLoopbackFail::set(Some("linkup"));
    let log = tmp.join("injected.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            net: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "loopback warn probe",
        interactive: false,
        droppriv: Some(ids.clone()),
    })
    .expect("the injected spawn runs");
    assert_eq!(status, 0, "the phase still runs when loopback setup fails");
    let out = std::fs::read_to_string(&log).expect("the child writes its verdict");
    assert!(
        out.lines().any(|l| l == "V4_ROUTE=0"),
        "no 10.0.0.1 route after the aborted setup:\n{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.starts_with("Unable to configure loopback interface: ")),
        "Portage's warning lands in the phase log:\n{out}"
    );
    assert!(
        out.lines()
            .next()
            .is_some_and(|l| l.starts_with("Unable to configure loopback interface: ")),
        "the warning heads the log, written pre-exec before phase output:\n{out}"
    );
    drop(_env);

    let control_script = tmp.join("lo-ok.sh");
    let presence = r#"if grep -A1 -E '^[[:space:]]*[|]-- 10\.0\.0\.1$' /proc/net/fib_trie | grep -q 'host LOCAL'; then
echo "V4_ROUTE=1"
else
echo "V4_ROUTE=0 reason=no-local-route"
exit 1
fi
"#;
    std::fs::write(
        &control_script,
        presence.to_string() + &loopback_probe_script(false),
    )
    .unwrap();
    let control_log = tmp.join("control.log");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &control_script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            net: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&control_log),
        context: "loopback control probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the control spawn runs");
    assert_eq!(status, 0, "localhost connects and egress stays blocked");
    let out = std::fs::read_to_string(&control_log).expect("the child writes its verdict");
    assert!(
        out.lines().any(|l| l == "V4_ROUTE=1"),
        "the clean setup configures the 10.0.0.1 route:\n{out}"
    );
    assert!(
        out.lines().any(|l| l == "LOOPBACK=1"),
        "real TCP round-trip over lo:\n{out}"
    );
    assert!(
        !out.lines()
            .any(|l| l.starts_with("Unable to configure loopback interface: ")),
        "no warning on the success path:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// `loopback_warn_table` is Portage's warning text exactly: the fixed prefix,
/// the `strerror` body, one trailing newline. Only the stable prefix and the
/// single newline are asserted (the C-locale wording of the body is assumed).
/// The table covers errnos 1..=135 with a `const` fallback past the end.
/// Portage: process.py, _configure_loopback_interface (:867-871).
#[cfg(target_os = "linux")]
#[test]
fn loopback_warn_table_matches_portage_text() {
    let table = loopback_warn_table();
    assert_eq!(table.len(), 135, "errnos 1..=135");
    let line = &table[libc::ENOENT as usize - 1];
    assert!(
        line.starts_with(b"Unable to configure loopback interface: "),
        "Portage-identical prefix: {line:?}"
    );
    assert!(
        line.ends_with(b"\n") && !line[..line.len() - 1].contains(&b'\n'),
        "one trailing newline: {line:?}"
    );
    assert!(
        !line.windows(b"(os error".len()).any(|w| w == b"(os error"),
        "bare strerror, not io::Error Display (no \" (os error N)\"): {line:?}"
    );
    assert!(
        LOOPBACK_WARN_FALLBACK.starts_with(b"Unable to configure loopback interface: "),
        "the fallback keeps the prefix: {LOOPBACK_WARN_FALLBACK:?}"
    );
}

/// The child's table lookup (`configure_loopback_netlink` caller in
/// `pre_exec`): errno `0` and errnos past the table (`200`) must both fall
/// through to the `const` fallback, not index out of range. The expression
/// is replicated here verbatim because the closure itself is unreachable
/// from a unit test.
#[cfg(target_os = "linux")]
#[test]
fn loopback_warn_lookup_falls_back_out_of_range() {
    let table = loopback_warn_table();
    let lookup = |loopback_errno: u8| -> &[u8] {
        loopback_errno
            .checked_sub(1)
            .and_then(|idx| table.get(idx as usize))
            .map(Vec::as_slice)
            .unwrap_or(LOOPBACK_WARN_FALLBACK)
    };
    assert_eq!(lookup(0), LOOPBACK_WARN_FALLBACK, "errno 0 falls back");
    assert_eq!(lookup(200), LOOPBACK_WARN_FALLBACK, "errno 200 falls back");
    assert_eq!(lookup(libc::ENOENT as u8), table[libc::ENOENT as usize - 1]);
}

/// The injected loopback warning survives a `.gz` phase log: with
/// `PORTUALE_TEST_LOOPBACK_FAIL=linkup` and `Capture` on a `build.log.gz`
/// path, the child-side `write(2)` flows through the gzip pump as part of
/// the member (never a raw tail append), so the member decodes to end and
/// carries the warning. This pins the #639 regression shape: a post-wait
/// parent append would land past the member as trailing garbage.
/// Root-gated and portage-user-gated like the plain-log test above.
/// Portage: process.py, _configure_loopback_interface (:838-871).
#[cfg(target_os = "linux")]
#[test]
fn demoted_loopback_warning_survives_gzip_log() {
    use std::io::Read as _;
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("SKIP: needs root");
        return;
    }
    let Some(ids) = portage_phase_ids(None) else {
        eprintln!("SKIP: no portage user");
        return;
    };
    // Serial: the hook is process-global (see `LOOPBACK_HOOK_LOCK`).
    let _hook = LOOPBACK_HOOK_LOCK.lock().unwrap();
    let tmp = TempDir::new("portuale-test-demoted-loopback-gz");
    let script = tmp.join("no-lo.sh");
    std::fs::write(&script, "echo PHASE_RAN=1\nexit 0\n").unwrap();
    let _env = RestoreLoopbackFail::set(Some("linkup"));
    let log = tmp.join("build.log.gz");
    let status = spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: "compile",
        vars: Vec::new(),
        iso: Isolation {
            net: true,
            ..Isolation::default()
        },
        category: "cat",
        pf: "pf-1.0",
        log_file: PhaseLog::Capture(&log),
        context: "loopback gz probe",
        interactive: false,
        droppriv: Some(ids),
    })
    .expect("the injected spawn runs");
    assert_eq!(status, 0, "the phase still runs when loopback setup fails");
    let raw = std::fs::read(&log).expect("the gz log exists");
    let mut decoded = String::new();
    flate2::read::MultiGzDecoder::new(&raw[..])
        .read_to_string(&mut decoded)
        .expect("the member decodes to end, no trailing-garbage corruption");
    assert!(
        decoded
            .lines()
            .any(|l| l.starts_with("Unable to configure loopback interface: ")),
        "Portage's warning is inside the decoded member:\n{decoded}"
    );
    assert!(
        decoded.lines().any(|l| l == "PHASE_RAN=1"),
        "phase output shares the member:\n{decoded}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
