use super::*;

/// #313: a local-transport client script gets `PORTUALE_BIN` (the shim's
/// target) and has the server's `PORTUALE_VDB_*` removed from its
/// inherited environment (`get_envs` reports a removal as `None`).
#[test]
fn local_client_bash_exports_the_binary_and_scrubs_the_servers_vdb_selection() {
    let cmd = local_client_bash();
    let envs: std::collections::BTreeMap<_, _> = cmd.get_envs().collect();
    let exe = std::env::current_exe().unwrap();
    assert_eq!(
        envs.get(std::ffi::OsStr::new("PORTUALE_BIN")),
        Some(&Some(exe.as_os_str()))
    );
    for var in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        crate::vdb_ipc::IPC_VAR,
    ] {
        assert_eq!(envs.get(std::ffi::OsStr::new(var)), Some(&None), "{var}");
    }
}

#[test]
fn sh_quote_wraps_and_escapes() {
    assert_eq!(sh_quote("/"), "'/'");
    assert_eq!(sh_quote("/var/tmp/a b"), "'/var/tmp/a b'");
    assert_eq!(sh_quote("o'clock"), "'o'\\''clock'");
    assert_eq!(sh_quote(""), "''");
    assert_eq!(sh_quote("a'b'c"), "'a'\\''b'\\''c'");
}

#[test]
fn transport_errors_are_not_remote_failures() {
    assert!(is_transport_error(Some(255), ""));
    assert!(is_transport_error(
        None,
        "ssh: Could not resolve hostname badhost"
    ));
    assert!(is_transport_error(None, "Connection refused"));
    assert!(!is_transport_error(Some(1), "some remote error"));
    assert!(!is_transport_error(Some(0), ""));
}

#[test]
fn control_dir_prefers_short_dirs_and_skips_unfitting_ones() {
    use std::path::PathBuf;
    // A deep base cannot fit `%C` + suffix under 108 bytes.
    let deep = PathBuf::from("/tmp/pytest-of-vivo/pytest-707/loopback-sshd0/home");
    assert!(control_dir_for(None, Some(deep)).is_none());
    // XDG_RUNTIME_DIR wins when it fits (and gets created 0700).
    // Deliberately NOT `TempDir`: this test is about the 108-byte
    // `sun_path` budget, and `TempDir`'s own `portuale-td-…-{pid}-{nanos}`
    // name already eats too much of it to fit `%C` + suffix.
    let tmp = PathBuf::from("/tmp/cpfit");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let picked = control_dir_for(Some(tmp.clone()), None);
    assert_eq!(picked, Some(tmp.join(".portuale-cp")));
    assert!(tmp.join(".portuale-cp").is_dir());
    let _ = std::fs::remove_dir_all(&tmp);
    // Nothing configured at all: no multiplexing.
    assert!(control_dir_for(None, None).is_none());
}
