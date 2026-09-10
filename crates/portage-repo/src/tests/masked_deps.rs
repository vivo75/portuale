use super::*;

#[test]
fn masked_candidates_for_atom_reports_masked_only_matches() {
    // Portage's `_show_unsatisfied_dep` version match: fires only when every
    // version-matching ebuild is masked.
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
    // A visible candidate silences the report (USE-mismatch takes Portage's
    // other paths).
    assert_eq!(
        masked_candidates_for_atom(&repos, "dev-libs/newpkg", &config),
        None
    );
    // Nothing matching by version at all is also silent (Portage's plain
    // "no ebuilds" case).
    assert_eq!(
        masked_candidates_for_atom(&repos, "dev-libs/doesnotexist-anywhere", &config),
        None
    );
}

#[test]
fn atom_all_use_unmasked_is_true_for_the_unsatuseor_alternative() {
    // Portage's `all_use_unmasked` probe (bug 515584): an off-by-default flag
    // keeps the choice selectable, not demoted to `other`.
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
    // The `other`-demotion side of bug 515584: a `[use]` dep on a use.masked
    // flag is NOT fixable by a USE change.
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
    // Only the masked 2.0 probes false; 1.0 keeps its +x default.
    let masked = atom_all_use_unmasked(&repos, "=dev-libs/slotusetarget-2.0[x]", &config, &[]);
    let unmasked = atom_all_use_unmasked(&repos, "=dev-libs/slotusetarget-1.0[x]", &config, &[]);
    assert!(!masked, "mask-affected x on 2.0 must demote to other");
    assert!(unmasked, "x default-enabled on 1.0 stays selectable");
}

#[test]
fn highest_available_candidate_ignoring_use_picks_the_use_unsatisfiable_ebuild() {
    // Portage's `mydbapi_match_pkgs(atom.without_use)[-1]`: the `[...]` block is
    // stripped for the seed probe.
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
    // Portage's `dep_zapdeps` composite db honours the backtrack
    // `runtime_pkg_mask`, so the probe seeds from the highest unmasked version.
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
