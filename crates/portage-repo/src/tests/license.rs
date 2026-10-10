use super::*;

#[test]
fn parse_license_tree_flattens_a_plain_group_at_top_level() {
    // Verified directly against real portage.dep.use_reduce(...,
    // opconvert=True): "GPL-2 MIT" -> ['GPL-2', 'MIT'] (a bare
    // top-level list already has AND semantics, so a plain "(...)"
    // there adds nothing structurally worth keeping).
    let tree = parse_license_tree(&license_tokens("GPL-2 ( MIT )"), &HashSet::new()).unwrap();
    assert_eq!(
        tree,
        vec![
            LicenseNode::License("GPL-2".to_string()),
            LicenseNode::License("MIT".to_string()),
        ]
    );
}

#[test]
fn parse_license_tree_keeps_any_of_members_flat() {
    // Verified: "GPL-2 || ( MIT BSD )" -> ['GPL-2', ['||', 'MIT', 'BSD']]
    // -- the || group's own members sit directly in its own list,
    // not double-nested.
    let tree =
        parse_license_tree(&license_tokens("GPL-2 || ( MIT BSD )"), &HashSet::new()).unwrap();
    assert_eq!(
        tree,
        vec![
            LicenseNode::License("GPL-2".to_string()),
            LicenseNode::AnyOf(vec![
                LicenseNode::License("MIT".to_string()),
                LicenseNode::License("BSD".to_string()),
            ]),
        ]
    );
}

#[test]
fn parse_license_tree_nests_a_plain_group_directly_inside_any_of() {
    // Verified: "|| ( ( GPL-2 MIT ) BSD )" ->
    // [['||', ['GPL-2', 'MIT'], 'BSD']] -- a plain sub-group sitting
    // directly inside a || group's own member list stays a genuine
    // nested "this whole bundle is one alternative" unit, unlike
    // the top-level case above.
    let tree =
        parse_license_tree(&license_tokens("|| ( ( GPL-2 MIT ) BSD )"), &HashSet::new()).unwrap();
    assert_eq!(
        tree,
        vec![LicenseNode::AnyOf(vec![
            LicenseNode::AllOf(vec![
                LicenseNode::License("GPL-2".to_string()),
                LicenseNode::License("MIT".to_string()),
            ]),
            LicenseNode::License("BSD".to_string()),
        ])]
    );
}

#[test]
fn parse_license_tree_resolves_a_use_conditional() {
    // Verified: "|| ( foo? ( GPL-2 MIT ) BSD )" with foo enabled ->
    // [['||', ['GPL-2', 'MIT'], 'BSD']]; with foo disabled ->
    // ['BSD'] (the conditional contributes nothing at all, and the
    // now-single-alternative || itself collapses away too -- real
    // use_reduce's own behavior, verified directly).
    let enabled = HashSet::from(["foo".to_string()]);
    let tree =
        parse_license_tree(&license_tokens("|| ( foo? ( GPL-2 MIT ) BSD )"), &enabled).unwrap();
    assert_eq!(
        tree,
        vec![LicenseNode::AnyOf(vec![
            LicenseNode::AllOf(vec![
                LicenseNode::License("GPL-2".to_string()),
                LicenseNode::License("MIT".to_string()),
            ]),
            LicenseNode::License("BSD".to_string()),
        ])]
    );

    let disabled = HashSet::new();
    let tree_disabled =
        parse_license_tree(&license_tokens("|| ( foo? ( GPL-2 MIT ) BSD )"), &disabled).unwrap();
    // Portuale's own parser keeps the || wrapper (unlike real
    // use_reduce's own further single-alternative collapse -- see
    // the module doc comment on why that specific collapse isn't
    // replicated); masking-wise this is equivalent either way,
    // since an AnyOf of one alternative and that alternative alone
    // make an identical accept/reject decision.
    assert_eq!(
        tree_disabled,
        vec![LicenseNode::AnyOf(vec![LicenseNode::License(
            "BSD".to_string()
        )])]
    );
}

#[test]
fn parse_license_tree_rejects_unbalanced_parens() {
    assert!(parse_license_tree(&license_tokens("( GPL-2"), &HashSet::new()).is_err());
    assert!(parse_license_tree(&license_tokens("GPL-2 )"), &HashSet::new()).is_err());
}

#[test]
fn parse_license_tree_rejects_a_dangling_double_pipe() {
    assert!(parse_license_tree(&license_tokens("|| GPL-2"), &HashSet::new()).is_err());
}

#[test]
fn has_masked_license_empty_string_is_never_masked() {
    assert!(!has_masked_license("", &HashSet::new(), &HashSet::new()).unwrap());
}

#[test]
fn has_masked_license_plain_and_semantics_needs_every_license_accepted() {
    let acceptable = HashSet::from(["GPL-2".to_string()]);
    assert!(has_masked_license("GPL-2 MIT", &HashSet::new(), &acceptable).unwrap());
    let acceptable_both = HashSet::from(["GPL-2".to_string(), "MIT".to_string()]);
    assert!(!has_masked_license("GPL-2 MIT", &HashSet::new(), &acceptable_both).unwrap());
}

#[test]
fn has_masked_license_any_of_needs_only_one_alternative_accepted() {
    let acceptable = HashSet::from(["BSD".to_string()]);
    assert!(!has_masked_license("|| ( GPL-2 BSD )", &HashSet::new(), &acceptable).unwrap());
    let acceptable_neither = HashSet::from(["MIT".to_string()]);
    assert!(has_masked_license("|| ( GPL-2 BSD )", &HashSet::new(), &acceptable_neither).unwrap());
}

#[test]
fn has_masked_license_any_of_alternative_bundle_needs_the_whole_bundle_accepted() {
    // "|| ( ( GPL-2 MIT ) BSD )": accepting only MIT (not GPL-2)
    // must NOT satisfy the (GPL-2 AND MIT) bundle -- only accepting
    // BSD, or both GPL-2 and MIT together, satisfies this.
    let only_mit = HashSet::from(["MIT".to_string()]);
    assert!(has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &only_mit).unwrap());
    let both = HashSet::from(["GPL-2".to_string(), "MIT".to_string()]);
    assert!(!has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &both).unwrap());
    let bsd_only = HashSet::from(["BSD".to_string()]);
    assert!(!has_masked_license("|| ( ( GPL-2 MIT ) BSD )", &HashSet::new(), &bsd_only).unwrap());
}
