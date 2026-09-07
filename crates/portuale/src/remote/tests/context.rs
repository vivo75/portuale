use super::*;

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
