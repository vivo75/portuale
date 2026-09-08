use super::*;

#[test]
fn remote_exec_handoff_sets_and_takes() {
    assert!(take_remote_exec().is_none());
    let ctx = ctx_with_args("h", None);
    set_remote_exec(ctx.clone());
    assert_eq!(take_remote_exec(), Some(ctx));
    // Taking clears the slot: no leakage across calls.
    assert!(take_remote_exec().is_none());
}

#[test]
fn strict_host_key_checking_parses() {
    assert_eq!(
        StrictHostKeyChecking::parse("accept-new"),
        Some(StrictHostKeyChecking::AcceptNew)
    );
    assert_eq!(
        StrictHostKeyChecking::parse("yes"),
        Some(StrictHostKeyChecking::Yes)
    );
    assert_eq!(
        StrictHostKeyChecking::parse("no"),
        Some(StrictHostKeyChecking::No)
    );
    assert_eq!(StrictHostKeyChecking::parse("sometimes"), None);
    assert_eq!(
        StrictHostKeyChecking::AcceptNew.as_ssh_value(),
        "accept-new"
    );
}

#[test]
fn etc_placement_parses_server_and_client_forms() {
    assert_eq!(
        ConfigPlacement::parse("--remote-etc-portage", "server:/etc/portage"),
        Ok(ConfigPlacement::Server("/etc/portage".to_string()))
    );
    assert_eq!(
        ConfigPlacement::parse("--remote-etc-portage", "client:/etc/portage"),
        Ok(ConfigPlacement::Client("/etc/portage".to_string()))
    );
    assert!(ConfigPlacement::parse("--remote-etc-portage", "/etc/portage").is_err());
    assert!(ConfigPlacement::parse("--remote-etc-portage", "server:").is_err());
    assert!(ConfigPlacement::parse("--remote-etc-portage", "bizarre").is_err());
}
