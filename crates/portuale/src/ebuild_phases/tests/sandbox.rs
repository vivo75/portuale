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
