use super::*;

/// The merge's protect lists are the resolved `CONFIG_PROTECT[_MASK]` fold,
/// not the raw process-env values; a fold resolving to nothing protects nothing.
/// Portage: `vartree.py`, `_get_protect_obj`.
#[test]
fn apply_config_protect_takes_the_resolved_fold() {
    let layers = |v: &[&[&str]]| -> Vec<Vec<String>> {
        v.iter()
            .map(|l| l.iter().map(|t| t.to_string()).collect())
            .collect()
    };
    let mut config = portage_profile::Config::default();
    config.incremental_sources.insert(
        "CONFIG_PROTECT".to_string(),
        layers(&[
            &["/etc", "/usr/share/gnupg/qualified.txt"],
            &["/usr/share/cfgprotme"],
        ]),
    );
    let mut options = MergeOptions {
        config_protect: "/from-env".to_string(),
        config_protect_mask: "/from-env-mask".to_string(),
        ..MergeOptions::default()
    };
    options.apply_config_protect(&config);
    assert_eq!(
        options.config_protect,
        "/etc /usr/share/cfgprotme /usr/share/gnupg/qualified.txt"
    );
    assert_eq!(options.config_protect_mask, "/from-env-mask");

    config
        .incremental_sources
        .insert("CONFIG_PROTECT".to_string(), layers(&[&["/etc"], &["-*"]]));
    options.apply_config_protect(&config);
    assert_eq!(options.config_protect, "");
}

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
