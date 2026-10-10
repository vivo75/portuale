use super::*;

#[test]
fn sandbox_wrapped_command_configures_the_real_addrconfig_workaround_addresses() {
    // Real `_configure_loopback_interface` (`process.py:838-871`,
    // bug #690758): `ip link set lo up` plus a real, non-loopback-
    // looking address per family, so glibc's `getaddrinfo()` with
    // `AI_ADDRCONFIG` set doesn't return spuriously empty results
    // for a build that only ever talks to `localhost`.
    let iso = Isolation {
        net: true,
        ipc: false,
        mount: false,
        pid: false,
        fs_sandbox: false,
    };
    let cmd = sandbox_wrapped_command(Path::new("/bin/true"), "install", iso);
    let shim = cmd
        .get_args()
        .filter_map(|a| a.to_str())
        .find(|a| a.contains("exec"))
        .expect("the sh -c shim argument is present");
    assert!(shim.contains("ip link set lo up"));
    assert!(shim.contains("ip addr add 10.0.0.1/8 dev lo"));
    assert!(shim.contains("ip -6 addr add fd::1/8 dev lo"));
}

#[test]
fn network_sandbox_exempt_matches_portage_doebuild_spawns_own_formula() {
    // PROPERTIES=live only exempts the unpack phase.
    assert!(network_sandbox_exempt("unpack", "", "live"));
    assert!(!network_sandbox_exempt("compile", "", "live"));
    assert!(!network_sandbox_exempt("test", "", "live"));

    // PROPERTIES=test_network only exempts the test phase.
    assert!(network_sandbox_exempt("test", "", "test_network"));
    assert!(!network_sandbox_exempt("unpack", "", "test_network"));

    // RESTRICT=network-sandbox exempts every phase.
    assert!(network_sandbox_exempt("unpack", "network-sandbox", ""));
    assert!(network_sandbox_exempt("compile", "network-sandbox", ""));
    assert!(network_sandbox_exempt("test", "network-sandbox", ""));

    // None of the three -> not exempt.
    assert!(!network_sandbox_exempt("unpack", "", ""));
    assert!(!network_sandbox_exempt("test", "", ""));

    // USE-conditional groups already dropped by the time these
    // strings arrive (restrict_and_properties's own job) -- a raw
    // conditional token string is simply never matched.
    assert!(!network_sandbox_exempt("unpack", "", "foo? ( live )"));
}

/// #37 S3: the phase-execution gates read the **resolved** `FEATURES`
/// threaded on `extra_env` (`features_string`), never the raw process
/// env -- a `make.conf` `-sandbox` disables the sandbox even when the
/// calling env exports `sandbox`. The helpers take the features string
/// as a parameter precisely so this is directly testable.
#[test]
fn feature_gates_use_the_resolved_features_string() {
    assert!(!feature_token_present("buildpkg", "sandbox"));
    assert!(feature_token_present("sandbox usersandbox", "sandbox"));
    assert!(!network_sandbox_requested("distlocks"));
    assert!(network_sandbox_requested("network-sandbox"));
    assert!(!fs_sandbox_requested(""));
    assert!(fs_sandbox_requested("usersandbox"));
    // The extra_env pair wins over the process env (here: whatever the
    // test runner exported); an absent pair falls back to it.
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
    assert_eq!(ENVIRON_WHITELIST.lines().count(), 153);
    assert!(listed(SANDBOXED_SRC_PHASES, "compile"));
    assert!(!listed(SANDBOXED_SRC_PHASES, "setup"));
    assert!(environ_whitelisted("HOME") && environ_whitelisted("CCACHE_DIR"));
    assert!(!environ_whitelisted("EMERGE_DEFAULT_OPTS"));
}
