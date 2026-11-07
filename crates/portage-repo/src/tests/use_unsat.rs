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

#[test]
fn show_masked_packages_prints_the_mask_file_comment_once_and_license_paths() {
    // Backlog #611(c): Portage's `show_masked_packages` (depgraph.py
    // D:12632-12690) prints, under a `package.mask` row, the masking file
    // and the comment above the entry (once per distinct comment), and a
    // line per missing license found under a repo's `licenses/`. Portage
    // 3.0.83 prints the fixture's user `package.mask` comment under
    // `maskeddep-1.0` (`LLM/evidence/2026-11-06-sweep/611c/`).
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
    let gate = crate::test_gate(&repos);
    let rows = masked_candidates_for_atom(gate, "dev-libs/maskeddep", &config).expect("masked");
    let pool = gate.repo_candidates("dev-libs", "maskeddep").expect("pool");
    let notes = masked_row_notes(&pool, &repos, &rows, &config);
    let (path, comment) = notes[0].mask_comment.clone().expect("a comment");
    assert_eq!(path, root.join("etc/portage/package.mask"));
    assert!(
        comment
            .last()
            .is_some_and(|l| l.contains("have been masked\" block")),
        "{comment:?}"
    );
    // Two rows sharing one comment print it once; a license prints once.
    let lic = std::path::PathBuf::from("/r/licenses/EULA");
    let note = MaskedRowNotes {
        mask_comment: Some((path.clone(), comment.clone())),
        licenses: vec![("EULA".to_string(), lic)],
    };
    let two = vec![
        ("a/b-2::r".to_string(), vec!["package.mask".to_string()]),
        ("a/b-1::r".to_string(), vec!["package.mask".to_string()]),
    ];
    let text = show_masked_packages(&two, &[note.clone(), note]);
    let mut want = String::from("- a/b-2::r (masked by: package.mask)\n");
    want.push_str(&format!("{}:\n", path.display()));
    for line in &comment {
        want.push_str(&format!("{line}\n"));
    }
    want.push('\n');
    want.push_str("A copy of the 'EULA' license is located at '/r/licenses/EULA'.\n\n");
    want.push_str("- a/b-1::r (masked by: package.mask)\n");
    assert_eq!(text, want);
    // No notes: rows only.
    assert_eq!(
        show_masked_packages(&two[..1], &[]),
        "- a/b-2::r (masked by: package.mask)\n"
    );
}
