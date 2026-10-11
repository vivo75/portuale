use super::*;

#[test]
fn lookup_ids_follow_host_key_alias_and_ports() {
    // No alias: hostname plus the bracketed host:port spelling (plain
    // `-F host` does not match `[host]:port` entries).
    assert_eq!(
        lookup_ids(&ctx_with_args("client.invalid", None)),
        vec![
            "client.invalid".to_string(),
            "[client.invalid]:22222".to_string()
        ]
    );
    // `-o HostKeyAlias=X` (joined or split) redirects the id.
    assert_eq!(
        lookup_ids(&ctx_with_args("h", Some("-o HostKeyAlias=alias.invalid"))),
        vec!["alias.invalid".to_string()]
    );
    assert_eq!(
        lookup_ids(&ctx_with_args("h", Some("-o HostKeyAlias=alias.invalid"))),
        lookup_ids(&ctx_with_args("h", Some("HostKeyAlias=alias.invalid"))),
    );
    // Unrelated extra args leave the hostname ids alone.
    assert_eq!(
        lookup_ids(&ctx_with_args("h", Some("-o Foo=yes")))[0],
        "h".to_string()
    );
}
