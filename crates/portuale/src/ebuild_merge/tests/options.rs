use super::*;

/// Both `FEATURES=collision-protect` **and** `protect-owned` off: an
/// ordinary file collision is merged over (`collisionpkg-c`
/// overwrites `collisionpkg-a`'s own `shared.txt`). Real
/// `protect-owned` *is* one of real `make.globals`'s own default
/// `FEATURES` tokens (`cnf/make.globals:77-84`, confirmed by reading
/// it directly), unlike `collision-protect` -- so real portage's own
/// actual default behavior for this exact scenario (an identifiable
/// owner for the collision, `collisionpkg-a`) is to *abort*, not
/// merge over; `MergeOptions::default()` alone (`protect_owned:
/// true`) reproduces that real default correctly. This test
/// therefore sets `protect_owned: false` explicitly rather than
/// relying on `MergeOptions::default()` -- see
/// `protect_owned_alone_aborts_when_an_owner_is_identified` for the
/// real-default (`protect_owned: true`) case.
/// #333: the merge's protect lists are the resolved
/// `CONFIG_PROTECT[_MASK]` fold (real `dblink._get_protect_obj`,
/// `vartree.py:2063-2072`, reading `settings`), not the raw process
/// env `from_env` read. A fold that resolves to nothing (`-*`)
/// protects nothing; a key no layer assigned keeps the `from_env`
/// value.
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
    // No layer assigned `CONFIG_PROTECT_MASK`: the `from_env` value stands.
    assert_eq!(options.config_protect_mask, "/from-env-mask");

    config
        .incremental_sources
        .insert("CONFIG_PROTECT".to_string(), layers(&[&["/etc"], &["-*"]]));
    options.apply_config_protect(&config);
    assert_eq!(options.config_protect, "");
}

#[test]
fn set_resolved_features_rederives_the_merge_tokens() {
    // #37 S3: the emerge paths replace `from_env`'s raw-FEATURES
    // fields with the resolved list, which is what makes a
    // make.conf `collision-protect` / `-protect-owned` /
    // `config-protect-if-modified` take effect; the GPG verify policy
    // follows the same list.
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
    // Defaults-on tokens present in make.globals stay on.
    options.set_resolved_features("protect-owned config-protect-if-modified");
    assert!(options.protect_owned);
    assert!(options.protect_if_modified);
    assert!(!options.collision_protect);
    assert!(!options.gpg_verify.request_signature);
}
