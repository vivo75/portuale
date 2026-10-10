use super::*;

#[test]
fn masked_candidates_for_atom_reports_masked_only_matches() {
    // Real `_show_unsatisfied_dep`'s version match: the masked list
    // fires when every version-matching ebuild is masked, and stays
    // silent when something is visible or nothing matches at all.
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
    let masked = masked_candidates_for_atom(&repos, "dev-libs/maskeddep", &config)
        .expect("package.mask'd dep discloses");
    assert_eq!(
        masked,
        vec![(
            "dev-libs/maskeddep-1.0::testrepo".to_string(),
            vec!["package.mask".to_string()]
        )]
    );
    let kw = masked_candidates_for_atom(&repos, "dev-libs/kwmaskeddep", &config)
        .expect("keyword-masked dep discloses");
    assert_eq!(
        kw,
        vec![(
            "dev-libs/kwmaskeddep-1.0::testrepo".to_string(),
            vec!["~amd64 keyword".to_string()]
        )]
    );
    // A visible candidate silences the report (USE-mismatch-only and
    // friends take real's other paths).
    assert_eq!(
        masked_candidates_for_atom(&repos, "dev-libs/newpkg", &config),
        None
    );
    // Nothing matching by version at all: real's plain "no ebuilds"
    // case, also silent here.
    assert_eq!(
        masked_candidates_for_atom(&repos, "dev-libs/doesnotexist-anywhere", &config),
        None
    );
}

#[test]
fn invalid_use_conditional_mask_matches_portage_on_all_three_fixture_cases() {
    // Backlog #153: real 3.0.82.2 masks all three `invalid` (oracles
    // captured against a `cp -a` copy of this same tree) --
    // `fucyclec` (atom `[x?]` in DEPEND), `usedeppkg` (atom
    // `[baz(+)?]` in RDEPEND -- a defaulted conditional is still
    // validated), `somercurial` (`soflag? ( … )` group in RDEPEND).
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
    for (cat, pkg, version, expected) in [
        (
            "dev-libs",
            "fucyclec",
            "1.0",
            "invalid: DEPEND: USE flag 'x' referenced in conditional 'x?' in atom 'dev-libs/fucyclea[x?]' is not in IUSE",
        ),
        (
            "dev-libs",
            "usedeppkg",
            "1.0",
            "invalid: RDEPEND: USE flag 'baz' referenced in conditional 'baz?' in atom 'dev-libs/multislotpkg:1[baz(+)?]' is not in IUSE",
        ),
        (
            "dev-vcs",
            "somercurial",
            "5.5.1",
            "invalid: RDEPEND: USE flag 'soflag' referenced in conditional 'soflag?' is not in IUSE",
        ),
    ] {
        let candidates = list_candidates(&repos, cat, pkg).expect("list_candidates");
        let c = candidates
            .iter()
            .find(|c| c.version == version)
            .expect("fixture version");
        assert_eq!(
            invalid_use_conditional_reasons(c, cat, pkg, &config),
            vec![expected.to_string()],
            "{cat}/{pkg}"
        );
        assert!(!is_visible(c, cat, pkg, &config), "{cat}/{pkg}");
    }
    // Negative control: `gpcyclea` conditions on `x` with `IUSE="+x"`.
    let gpl = list_candidates(&repos, "dev-libs", "gpcyclea").expect("list_candidates");
    let g = gpl.iter().find(|c| c.version == "1.0").expect("version");
    assert!(invalid_use_conditional_reasons(g, "dev-libs", "gpcyclea", &config).is_empty());
    assert!(is_visible(g, "dev-libs", "gpcyclea", &config));
    // End to end: the masked-candidates disclosure carries the reason.
    let masked = masked_candidates_for_atom(&repos, "dev-libs/fucyclec", &config)
        .expect("invalid dep discloses");
    assert_eq!(
        masked,
        vec![(
            "dev-libs/fucyclec-1.0::testrepo".to_string(),
            vec!["invalid: DEPEND: USE flag 'x' referenced in conditional 'x?' in atom 'dev-libs/fucyclea[x?]' is not in IUSE".to_string()]
        )]
    );
}

#[test]
fn first_invalid_conditional_covers_forms_nesting_and_order() {
    let valid: HashSet<String> = ["x".to_string()].into_iter().collect();
    // Clean strings pass, whatever the form.
    for dep in [
        "dev-libs/a[x?] dev-libs/b",
        "x? ( dev-libs/a )",
        "dev-libs/a[!x?] dev-libs/b[x=]",
        "|| ( dev-libs/a dev-libs/b )",
        "dev-libs/a[x(+)?]",
    ] {
        assert_eq!(first_invalid_conditional(dep, &valid), None, "{dep}");
    }
    // Every conditional form reports with real's conditional text.
    for (dep, msg) in [
        (
            "dev-libs/a[y?]",
            "USE flag 'y' referenced in conditional 'y?' in atom 'dev-libs/a[y?]' is not in IUSE",
        ),
        (
            "dev-libs/a[!y?]",
            "USE flag 'y' referenced in conditional '!y?' in atom 'dev-libs/a[!y?]' is not in IUSE",
        ),
        (
            "dev-libs/a[y=]",
            "USE flag 'y' referenced in conditional 'y=' in atom 'dev-libs/a[y=]' is not in IUSE",
        ),
        (
            "dev-libs/a[!y=]",
            "USE flag 'y' referenced in conditional '!y=' in atom 'dev-libs/a[!y=]' is not in IUSE",
        ),
        (
            "y? ( dev-libs/a )",
            "USE flag 'y' referenced in conditional 'y?' is not in IUSE",
        ),
        (
            "!y? ( dev-libs/a )",
            "USE flag 'y' referenced in conditional '!y?' is not in IUSE",
        ),
    ] {
        assert_eq!(
            first_invalid_conditional(dep, &valid).as_deref(),
            Some(msg),
            "{dep}"
        );
    }
    // Encounter order: an atom before a group reports the atom (real
    // validates each token as scanned, groups at their `)` close).
    assert_eq!(
        first_invalid_conditional("dev-libs/a[y?] z? ( dev-libs/b )", &valid).as_deref(),
        Some("USE flag 'y' referenced in conditional 'y?' in atom 'dev-libs/a[y?]' is not in IUSE")
    );
    assert_eq!(
        first_invalid_conditional("z? ( dev-libs/b ) dev-libs/a[y?]", &valid).as_deref(),
        Some("USE flag 'z' referenced in conditional 'z?' is not in IUSE")
    );
    // Nested groups validate innermost first.
    assert_eq!(
        first_invalid_conditional("z? ( y? ( dev-libs/b ) )", &valid).as_deref(),
        Some("USE flag 'y' referenced in conditional 'y?' is not in IUSE")
    );
}

#[test]
fn atom_all_use_unmasked_is_true_for_the_unsatuseor_alternative() {
    // `dev-libs/unsatuseor`'s `||` group's first alternative is
    // `dev-libs/unsatusealt[unsatuseorflag]`: unsatusealt is visible
    // but the flag is off by default (IUSE declares it, nothing
    // enables it), and `unsatuseorflag` is in NO use.mask /
    // use.force profile file -- so real's bug-515584 probe
    // (`all_use_unmasked`) must keep the choice in the selectable
    // `unsat_use_*` bins, not demote it to `other`. The second
    // alternative (dev-libs/doesnotexist-unsatuseor) has no visible
    // ebuild at all.
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
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    assert!(atom_all_use_unmasked(
        &repos,
        "dev-libs/unsatusealt[unsatuseorflag]",
        &config,
        &[]
    ));
}

#[test]
fn atom_all_use_unmasked_is_false_when_the_violated_flag_is_in_use_mask() {
    // The `other`-demotion side of bug-515584: a `[use]` dep on a
    // flag that sits in the candidate's own use.mask is NOT "fixable
    // by a USE change", so the whole `||` choice drops out of every
    // selectable bin. `dev-libs/slotusetarget` has `=...-2.0` mask `x`
    // in the fixture profile (fixtures/repo/profiles/package.use.mask)
    // -- see the slotconflict fixture tests for the matching side.
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
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    // The 1.0 version keeps its +x default (unmasked) -- only the
    // masked 2.0 should probe false.
    let masked = atom_all_use_unmasked(&repos, "=dev-libs/slotusetarget-2.0[x]", &config, &[]);
    let unmasked = atom_all_use_unmasked(&repos, "=dev-libs/slotusetarget-1.0[x]", &config, &[]);
    assert!(!masked, "mask-affected x on 2.0 must demote to other");
    assert!(unmasked, "x default-enabled on 1.0 stays selectable");
}

#[test]
fn highest_available_candidate_ignoring_use_picks_the_use_unsatisfiable_ebuild() {
    // Real `mydbapi_match_pkgs(atom.without_use)[-1]`: asking for
    // `dev-libs/unsatusealt[unsatuseorflag]` must still find
    // unspecified-use candidate `unsatusealt-1.0` (the `[...]` block
    // stripped) -- the seed for both the mask/force probe and the
    // violated-flag list. The hard dep would find NO candidate
    // (`doesnotexist-unsatuseor`).
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
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let (candidate_str, keywords, iuse, use_flags, slot) =
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos: &repos,
            atom_str: "dev-libs/unsatusealt[unsatuseorflag]",
            category: "dev-libs",
            package: "unsatusealt",
            config: &config,
            extra_constraints: &[],
        })
        .expect("an ignoring-use candidate exists");
    assert_eq!(
        candidate_str, "dev-libs/unsatusealt-1.0:0/0::testrepo",
        "the sole visible version is found"
    );
    assert!(keywords.contains(&"amd64".to_string()));
    assert!(iuse.contains("unsatuseorflag"));
    assert!(!use_flags.contains("unsatuseorflag"), "off by default");
    assert_eq!(slot, "0");

    assert!(
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos: &repos,
            atom_str: "dev-libs/doesnotexist-unsatuseor",
            category: "dev-libs",
            package: "doesnotexist-unsatuseor",
            config: &config,
            extra_constraints: &[]
        })
        .is_none()
    );
}

#[test]
fn highest_available_candidate_ignoring_use_honours_extra_constraints() {
    // F5: real's `mydbapi` in `dep_zapdeps` is `_dep_check_composite_db`,
    // which honours the `'backtrack` loop's accumulated
    // `runtime_pkg_mask` -- so once backtracking excludes the highest
    // version of a `cat/pkg`, the bug-515584 probe must be seeded from
    // the highest *unmasked* version instead. `dev-libs/slotusetarget`
    // has two visible versions (1.0, 2.0); a negative constraint
    // excluding 2.0 must fall back to 1.0.
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
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let (unconstrained, ..) =
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos: &repos,
            atom_str: "dev-libs/slotusetarget[x]",
            category: "dev-libs",
            package: "slotusetarget",
            config: &config,
            extra_constraints: &[],
        })
        .expect("an ignoring-use candidate exists");
    assert_eq!(unconstrained, "dev-libs/slotusetarget-2.0:0/0::testrepo");

    let constraints = vec!["!=dev-libs/slotusetarget-2.0".to_string()];
    let (constrained, ..) =
        highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
            repos: &repos,
            atom_str: "dev-libs/slotusetarget[x]",
            category: "dev-libs",
            package: "slotusetarget",
            config: &config,
            extra_constraints: &constraints,
        })
        .expect("an ignoring-use candidate exists under the constraint");
    assert_eq!(
        constrained, "dev-libs/slotusetarget-1.0:0/0::testrepo",
        "the highest UNMASKED candidate is returned once backtracking excludes 2.0"
    );
}
