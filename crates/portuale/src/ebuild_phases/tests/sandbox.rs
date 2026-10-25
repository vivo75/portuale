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
    let cmd = sandbox_wrapped_command(Path::new("/bin/true"), "install", iso, "[cat/pf-1.0] bash");
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
    let mut cmd = sandbox_wrapped_command(&script, "compile", Isolation::default(), opt_name);
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
    })
    .expect("the sandboxed spawn runs");
    assert_eq!(status, 0, "the sandboxed spawn exits 0");
    let raw = std::fs::read(&out).expect("the child dumps the wrapper cmdline");
    let argv0 = raw.split(|&b| b == 0).next().unwrap();
    // Without the `SANDBOX_ACTIVE` fold this is `[cat/pf-1.0] sandbox`.
    assert_eq!(argv0, b"[cat/pf-1.0] bash");
    let _ = std::fs::remove_dir_all(&tmp);
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

/// Portage 3.0.83 `special_env_vars.py:256` (`f7bfe34de`, bug 978685):
/// `environ_whitelist_re` is `^(CCACHE_|DISTCC_|QEMU_).*`, so the empty
/// `USE_EXPAND` variables `QEMU_SOFTMMU_TARGETS`/`QEMU_USER_TARGETS` reach a
/// binary merge's saved environment. Expected values from the host's source
/// and the L1 bash merge (`LLM/evidence/2026-10-24-vdb-env/`).
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

/// Portage 3.0.83 `special_env_vars.py` (`5dc9b552e`, `c15ab2464`, `b7052a33d`,
/// `1bad86d92`) adds four names to `environ_whitelist`, and the two
/// descriptors also to `env_blacklist`.
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
