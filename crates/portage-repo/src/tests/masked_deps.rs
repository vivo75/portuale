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
fn invalid_use_conditional_mask_matches_portage_on_all_three_fixture_cases() {
    // Portage 3.0.82.2 masks all three `invalid` fixture cases (oracle against
    // a copy of this same tree).
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
            invalid_dep_reasons(c, cat, pkg, &config),
            vec![expected.to_string()],
            "{cat}/{pkg}"
        );
        assert!(!is_visible(c, cat, pkg, &config), "{cat}/{pkg}");
    }
    // Negative control: `gpcyclea` conditions on `x` with `IUSE="+x"`.
    let gpl = list_candidates(&repos, "dev-libs", "gpcyclea").expect("list_candidates");
    let g = gpl.iter().find(|c| c.version == "1.0").expect("version");
    assert!(invalid_dep_reasons(g, "dev-libs", "gpcyclea", &config).is_empty());
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
fn dep_validation_covers_forms_nesting_and_order() {
    // Ported from the retired `first_invalid_conditional` scan: the same
    // encounter order through `validate_dep_string` (EAPI 8, `x` valid).
    // Portage: `use_reduce(matchall=True, flat=True, token_class=Atom,
    // is_valid_flag=…)` (dep:700-715, 1031-1041).
    let text = |dep: &str| validate_dep_string(dep, "8", &|flag| flag == "x").map_err(|e| e.text);
    // Clean strings pass, whatever the form.
    for dep in [
        "dev-libs/a[x?] dev-libs/b",
        "x? ( dev-libs/a )",
        "dev-libs/a[!x?] dev-libs/b[x=]",
        "|| ( dev-libs/a dev-libs/b )",
        "dev-libs/a[x(+)?]",
    ] {
        assert!(text(dep).is_ok(), "{dep}");
    }
    // Every conditional form reports with Portage's conditional text.
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
        assert_eq!(text(dep).err().as_deref(), Some(msg), "{dep}");
    }
    // Encounter order: an atom before a group reports the atom (Portage
    // validates each token as scanned, groups at their `)` close).
    assert_eq!(
        text("dev-libs/a[y?] z? ( dev-libs/b )").err().as_deref(),
        Some("USE flag 'y' referenced in conditional 'y?' in atom 'dev-libs/a[y?]' is not in IUSE")
    );
    assert_eq!(
        text("z? ( dev-libs/b ) dev-libs/a[y?]").err().as_deref(),
        Some("USE flag 'z' referenced in conditional 'z?' is not in IUSE")
    );
    // Nested groups validate innermost first.
    assert_eq!(
        text("z? ( y? ( dev-libs/b ) )").err().as_deref(),
        Some("USE flag 'y' referenced in conditional 'y?' is not in IUSE")
    );
}

#[test]
fn binary_masked_report_lists_masked_binaries_like_the_ebuild_block() {
    // S0 cell `F-binary-rewritten`: every version-matching binary masked
    // reports its rows under the same header Portage prints
    // (`emerge -pv --usepkgonly=y app-misc/t341-good`, container
    // `localhost/test-portuale:latest`).
    let binary = Candidate {
        source: CandidateSource::Binary,
        binary_deps: HashMap::from([(
            "RDEPEND".to_string(),
            "dev-libs/t341-dep::gentoo".to_string(),
        )]),
        binary_use: Some(HashSet::new()),
        eapi: "8".to_string(),
        repo_name: "t341".to_string(),
        ..candidate("1", &["amd64"])
    };
    let config = test_config();
    let report = all_binary_masked_report(
        std::slice::from_ref(&binary),
        "app-misc/t341-good",
        &config,
        "\"app-misc/t341-good\"",
    )
    .expect("masked binary reports");
    assert!(
        report.contains(
            "- app-misc/t341-good-1::t341 (masked by: invalid: RDEPEND: \
             Invalid atom (dev-libs/t341-dep::gentoo), token 1)"
        ),
        "{report}"
    );
    // A visible binary reports nothing (the resolve proceeds instead).
    let mut visible = binary.clone();
    visible.binary_deps = HashMap::from([("RDEPEND".to_string(), "dev-libs/t341-dep".to_string())]);
    assert!(
        all_binary_masked_report(
            std::slice::from_ref(&visible),
            "app-misc/t341-good",
            &config,
            "\"app-misc/t341-good\"",
        )
        .is_none()
    );
}

/// An `=`+build-id `package.mask` line reports exactly the masked binary
/// instance: the atom matches only the equal build-id, and the reasons
/// name `package.mask`./// Portage 3.0.82.2, host run 2026-10-12 (S9 oracle cell
/// `H-bid-mask-atom-masked`, rc 1):
/// `!!! All ebuilds that could satisfy "=dev-libs/t341-dep-1-2" …` with
/// `- dev-libs/t341-dep-1::t341 (masked by: package.mask)`.
#[test]
fn binary_masked_report_for_build_id_atom_lists_only_the_masked_instance() {
    let instance = |bid: &str| Candidate {
        source: CandidateSource::Binary,
        binary_use: Some(HashSet::new()),
        eapi: "8".to_string(),
        repo_name: "t341".to_string(),
        build_id: Some(bid.to_string()),
        build_time: Some(if bid == "2" { 2000 } else { 1000 }),
        slot: "0".to_string(),
        sub_slot: "1".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let mut config = test_config();
    config.package_mask = vec!["=dev-libs/x-1.0-2".to_string()];
    let binaries = vec![instance("1"), instance("2")];
    let report = all_binary_masked_report(
        &binaries,
        "=dev-libs/x-1.0-2",
        &config,
        "\"=dev-libs/x-1.0-2\"",
    )
    .expect("the masked instance reports");
    assert!(
        report.contains("- dev-libs/x-1.0::t341 (masked by: package.mask)"),
        "{report}"
    );
}

/// An unsupported-EAPI candidate is invisible with the lone
/// `EAPI <eapi>` reason — ebuild and binary alike — and the masked block
/// carries Portage's upgrade paragraph.
/// Portage 3.0.82.2, host S0 cells `d-eapi99-pret` (`- …::t348
/// (masked by: EAPI 99)`) and `d-binpkg-eapi99` (binary row without
/// `::repo`, same upgrade text).
#[test]
fn unsupported_eapi_candidate_masks_by_eapi_alone() {
    let config = test_config();
    let mut ebuild = candidate("1", &["amd64"]);
    ebuild.eapi = "99".to_string();
    assert!(!is_visible(&ebuild, "app-misc", "t348-d-eapi99", &config));
    assert_eq!(
        candidate_masking_reasons(&ebuild, "app-misc", "t348-d-eapi99", &config),
        vec!["EAPI 99".to_string()]
    );
    let mut alpha = candidate("1", &["amd64"]);
    alpha.eapi = "7_alpha".to_string();
    assert_eq!(
        candidate_masking_reasons(&alpha, "app-misc", "t348-d-eapi7a", &config),
        vec!["EAPI 7_alpha".to_string()]
    );
    // The EAPI mask is exclusive: never "masked by KEYWORDS/mask alone",
    // never relaxed.
    assert!(!keyword_masked_only(
        &ebuild,
        "app-misc",
        "t348-d-eapi99",
        &config
    ));
    assert!(!mask_masked_only(
        &ebuild,
        "app-misc",
        "t348-d-eapi99",
        &config
    ));
    assert!(!license_masked_only(
        &ebuild,
        "app-misc",
        "t348-d-eapi99",
        &config
    ));
    assert!(!visible_with_relax(VisibleWithRelaxArgs {
        candidate: &ebuild,
        category: "app-misc",
        package: "t348-d-eapi99",
        config: &config,
        relax_license: true,
        relax_keywords: true,
        relax_masks: true,
    }));
    // A binary with an unknown repo prints the bare cpv, like Portage.
    let mut binary = ebuild.clone();
    binary.source = CandidateSource::Binary;
    binary.binary_deps = HashMap::new();
    binary.binary_use = Some(HashSet::new());
    binary.repo_name = "__unknown__".to_string();
    let report = all_binary_masked_report(
        std::slice::from_ref(&binary),
        "=app-misc/t348-d-eapi99-1",
        &config,
        "\"=app-misc/t348-d-eapi99-1\"",
    )
    .expect("EAPI-masked binary reports");
    assert!(
        report.contains("- app-misc/t348-d-eapi99-1 (masked by: EAPI 99)\n"),
        "{report}"
    );
    assert!(
        report.contains("The current version of portage supports EAPI '9'."),
        "{report}"
    );
    // A repo-known EAPI-masked binary is bare too (host 3.0.82.2 prints
    // `- app-misc/t348-d-good-1 (masked by: EAPI 99)` with `REPO: t348`
    // in the stanza; a keyword-masked twin keeps `::t348`).    // A repo-known EAPI-masked binary is bare too (host 3.0.82.2 prints
    // `- app-misc/t348-d-good-1 (masked by: EAPI 99)` with `REPO: t348`
    // in the stanza; a keyword-masked twin keeps `::t348`).
    binary.repo_name = "t348".to_string();
    let report = all_binary_masked_report(
        std::slice::from_ref(&binary),
        "=app-misc/t348-d-eapi99-1",
        &config,
        "\"=app-misc/t348-d-eapi99-1\"",
    )
    .expect("EAPI-masked binary reports");
    assert!(
        report.contains("- app-misc/t348-d-eapi99-1 (masked by: EAPI 99)\n"),
        "{report}"
    );
    // ... while a repo-known binary keeps its `::repo` suffix for any
    // other mask (Portage prints `::t341` for the S0 `F-binary-rewritten`
    // invalid-RDEPEND row).
    binary.repo_name = "t348".to_string();
    binary.eapi = "8".to_string();
    binary.binary_deps = HashMap::from([("RDEPEND".to_string(), "dev-libs/x::gentoo".to_string())]);
    let report = all_binary_masked_report(
        std::slice::from_ref(&binary),
        "=app-misc/t348-d-eapi99-1",
        &config,
        "\"=app-misc/t348-d-eapi99-1\"",
    )
    .expect("masked binary reports");
    assert!(
        report.contains("- app-misc/t348-d-eapi99-1::t348 (masked by: invalid:"),
        "{report}"
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

/// Backlog #420 cell B: under `--binpkg-changed-deps`, a binary built
/// against since-changed deps reports Portage's `changed deps` masked row
/// instead of vanishing into the plain `no binary packages` miss.
/// Without the gate the same visible binary is selectable (no rows).
///
/// Portage 3.0.82.2, s10x cell B on the real fixtures (rc 1, full argv
/// `emerge -pv --package-moves=n --usepkgonly=y --binpkg-changed-deps=y
/// dev-libs/bcdeppkg`): tree `RDEPEND dev-libs/bcdepnew`, local binary
/// `RDEPEND dev-libs/bcdepold`, stderr `- dev-libs/bcdeppkg-1.0::testrepo
/// (masked by: changed deps)`.
#[test]
fn binary_masked_rows_report_changed_deps_rejections() {
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let config = test_config();
    let root = TempDir::new("portuale-420-changed-deps").keep();
    let binary = Candidate {
        source: CandidateSource::Binary,
        binary_deps: HashMap::from([("RDEPEND".to_string(), "dev-libs/bcdepold".to_string())]),
        binary_use: Some(HashSet::new()),
        eapi: "8".to_string(),
        repo_name: "testrepo".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let no_gate = || BinaryRespectUseGate {
        respect_use: false,
        newuse: false,
        changed_use: false,
        ebuilds: &[],
        changed_deps: None,
    };
    // Gate off: the visible binary is selectable, so no rows.
    assert!(
        binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
            binaries: std::slice::from_ref(&binary),
            atom_str: "dev-libs/bcdeppkg",
            config: &config,
            gate: no_gate(),
        })
        .is_none()
    );
    // Gate on: the stale binary reports one `changed deps` row.
    let rows = binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
        binaries: std::slice::from_ref(&binary),
        atom_str: "dev-libs/bcdeppkg",
        config: &config,
        gate: BinaryRespectUseGate {
            changed_deps: Some(BinaryChangedDepsGate {
                root: &root,
                repos: &repos,
                with_bdeps: false,
            }),
            ..no_gate()
        },
    })
    .expect("stale binary reports");
    assert_eq!(
        rows,
        vec![(
            "dev-libs/bcdeppkg-1.0::testrepo".to_string(),
            vec!["changed deps".to_string()]
        )]
    );
    let _ = fs::remove_dir_all(&root);
}

/// Backlog #420: a binary rejected by both gates reports Portage's first
/// reason (`respect_use` before `changed_deps`, the `elif` order at
/// depgraph.py:6655-6671), never both.
#[test]
fn binary_masked_rows_prefer_respect_use_over_changed_deps() {
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    // The user enables `x`, the binary was baked without it (the #402
    // mismatch shape), and its RDEPEND is stale too (the #420 shape).
    let mut config = test_config();
    config.package_use_user = vec![("=dev-libs/bcdeppkg-1.0".to_string(), vec!["x".to_string()])];
    let root = TempDir::new("portuale-420-gate-order").keep();
    let binary = Candidate {
        source: CandidateSource::Binary,
        binary_deps: HashMap::from([("RDEPEND".to_string(), "dev-libs/bcdepold".to_string())]),
        binary_use: Some(HashSet::new()),
        eapi: "8".to_string(),
        iuse: "x".to_string(),
        repo_name: "testrepo".to_string(),
        ..candidate("1.0", &["amd64"])
    };
    let rows = binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
        binaries: std::slice::from_ref(&binary),
        atom_str: "dev-libs/bcdeppkg",
        config: &config,
        gate: BinaryRespectUseGate {
            respect_use: true,
            newuse: false,
            changed_use: false,
            ebuilds: &[],
            changed_deps: Some(BinaryChangedDepsGate {
                root: &root,
                repos: &repos,
                with_bdeps: false,
            }),
        },
    })
    .expect("twice-rejected binary reports");
    assert_eq!(
        rows,
        vec![(
            "dev-libs/bcdeppkg-1.0::testrepo".to_string(),
            vec!["use flag configuration mismatch".to_string()]
        )]
    );
    let _ = fs::remove_dir_all(&root);
}

/// Backlog #420: the dependency disclosure merges the ebuild and binary
/// pools into one block, cpv-deduped like Portage's `show_masked_packages`
/// (`shown_cpvs`) — ebuild rows first, a binary twin of a listed row dropped.
#[test]
fn merge_masked_rows_dedups_binary_twins_of_ebuild_rows() {
    let merged = merge_masked_rows(
        vec![(
            "dev-libs/x-1.0::testrepo".to_string(),
            vec!["package.mask".to_string()],
        )],
        vec![
            (
                "dev-libs/x-1.0::testrepo".to_string(),
                vec!["changed deps".to_string()],
            ),
            (
                "dev-libs/x-1.0::other".to_string(),
                vec!["changed deps".to_string()],
            ),
        ],
    );
    assert_eq!(
        merged,
        vec![
            (
                "dev-libs/x-1.0::testrepo".to_string(),
                vec!["package.mask".to_string()],
            ),
            (
                "dev-libs/x-1.0::other".to_string(),
                vec!["changed deps".to_string()],
            ),
        ]
    );
}
