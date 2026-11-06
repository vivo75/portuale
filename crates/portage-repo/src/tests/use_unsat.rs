use super::*;

#[test]
fn use_unsat_candidates_classify_change_use_and_missing_iuse() {
    // Two reason kinds and the "latest version only" rule, direct on
    // the fixture repo -- the same rows the contract pins.
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
    let one = |atom: &str| {
        use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
            root: &root,
            entries: &[],
            display_atom: atom,
            evaluated_atom: atom,
            owner: None,
            config: &config,
            gate: crate::test_gate(&repos),
        })
    };
    assert_eq!(
        one("dev-libs/unsatusealt[unsatuseorflag]"),
        Some(vec![(
            "dev-libs/unsatusealt-1.0::testrepo".to_string(),
            vec!["Change USE: +unsatuseorflag".to_string()]
        )])
    );
    assert_eq!(
        one("dev-libs/unsatuseiusetarget[noiuse]"),
        Some(vec![(
            "dev-libs/unsatuseiusetarget-1.0::testrepo".to_string(),
            vec!["Missing IUSE: noiuse".to_string()]
        )])
    );
    // Two visible versions both miss: only the latest is listed.
    assert_eq!(
        one("dev-libs/unsatusealtmulti[unsatuseorflag]"),
        Some(vec![(
            "dev-libs/unsatusealtmulti-2.0::testrepo".to_string(),
            vec!["Change USE: +unsatuseorflag".to_string()]
        )])
    );
    // A satisfiable use-dep (flag is off, `-flag` required) is not
    // this disclosure at all.
    assert_eq!(one("dev-libs/unsatusealt[-unsatuseorflag]"), None);
    // No use-deps at all: Portage's other paths own the atom.
    assert_eq!(one("dev-libs/unsatusealt"), None);
}

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
            root: &root,
            entries: &[],
            display_atom: display,
            evaluated_atom: evaluated,
            owner: None,
            config: &config,
            gate: crate::test_gate(&repos),
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

#[test]
fn a_masked_version_with_the_required_iuse_hands_the_miss_to_the_masked_block() {
    // Backlog #611(a): visible `r594xdep-1` lacks `r594flag`, keyword-masked
    // `r594xdep-2` declares it. Portage 3.0.83 (`emerge -p dev-libs/r594xtop`,
    // `LLM/evidence/2026-11-06-sweep/611a/`) prints the all-masked block with
    // the `-2` row, not `Missing IUSE` (`masked_with_iuse`, D:6918-6929).
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
    let atom = "dev-libs/r594xdep[r594flag]";
    let args = || UseUnsatCandidatesForAtomArgs {
        root: &root,
        entries: &[],
        display_atom: atom,
        evaluated_atom: atom,
        owner: None,
        config: &config,
        gate: crate::test_gate(&repos),
    };
    assert_eq!(use_unsat_candidates_for_atom(args()), None);
    // Still a USE-only miss underneath, for the walk's stop order.
    assert_eq!(
        use_unsat_scan(args()),
        Some((
            vec![(
                "dev-libs/r594xdep-1::testrepo".to_string(),
                vec!["Missing IUSE: r594flag".to_string()]
            )],
            true
        ))
    );
    assert_eq!(
        masked_candidates_for_atom(crate::test_gate(&repos), atom, &config),
        Some(vec![(
            "dev-libs/r594xdep-2::testrepo".to_string(),
            vec!["~amd64 keyword".to_string()]
        )])
    );
    // No masked version declares `noiuse`: the Missing IUSE rows stay.
    assert!(
        use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
            display_atom: "dev-libs/unsatuseiusetarget[noiuse]",
            evaluated_atom: "dev-libs/unsatuseiusetarget[noiuse]",
            ..args()
        })
        .is_some()
    );
}
