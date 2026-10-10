use super::*;

#[test]
fn expand_license_token_passes_through_a_plain_license_name() {
    let groups = HashMap::new();
    assert_eq!(expand_license_token("GPL-2", &groups), vec!["GPL-2"]);
}

#[test]
fn expand_license_token_passes_through_the_star_wildcard_tokens() {
    let groups = HashMap::new();
    assert_eq!(expand_license_token("*", &groups), vec!["*"]);
    assert_eq!(expand_license_token("-*", &groups), vec!["-*"]);
}

#[test]
fn expand_license_token_expands_a_group_reference() {
    let groups = HashMap::from([(
        "FREE".to_string(),
        vec!["GPL-2".to_string(), "MIT".to_string()],
    )]);
    assert_eq!(expand_license_token("@FREE", &groups), vec!["GPL-2", "MIT"]);
}

#[test]
fn expand_license_token_negates_every_expanded_member() {
    let groups = HashMap::from([(
        "EULA".to_string(),
        vec!["Eula1".to_string(), "Eula2".to_string()],
    )]);
    assert_eq!(
        expand_license_token("-@EULA", &groups),
        vec!["-Eula1", "-Eula2"]
    );
}

#[test]
fn expand_license_token_recurses_into_a_nested_group() {
    let groups = HashMap::from([
        ("FREE".to_string(), vec!["@FSF-APPROVED".to_string()]),
        ("FSF-APPROVED".to_string(), vec!["GPL-2".to_string()]),
    ]);
    assert_eq!(expand_license_token("@FREE", &groups), vec!["GPL-2"]);
}

#[test]
fn expand_license_token_guards_against_a_circular_group_reference() {
    // A -> B -> A: expanding "@A" recurses into B, which tries to
    // re-expand "@A" while "A" is still on the traversal stack, so
    // that innermost attempt falls back to the literal "@A" text
    // (matching real portage's own writemsg-then-literal-fallback
    // behavior) rather than looping forever.
    let groups = HashMap::from([
        ("A".to_string(), vec!["@B".to_string()]),
        ("B".to_string(), vec!["@A".to_string()]),
    ]);
    assert_eq!(expand_license_token("@A", &groups), vec!["@A"]);
}

#[test]
fn expand_license_token_leaves_an_undefined_group_as_literal_text() {
    let groups = HashMap::new();
    assert_eq!(expand_license_token("@NOPE", &groups), vec!["@NOPE"]);
}

#[test]
fn parse_license_groups_lines_extends_a_repeated_group_name() {
    let lines = vec![
        "FREE GPL-2".to_string(),
        "FREE MIT".to_string(),
        "EULA MyEula".to_string(),
    ];
    let groups = parse_license_groups_lines(&lines);
    assert_eq!(
        groups.get("FREE"),
        Some(&vec!["GPL-2".to_string(), "MIT".to_string()])
    );
    assert_eq!(groups.get("EULA"), Some(&vec!["MyEula".to_string()]));
}

#[test]
fn scope_repo_mask_lines_adds_reponame_only_when_no_repo_part_is_present() {
    let lines = vec![
        "dev-libs/a".to_string(),
        "-dev-libs/b".to_string(),
        "dev-libs/c::already-scoped".to_string(),
    ];
    assert_eq!(
        scope_repo_mask_lines(&lines, "overlay"),
        vec![
            "dev-libs/a::overlay".to_string(),
            "-dev-libs/b::overlay".to_string(),
            "dev-libs/c::already-scoped".to_string(),
        ]
    );
}
