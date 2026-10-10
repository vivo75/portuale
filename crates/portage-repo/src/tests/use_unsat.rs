use super::*;

#[test]
fn use_unsat_candidates_classify_change_use_and_missing_iuse() {
    // Backlog #20: real `_show_unsatisfied_dep`'s two reason kinds and
    // its "Only show the latest version" rule, direct on the fixture
    // repo -- the same rows the contract pins.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(
        &root,
        &root.join("repo"),
        &[("overlay".to_string(), root.join("overlay"))],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let one =
        |atom: &str| use_unsat_candidates_for_atom(&repos, &root, &[], atom, atom, None, &config);
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
    // No use-deps at all: real's other paths own the atom.
    assert_eq!(one("dev-libs/unsatusealt"), None);
}

#[test]
fn use_unsat_candidates_count_conditional_flags_toward_missing_iuse() {
    // Backlog #265(a): real `use.required` is every no-default flag,
    // conditionals included (`lib/portage/dep/__init__.py:1363`
    // `self.required = frozenset(no_default)`), so a conditional flag
    // a visible candidate's own IUSE lacks is `Missing IUSE:`, never
    // a `Change USE:` row and never a plain miss. Live real 3.0.82.2
    // staged-fixture probes
    // (`LLM/evidence/2026-09-30-265/real/mia0a-{1,2}.txt`):
    // `=dev-libs/mia0a-1` (`mia0b[foo?]`) prints the block with
    // `- dev-libs/mia0b-1::testrepo (Missing IUSE: foo)`, and
    // `=dev-libs/mia0a-2` (`mia0b[foo?,bar]`) prints the same row
    // (not portuale's `Change USE: +bar`).
    let root = fixtures_root();
    let config = portage_profile::resolve_config(
        &root,
        &root.join("repo"),
        &[("overlay".to_string(), root.join("overlay"))],
        &[],
        "testrepo",
        &HashMap::new(),
        &root,
    )
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("repos");
    let scan = |display: &str, evaluated: &str| {
        use_unsat_candidates_for_atom(&repos, &root, &[], display, evaluated, None, &config)
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
