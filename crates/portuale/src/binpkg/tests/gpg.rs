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

/// Backlog #43: real `gpkg.__init__` (`gpkg.py:792-819`) takes a
/// binrepo's explicit `verify-signature` as *both* flags (unlike the
/// unset case, where `request` depends on the request feature), then
/// lets the stronger `FEATURES` token override.
#[test]
fn gpg_verify_from_binrepo_matches_portage_gpkg_init_with_an_explicit_verify_signature() {
    // `verify-signature = true`: verify + the sidecar requirement.
    assert!(GpgVerify::from_binrepo(true, "sandbox").verify_signature);
    assert!(GpgVerify::from_binrepo(true, "sandbox").request_signature);
    // `verify-signature = false`: both off, so an unsigned archive
    // from that repo merges (real's documented opt-out).
    assert!(!GpgVerify::from_binrepo(false, "sandbox").verify_signature);
    assert!(!GpgVerify::from_binrepo(false, "sandbox").request_signature);
    // FEATURES override in one direction if stronger.
    assert!(GpgVerify::from_binrepo(false, "binpkg-request-signature").verify_signature);
    assert!(GpgVerify::from_binrepo(false, "binpkg-request-signature").request_signature);
    assert!(!GpgVerify::from_binrepo(true, "binpkg-ignore-signature").verify_signature);
    assert!(!GpgVerify::from_binrepo(true, "binpkg-ignore-signature").request_signature);
    // Request beats ignore (real's `if`/`elif`).
    assert!(
        GpgVerify::from_binrepo(false, "binpkg-request-signature binpkg-ignore-signature")
            .verify_signature
    );
}
