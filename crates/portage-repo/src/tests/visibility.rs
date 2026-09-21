use super::*;

#[test]
fn license_default_accept_license_star_makes_any_declared_license_visible() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["*".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        license: "GPL-2".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn license_not_in_the_acceptable_set_is_masked() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["MIT".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        license: "GPL-2".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(!is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn license_eula_style_negation_masks_a_matching_license() {
    // accept_license here is the already-@group-expanded form
    // resolve_config would have produced from `"* -@EULA"`.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["*".to_string(), "-SomeEula".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        license: "SomeEula".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(!is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn license_package_license_override_unmasks_for_one_matching_package_only() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["*".to_string(), "-SomeEula".to_string()],
        package_license: vec![("dev-libs/foo".to_string(), vec!["SomeEula".to_string()])],
        ..Default::default()
    };
    let c = Candidate {
        license: "SomeEula".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
    assert!(!is_visible(&c, "dev-libs", "bar", &config));
}

#[test]
fn license_any_of_group_is_satisfied_by_one_accepted_alternative() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["BSD".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        license: "|| ( GPL-2 BSD )".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn license_use_conditional_only_masks_once_the_flag_is_actually_enabled() {
    let c = Candidate {
        license: "GPL-2 foo? ( MIT )".to_string(),
        iuse: "foo".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let config_foo_off = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["GPL-2".to_string()],
        ..Default::default()
    };
    assert!(is_visible(&c, "dev-libs", "foo-pkg", &config_foo_off));

    let config_foo_on = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["GPL-2".to_string()],
        use_flags: HashSet::from(["foo".to_string()]),
        use_tokens: vec!["foo".to_string()],
        ..Default::default()
    };
    assert!(!is_visible(&c, "dev-libs", "foo-pkg", &config_foo_on));
}

#[test]
fn properties_default_star_accepts_any_declared_property() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec!["*".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        properties: "live".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn properties_not_in_the_acceptable_set_is_masked() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec!["interactive".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        properties: "live".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(!is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn package_properties_override_unmasks_for_one_matching_package_only() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec![],
        package_properties: vec![("dev-libs/foo".to_string(), vec!["live".to_string()])],
        ..Default::default()
    };
    let c = Candidate {
        properties: "live".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
    assert!(!is_visible(&c, "dev-libs", "bar", &config));
}

#[test]
fn restrict_default_star_accepts_any_declared_token() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_restrict: vec!["*".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        restrict: "test bindist".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn restrict_not_in_the_acceptable_set_is_masked() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_restrict: vec!["test".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        restrict: "bindist".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(!is_visible(&c, "dev-libs", "foo", &config));
}

#[test]
fn package_accept_restrict_override_unmasks_for_one_matching_package_only() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_restrict: vec![],
        package_accept_restrict: vec![("dev-libs/foo".to_string(), vec!["bindist".to_string()])],
        ..Default::default()
    };
    let c = Candidate {
        restrict: "bindist".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(is_visible(&c, "dev-libs", "foo", &config));
    assert!(!is_visible(&c, "dev-libs", "bar", &config));
}

#[test]
fn restrict_multiple_tokens_all_need_accepting() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_restrict: vec!["test".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        restrict: "test bindist".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    assert!(!is_visible(&c, "dev-libs", "foo", &config));
}

/// The memo key distinguishes configs that differ only in their
/// accept lists (outside the USE-context fingerprint).
#[test]
fn metadata_key_accepted_memo_does_not_leak_between_differing_accept_lists() {
    let base = |accept: &str| portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec![accept.to_string()],
        ..Default::default()
    };
    let star = base("*");
    let interactive = base("interactive");
    let c = Candidate {
        properties: "live".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let candidate_str = "dev-libs/memoaccept-1.0:0/0::test";
    let call = |config: &portage_profile::Config| {
        metadata_key_accepted(MetadataKeyAcceptedArgs {
            value_str: &c.properties,
            candidate: &c,
            category: "dev-libs",
            package: "memoaccept",
            candidate_str,
            config,
            which: MetadataKey::Properties,
            global_accept: &config.accept_properties,
            package_accept: &config.package_properties,
        })
    };
    assert!(call(&star));
    assert!(
        !call(&interactive),
        "a differing accept_properties must not reuse the memo entry"
    );
}

/// The memo key includes the candidate's own `IUSE`/`KEYWORDS`
/// (`candidate_str` does not encode them).
#[test]
fn metadata_key_accepted_memo_does_not_leak_between_differing_candidate_iuse() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec![],
        ..Default::default()
    };
    let value = "!test? ( test )";
    let candidate_str = "dev-libs/memoiuse-1.0:0/0::test";
    let call = |iuse: &str| {
        let c = Candidate {
            iuse: iuse.to_string(),
            properties: value.to_string(),
            ..candidate("1.0", &["amd64"])
        };
        metadata_key_accepted(MetadataKeyAcceptedArgs {
            value_str: &c.properties,
            candidate: &c,
            category: "dev-libs",
            package: "memoiuse",
            candidate_str,
            config: &config,
            which: MetadataKey::Properties,
            global_accept: &config.accept_properties,
            package_accept: &config.package_properties,
        })
    };
    // IUSE="+test" enables the flag through its own default marker, so
    // `!test?` is false and the group is dropped -> true.
    assert!(call("+test"));
    // IUSE="test" carries no default marker, so the flag stays off,
    // `!test?` holds, the group yields "test" and nothing accepts it
    // -> false. Only the candidate's own IUSE separates the two calls.
    assert!(
        !call("test"),
        "a differing candidate IUSE must not reuse the memo entry"
    );
}

/// A repeated call with identical arguments hits the memo, and the
/// `MetadataKey` discriminant keeps `PROPERTIES`/`RESTRICT` apart.
#[test]
fn metadata_key_accepted_memo_hits_on_a_repeat_and_separates_metadata_keys() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_properties: vec!["*".to_string()],
        accept_restrict: vec![],
        ..Default::default()
    };
    let c = Candidate {
        properties: "test".to_string(),
        restrict: "test".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let candidate_str = "dev-libs/memokey-1.0:0/0::test";
    let call =
        |which: MetadataKey, value: &str, global: &[String], package: &[(String, Vec<String>)]| {
            metadata_key_accepted(MetadataKeyAcceptedArgs {
                value_str: value,
                candidate: &c,
                category: "dev-libs",
                package: "memokey",
                candidate_str,
                config: &config,
                which,
                global_accept: global,
                package_accept: package,
            })
        };
    let properties = || {
        call(
            MetadataKey::Properties,
            &c.properties,
            &config.accept_properties,
            &config.package_properties,
        )
    };
    let restrict = || {
        call(
            MetadataKey::Restrict,
            &c.restrict,
            &config.accept_restrict,
            &config.package_accept_restrict,
        )
    };

    let before = metadata_key_accepted_uncached_calls();
    assert!(properties());
    let after_first = metadata_key_accepted_uncached_calls();
    assert_eq!(after_first, before + 1, "first call is a miss");
    assert!(properties());
    assert_eq!(
        metadata_key_accepted_uncached_calls(),
        after_first,
        "a repeat with identical arguments must hit the memo"
    );
    // Same value string, same candidate, same config: only the
    // `MetadataKey` discriminant separates the two accept-list pairs,
    // and `accept_restrict=[]` rejects "test" while
    // `accept_properties=["*"]` accepts it.
    assert!(!restrict());
}

#[test]
fn package_mask_hides_a_matching_candidate() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_mask: vec!["dev-libs/foo".to_string()],
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "foo",
        &config
    ));
}

#[test]
fn package_unmask_cancels_a_matching_mask() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_mask: vec!["dev-libs/foo".to_string()],
        package_unmask: vec!["dev-libs/foo".to_string()],
        ..Default::default()
    };
    assert!(is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "foo",
        &config
    ));
}

/// The `is_visible` memo is transparent and separates candidate
/// metadata and config visibility fields.
#[test]
fn is_visible_memo_hits_on_repeat_and_separates_its_inputs() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        accept_license: vec!["GPL-2".to_string()],
        accept_properties: vec!["*".to_string()],
        accept_restrict: vec!["*".to_string()],
        ..Default::default()
    };
    let c = Candidate {
        license: "GPL-2".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let before = is_visible_uncached_calls();
    assert!(is_visible(&c, "dev-libs", "vismemo", &config));
    let after_first = is_visible_uncached_calls();
    assert_eq!(after_first, before + 1, "first call is a miss");
    assert!(is_visible(&c, "dev-libs", "vismemo", &config));
    assert_eq!(
        is_visible_uncached_calls(),
        after_first,
        "a repeat with identical arguments must hit the memo"
    );
    // A differing candidate LICENSE misses (and flips the answer).
    let mut proprietary = c.clone();
    proprietary.license = "Proprietary".to_string();
    assert!(!is_visible(&proprietary, "dev-libs", "vismemo", &config));
    assert_eq!(is_visible_uncached_calls(), after_first + 1);
    // A differing candidate version misses (the literal
    // `candidate_str` in the key bounds any digest collision to
    // same-candidate inputs).
    let mut v2 = c.clone();
    v2.version = "2.0".to_string();
    assert!(is_visible(&v2, "dev-libs", "vismemo", &config));
    assert_eq!(is_visible_uncached_calls(), after_first + 2);
    // A differing config accept_license misses.
    let mut nothing_accepted = config.clone();
    nothing_accepted.accept_license = Vec::new();
    assert!(!is_visible(&c, "dev-libs", "vismemo", &nothing_accepted));
    assert_eq!(is_visible_uncached_calls(), after_first + 3);
}

#[test]
fn package_mask_wildcard_matches_whole_category() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_mask: vec!["dev-libs/*".to_string()],
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "anything",
        &config
    ));
    assert!(is_visible(
        &candidate("1.0", &["amd64"]),
        "app-misc",
        "anything",
        &config
    ));
}

#[test]
fn package_accept_keywords_wildcard_extends_visibility() {
    // Globally only "amd64" is accepted, but a package.accept_keywords
    // wildcard entry additionally accepts "~amd64" for dev-qt/*.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![("dev-qt/*".to_string(), vec!["~amd64".to_string()])],
        ..Default::default()
    };
    assert!(is_visible(
        &candidate("1.0", &["~amd64"]),
        "dev-qt",
        "qtcore",
        &config
    ));
    // A package outside the wildcard doesn't get the extra keyword.
    assert!(!is_visible(
        &candidate("1.0", &["~amd64"]),
        "dev-libs",
        "foo",
        &config
    ));
}

#[test]
fn package_accept_keywords_double_star_accepts_unconditionally() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![("dev-libs/live".to_string(), vec!["**".to_string()])],
        ..Default::default()
    };
    // No KEYWORDS at all (e.g. a live/9999 ebuild) is still visible.
    assert!(is_visible(
        &candidate("9999", &[]),
        "dev-libs",
        "live",
        &config
    ));
}

#[test]
fn package_accept_keywords_negation_revokes_a_globally_accepted_keyword() {
    // Globally "amd64" is accepted, but a `-amd64` entry revokes it
    // for this one package (folded `-token` removals).
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![(
            "dev-libs/norevoke".to_string(),
            vec!["-amd64".to_string()],
        )],
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "norevoke",
        &config
    ));
    // An unrelated package keeps the global "amd64" acceptance.
    assert!(is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "unrelated",
        &config
    ));
}

#[test]
fn package_accept_keywords_dash_star_clears_every_globally_accepted_keyword() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string(), "x86".to_string()]),
        package_accept_keywords: vec![("dev-libs/norevoke".to_string(), vec!["-*".to_string()])],
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["amd64"]),
        "dev-libs",
        "norevoke",
        &config
    ));
    assert!(!is_visible(
        &candidate("1.0", &["x86"]),
        "dev-libs",
        "norevoke",
        &config
    ));
}

#[test]
fn package_accept_keywords_entries_fold_in_source_order() {
    // Entries fold in file / profile-chain order: a later `-amd64`
    // revoke wins over an earlier grant, whatever the specificity.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![
            (
                "=dev-libs/regrant-2.0".to_string(),
                vec!["amd64".to_string()],
            ),
            ("dev-libs/regrant".to_string(), vec!["-amd64".to_string()]),
        ],
        ..Default::default()
    };
    // Both entries match 2.0; the later `-amd64` wins -> revoked.
    assert!(!is_visible(
        &candidate("2.0", &["amd64"]),
        "dev-libs",
        "regrant",
        &config
    ));
    // Reverse the order: the later `amd64` grant wins -> visible.
    let config_rev = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![
            ("dev-libs/regrant".to_string(), vec!["-amd64".to_string()]),
            (
                "=dev-libs/regrant-2.0".to_string(),
                vec!["amd64".to_string()],
            ),
        ],
        ..Default::default()
    };
    assert!(is_visible(
        &candidate("2.0", &["amd64"]),
        "dev-libs",
        "regrant",
        &config_rev
    ));
}

#[test]
fn package_accept_keywords_dash_star_can_revoke_an_earlier_double_star() {
    // A more specific "-*" revokes even an unconditional "**" grant
    // from a less specific entry -- proving "**" is folded in fold
    // order now, not checked via a separate order-blind pre-scan.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        package_accept_keywords: vec![
            ("dev-libs/live".to_string(), vec!["**".to_string()]),
            ("=dev-libs/live-9999".to_string(), vec!["-*".to_string()]),
        ],
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("9999", &[]),
        "dev-libs",
        "live",
        &config
    ));
}

#[test]
fn package_accept_keywords_star_accepts_any_stable_keyword() {
    // Global ACCEPT_KEYWORDS "*" accepts any stable-classified
    // keyword the candidate declares, even one never otherwise
    // mentioned anywhere.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["*".to_string()]),
        ..Default::default()
    };
    assert!(is_visible(
        &candidate("1.0", &["arm64"]),
        "dev-libs",
        "starpkg",
        &config
    ));
}

#[test]
fn package_accept_keywords_star_does_not_accept_a_testing_only_candidate() {
    // "*" only ever covers stable-classified keywords -- a
    // testing-only ("~"-prefixed) candidate still needs "~*"
    // specifically.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["*".to_string()]),
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["~arm64"]),
        "dev-libs",
        "starpkg",
        &config
    ));
}

#[test]
fn package_accept_keywords_tilde_star_accepts_any_testing_keyword() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["~*".to_string()]),
        ..Default::default()
    };
    assert!(is_visible(
        &candidate("1.0", &["~arm64"]),
        "dev-libs",
        "starpkg",
        &config
    ));
}

#[test]
fn package_accept_keywords_tilde_star_does_not_accept_a_stable_only_candidate() {
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["~*".to_string()]),
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["arm64"]),
        "dev-libs",
        "starpkg",
        &config
    ));
}

#[test]
fn package_accept_keywords_negative_keyword_never_counts_toward_star() {
    // A "-arm64" KEYWORDS token (explicit "not supported here") is
    // excluded from classification entirely -- with no other real
    // keyword declared, "*" has nothing stable-classified to grant
    // acceptance for.
    let config = portage_profile::Config {
        accept_keywords: HashSet::from(["*".to_string()]),
        ..Default::default()
    };
    assert!(!is_visible(
        &candidate("1.0", &["-arm64"]),
        "dev-libs",
        "starpkg",
        &config
    ));
}
