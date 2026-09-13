use super::*;

#[test]
fn set_resolved_features_rederives_the_merge_tokens() {
    // The emerge paths replace the raw-FEATURES fields with the resolved
    // list, so make.conf tokens take effect.
    let mut options = MergeOptions::default();
    options.set_resolved_features("collision-protect binpkg-request-signature");
    assert!(options.collision_protect);
    assert!(!options.protect_owned);
    assert!(!options.protect_if_modified);
    assert_eq!(
        options.features,
        "collision-protect binpkg-request-signature"
    );
    assert!(options.gpg_verify.request_signature);
    options.set_resolved_features("protect-owned config-protect-if-modified");
    assert!(options.protect_owned);
    assert!(options.protect_if_modified);
    assert!(!options.collision_protect);
    assert!(!options.gpg_verify.request_signature);
}
