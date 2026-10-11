use super::*;

/// #105: `resolve_config` freezes the immutable USE-context digest
/// into `use_context_base`, and `use_context_base_fingerprint` is
/// content-derived while excluding `autounmask_use` -- the one field
/// the `'backtrack` loop mutates, which the memo keys hash
/// separately.
#[test]
fn use_context_base_fingerprint_is_frozen_at_resolution_and_excludes_autounmask_use() {
    let root = fixtures_root();
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config must resolve");
    assert!(
        config.use_context_base.is_some(),
        "resolve_config must freeze the immutable USE-context digest"
    );
    // Content-derived: changing a hashed field changes the digest.
    let mut other = config.clone();
    other.accept_keywords.insert("~amd64".to_string());
    assert_ne!(
        use_context_base_fingerprint(&config),
        use_context_base_fingerprint(&other)
    );
    // `autounmask_use` is not part of the base.
    let mut with_autounmask = config.clone();
    with_autounmask.autounmask_use = vec![("dev-libs/foo".to_string(), vec!["flag".to_string()])];
    assert_eq!(
        use_context_base_fingerprint(&config),
        use_context_base_fingerprint(&with_autounmask)
    );
}

/// #114: `resolve_config` also freezes the visibility base (masks +
/// the license/keyword/property/restrict acceptance lists +
/// `license_groups`) and it is content-derived.
#[test]
fn is_visible_base_fingerprint_is_frozen_at_resolution_and_content_derived() {
    let root = fixtures_root();
    let config = resolve_config(ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config must resolve");
    assert!(
        config.is_visible_base.is_some(),
        "resolve_config must freeze the visibility base"
    );
    assert_ne!(
        config.is_visible_base, config.use_context_base,
        "the visibility base must not just repeat the USE-context base"
    );
    // Content-derived: masks...
    let mut masked = config.clone();
    masked.package_mask.push("dev-libs/foo".to_string());
    assert_ne!(
        is_visible_base_fingerprint(&config),
        is_visible_base_fingerprint(&masked)
    );
    // ...and the accept lists.
    let mut accepted = config.clone();
    accepted.accept_license.push("SomeEula".to_string());
    assert_ne!(
        is_visible_base_fingerprint(&config),
        is_visible_base_fingerprint(&accepted)
    );
}
