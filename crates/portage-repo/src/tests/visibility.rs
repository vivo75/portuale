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
