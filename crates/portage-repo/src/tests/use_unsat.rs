use super::*;

#[test]
fn use_unsat_candidates_count_conditional_flags_toward_missing_iuse() {
    // Conditional flags count toward `Missing IUSE`: a conditional a
    // visible candidate's IUSE lacks is never a `Change USE:` row.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let scan = |display: &str, evaluated: &str| {
        use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
            repos: &repos,
            root: &root,
            entries: &[],
            display_atom: display,
            evaluated_atom: evaluated,
            owner: None,
            config: &config,
        })
    };
    // `foo?` alone: no unconditional flag at all, yet Missing IUSE.
    assert_eq!(
        scan("dev-libs/mia0b[foo?]", "dev-libs/mia0b[foo?]"),
        Some(vec![(
            "dev-libs/mia0b-1::testrepo".to_string(),
            vec!["Missing IUSE: foo".to_string()]
        )])
    );
    // `foo?` beside a satisfiable unconditional `bar`: the conditional
    // flag still counts, so the row is Missing IUSE, not Change USE.
    // (Evaluated against a foo-enabled parent, as the walk does.)
    assert_eq!(
        scan("dev-libs/mia0b[foo?,bar]", "dev-libs/mia0b[foo,bar]"),
        Some(vec![(
            "dev-libs/mia0b-1::testrepo".to_string(),
            vec!["Missing IUSE: foo".to_string()]
        )])
    );
}
