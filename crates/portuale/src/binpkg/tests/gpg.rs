use super::*;

#[test]
fn gpg_policy_for_features_matches_portage_gpkg_init_precedence() {
    // Real `gpkg.__init__` (`gpkg.py:798-819`): default is
    // verify-when-present, request off.
    assert_eq!(gpg_policy_for_features(""), (true, false));
    assert_eq!(
        gpg_policy_for_features("sandbox binpkg-signing"),
        (true, false)
    );
    assert_eq!(
        gpg_policy_for_features("binpkg-request-signature"),
        (true, true)
    );
    assert_eq!(
        gpg_policy_for_features("binpkg-ignore-signature"),
        (false, false)
    );
    // Real's own `if`/`elif` order: a request beats an ignore.
    assert_eq!(
        gpg_policy_for_features("binpkg-request-signature binpkg-ignore-signature"),
        (true, true)
    );
}

#[test]
fn gpg_verify_argv_substitutes_both_placeholders() {
    let argv = gpg_verify_argv(
        DEFAULT_GPG_VERIFY_BASE_COMMAND,
        "/tmp/test-home",
        "/tmp/x.sig -",
    );
    assert_eq!(
        argv,
        vec![
            "/usr/bin/gpg",
            "--verify",
            "--batch",
            "--no-tty",
            "--yes",
            "--no-auto-check-trustdb",
            "--status-fd",
            "2",
            "--homedir",
            "/tmp/test-home",
            "/tmp/x.sig",
            "-",
        ]
    );
    let argv = gpg_verify_argv(DEFAULT_GPG_VERIFY_BASE_COMMAND, "/tmp/h", "--output - -");
    assert!(argv.ends_with(&["--output".to_string(), "-".to_string(), "-".to_string()]));
}
