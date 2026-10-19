use super::*;

#[test]
fn autounmask_suggests_a_keyword_only_when_enabled() {
    // `~amd64`-only, masked by KEYWORDS alone: the keyword suggestion shape. With
    // suggestion off (Portage's default) nothing is appended.
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
    let atoms = vec!["dev-libs/autounmaskkeywordpkg".to_string()];
    let err_without_suggestion = ResolveRequest::new(
        &root,
        &root,
        &atoms,
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err("no visible candidate at all");
    // Keyword-masked-only renders the "have been masked" block, not the bare miss.
    assert_eq!(
        err_without_suggestion.to_string(),
        "\n!!! All ebuilds that could satisfy \"dev-libs/autounmaskkeywordpkg\" \
         have been masked.\n\
         !!! One of the following masked packages is required to complete your request:\n\
         - dev-libs/autounmaskkeywordpkg-1.0::testrepo (masked by: ~amd64 keyword)\n\
         \nFor more information, see the MASKED PACKAGES section in the emerge\n\
         man page or refer to the Gentoo Handbook.\n"
    );

    // With `--autounmask` the target resolves as `New` and the change is recorded
    // for the keyword-changes block (Portage applies the implicit `=cpv ~arch`).
    let resolved = graph_result_autounmask("dev-libs/autounmaskkeywordpkg");
    let entry = &resolved.entries[0];
    assert!(matches!(entry.outcome, PretendOutcome::New { .. }));
    assert_eq!(entry.keyword_mask, Some('~'));
    assert_eq!(resolved.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        resolved.autounmask_keyword_changes[0].atom,
        "=dev-libs/autounmaskkeywordpkg-1.0"
    );
    assert_eq!(resolved.autounmask_keyword_changes[0].token, "~amd64");
}

#[test]
fn autounmask_use_suggests_a_flag_flip_only_when_enabled() {
    // Masked by one USE-dep alone: the use suggestion shape. With suggestion off
    // (Portage's default) the abort is Portage's USE block, not the bare miss.
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
    let atoms = vec!["dev-libs/useflagpkg[-foo]".to_string()];
    let err_without_suggestion = ResolveRequest::new(
        &root,
        &root,
        &atoms,
        &config,
        &fixtures_root().join("distfiles"),
    )
    .resolve()
    .expect_err("no visible candidate at all");
    assert_eq!(
        err_without_suggestion.to_string(),
        "there are no ebuilds built with USE flags to satisfy \
         \"dev-libs/useflagpkg[-foo]\".\n\
         !!! One of the following packages is required to complete your request:\n\
         - dev-libs/useflagpkg-1.0::testrepo (Change USE: -foo)"
    );

    // With `--autounmask-use` (on by default) the atom resolves via the flip.
    let resolved = graph_result_autounmask("dev-libs/useflagpkg[-foo]");
    let entry = resolved
        .entries
        .iter()
        .find(|e| e.package == "useflagpkg")
        .expect("resolves via the flip");
    assert!(matches!(entry.outcome, PretendOutcome::New { .. }));
    assert_eq!(resolved.autounmask_use_changes.len(), 1);
    assert_eq!(
        resolved.autounmask_use_changes[0].atom,
        ">=dev-libs/useflagpkg-1.0"
    );
    assert_eq!(resolved.autounmask_use_changes[0].token, "-foo");
}

#[test]
fn fixture_dependency_use_dep_mismatch_resolves_via_the_flip_once_autounmask_use_is_on() {
    // With `--autounmask-use` on the dependency resolves (was `NoVisibleCandidate`
    // with only a `use_suggestion` before flip resolution shipped).
    let result = graph_result_autounmask("dev-libs/usedeprejectedpkg");
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "useflagpkg")
        .expect("dependency entry present");
    assert!(matches!(dep.outcome, PretendOutcome::New { .. }));
    assert_eq!(
        dep.use_flags_display
            .iter()
            .find(|(f, _)| f == "foo")
            .map(|(_, on)| *on),
        Some(false)
    );
    assert_eq!(result.autounmask_use_changes.len(), 1);
}

#[test]
fn autounmask_license_resolves_a_eula_masked_dependency() {
    // EULA-masked dep: resolves with `--autounmask-license`, license recorded.
    let result = graph_result_autounmask("dev-libs/licensemaskedconsumer");
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "licensemaskedpkg")
        .expect("the EULA-masked dependency is a real graph entry now");
    assert!(matches!(dep.outcome, PretendOutcome::New { .. }));
    assert_eq!(result.autounmask_license_changes.len(), 1);
    let change = &result.autounmask_license_changes[0];
    assert_eq!(change.atom, ">=dev-libs/licensemaskedpkg-1.0");
    assert_eq!(change.token, "SomeEula");
    assert_eq!(
        change.dep_chain,
        vec![
            "required by dev-libs/licensemaskedconsumer-1.0::testrepo".to_string(),
            "required by dev-libs/licensemaskedconsumer (argument)".to_string(),
        ]
    );

    // Off by default: the dep stays `NoVisibleCandidate`, nothing recorded.
    let plain = graph_result_real("dev-libs/licensemaskedconsumer");
    assert!(plain.autounmask_license_changes.is_empty());
    assert!(plain.entries.iter().any(|e| e.package == "licensemaskedpkg"
        && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)));
}

#[test]
fn autounmask_keep_masks_n_unmasks_a_package_mask() {
    // package.mask'd dep: resolves with masks on, `=<cpv>` change recorded.
    let result = graph_result_autounmask("dev-libs/maskmaskedconsumer");
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "hardmaskedpkg")
        .expect("the package.mask'd dependency is a real graph entry now");
    assert!(matches!(dep.outcome, PretendOutcome::New { .. }));
    assert_eq!(result.autounmask_mask_changes.len(), 1);
    let change = &result.autounmask_mask_changes[0];
    assert_eq!(change.atom, "=dev-libs/hardmaskedpkg-1.0");
    assert_eq!(change.token, "");

    // Off by default: the dep stays `NoVisibleCandidate`.
    let plain = graph_result_real("dev-libs/maskmaskedconsumer");
    assert!(plain.autounmask_mask_changes.is_empty());
    assert!(
        plain.entries.iter().any(|e| e.package == "hardmaskedpkg"
            && matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    );
}

#[test]
fn abort_outcome_membership_and_order() {
    // Expected value from Portage (live captures in fixtures/abort-captures/):
    // the outcome is produced on the settled graph while `entries` stays the
    // full graph.
    //
    // Masked-only dep (§4a): empty partial, the report's display atom, the top
    // as the first merge-bound requirer.
    for top in ["dev-libs/abort-masked-mid", "dev-libs/abort-masked-last"] {
        let result = graph_result_real(top);
        assert_eq!(result.entries.len(), 4, "{top}: full graph kept");
        match &result.outcome {
            ResolveOutcome::Aborted {
                reason: AbortReason::MaskedDep { atom, parent_cpv },
                partial,
            } => {
                assert_eq!(atom, "dev-libs/maskeddep", "{top}");
                assert_eq!(parent_cpv, &format!("{top}-1.0"), "{top}");
                assert!(partial.is_empty(), "{top}: real shows no list at all");
            }
            other => panic!("{top}: expected MaskedDep, got {other:?}"),
        }
    }
    // Nothing matches at all (§4b): same shape, `UnsatisfiedAtom`.
    for top in ["dev-libs/abort-unsat-mid", "dev-libs/abort-unsat-last"] {
        let result = graph_result_real(top);
        match &result.outcome {
            ResolveOutcome::Aborted {
                reason: AbortReason::UnsatisfiedAtom { atom, parent_cpv },
                partial,
            } => {
                assert_eq!(atom, "dev-libs/abort-nonexistent", "{top}");
                assert_eq!(parent_cpv, &format!("{top}-1.0"), "{top}");
                assert!(partial.is_empty(), "{top}");
            }
            other => panic!("{top}: expected UnsatisfiedAtom, got {other:?}"),
        }
    }
    // Unserializable cycle (§4c): the stuck remainder in leaf-drain order, one
    // partial entry per member. The autounmask+cycle fixtures (§4d) match, with
    // the changes only stopping backtracking at try 0.
    for (top, result) in [
        (
            "dev-libs/abort-cycle-mid",
            graph_result_real("dev-libs/abort-cycle-mid"),
        ),
        (
            "dev-libs/abort-cycle-last",
            graph_result_real("dev-libs/abort-cycle-last"),
        ),
        (
            "dev-libs/abort-au-cycle",
            graph_result_autounmask("dev-libs/abort-au-cycle"),
        ),
        (
            "dev-libs/abort-au-restart-cycle",
            graph_result_autounmask("dev-libs/abort-au-restart-cycle"),
        ),
    ] {
        let expected = vec![
            format!("{top}-1.0"),
            "dev-libs/abort-cycle-a-1.0".to_string(),
            "dev-libs/abort-cycle-b-1.0".to_string(),
        ];
        match &result.outcome {
            ResolveOutcome::Aborted {
                reason: AbortReason::UnserializableCycle { members },
                partial,
            } => {
                assert_eq!(members, &expected, "{top}");
                assert_eq!(members, &result.cycle_display, "{top}");
                let partial_cpvs: Vec<String> =
                    partial.iter().filter_map(merge_bound_cpv).collect();
                assert_eq!(
                    partial_cpvs, expected,
                    "{top}: partial follows remainder order"
                );
                assert!(
                    !partial.iter().any(|e| e.package.starts_with("abort-leaf")),
                    "{top}: drained leaves are not in the remainder"
                );
            }
            other => panic!("{top}: expected UnserializableCycle, got {other:?}"),
        }
        assert!(
            result.entries.iter().any(|e| e.package == "abort-leaf-a"),
            "{top}: full graph kept"
        );
    }
    for top in ["dev-libs/abort-au-cycle", "dev-libs/abort-au-restart-cycle"] {
        assert!(
            !graph_result_autounmask(top)
                .autounmask_use_changes
                .is_empty(),
            "{top}: the autounmask change is still reported alongside the cycle"
        );
    }
    // With `--autounmask-use=n` the unsatisfiable dep wins over the cycle (spec
    // §4e: Portage prints the USE-flags block, no list).
    assert!(matches!(
        graph_result_autounmask_use_n("dev-libs/abort-au-cycle").outcome,
        ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom { .. },
            ..
        }
    ));
    // Walk-time failure beats the serialize-time one in the same graph.
    let result = graph_result_real("dev-libs/abort-masked-cycle");
    assert!(!result.circular_deps.is_empty());
    assert!(matches!(
        result.outcome,
        ResolveOutcome::Aborted {
            reason: AbortReason::MaskedDep { .. },
            ..
        }
    ));
    // Changes alone never abort (Portage shows the full list).
    for top in ["dev-libs/abort-au-plain", "dev-libs/aucasctop"] {
        assert_eq!(
            graph_result_autounmask(top).outcome,
            ResolveOutcome::Complete,
            "{top}"
        );
    }
    for top in ["dev-libs/newpkg", "dev-libs/diamond"] {
        assert_eq!(
            graph_result_real(top).outcome,
            ResolveOutcome::Complete,
            "{top}"
        );
    }
}

#[test]
fn autounmask_backtrack_disabled_gate() {
    // Both branches of the "terminated early" gate, expected value from Portage
    // (live captures in fixtures/abort-captures/).
    let plain = graph_result_autounmask("dev-libs/abort-au-plain");
    assert!(
        !plain.autounmask_use_changes.is_empty(),
        "plain: the change block is still reported"
    );
    assert!(
        !plain.autounmask_no_clean_tail,
        "plain: fresh-candidate flip, the tail stays clean"
    );
    assert!(
        !plain.autounmask_backtrack_disabled(false),
        "plain: lone change prints no notice"
    );
    // Cycle keeps the notice via the outcome arm, restart-wanted via the
    // accumulator arm.
    let cycle = graph_result_autounmask("dev-libs/abort-au-cycle");
    assert!(
        !cycle.autounmask_use_changes.is_empty(),
        "cycle: the change block is still reported alongside"
    );
    assert!(
        cycle.autounmask_backtrack_disabled(false),
        "cycle: another failure coincides, notice stays"
    );
    let casc = graph_result_autounmask("dev-libs/aucasctop");
    assert!(
        casc.autounmask_no_clean_tail,
        "aucasctop: the flip folds for the already-graphed slot"
    );
    assert!(
        casc.autounmask_backtrack_disabled(false),
        "aucasctop: restart-wanted, notice stays"
    );
    // Post-failure parent collects keep the notice too (Portage applies parent
    // changes before re-probing).
    let aup = graph_result_autounmask("=dev-libs/aup0b-1");
    assert_eq!(
        aup.autounmask_use_changes.len(),
        1,
        "aup0b: the parent flip is still reported"
    );
    assert!(
        aup.autounmask_no_clean_tail,
        "aup0b: the failed dep was collected into a parent flip"
    );
    assert!(
        aup.autounmask_backtrack_disabled(false),
        "aup0b: notice stays"
    );
    // Masked child: no flip recorded at all (Portage skips the parent probe), so
    // no change block and no notice -- the bare miss.
    let flip = graph_result_autounmask("dev-libs/parentflipeqpkg");
    assert!(
        flip.autounmask_use_changes.is_empty(),
        "parentflipeqpkg: no flip is recorded for a masked child"
    );
    assert!(
        !flip.autounmask_no_clean_tail,
        "parentflipeqpkg: nothing collected, the tail stays clean"
    );
    assert!(
        !flip.autounmask_backtrack_disabled(false),
        "parentflipeqpkg: bare miss prints no notice"
    );
    // Same masked-child shape (`use.mask pf`): Portage reports the bare miss under
    // default and `--autounmask-backtrack=y` alike (live probe, rc 1, no rows).
    let result = graph_result_autounmask("dev-libs/pfgraphparent");
    assert!(
        result.autounmask_use_changes.is_empty(),
        "pfgraphparent: no flip is recorded for a masked child"
    );
    assert!(
        !result.autounmask_no_clean_tail,
        "pfgraphparent: nothing collected, the tail stays clean"
    );
    assert!(
        !result.autounmask_backtrack_disabled(false),
        "pfgraphparent: bare miss prints no notice"
    );
    // Config conjuncts suppress unconditionally: `--autounmask-backtrack=y` opts
    // back in, and `--backtrack=0` never sets the flag.
    assert!(
        !graph_result_autounmask_backtrack("dev-libs/abort-au-cycle")
            .autounmask_backtrack_disabled(true),
        "backtrack=y suppresses the notice"
    );
    let mut no_budget = graph_result_autounmask("dev-libs/aucasctop");
    no_budget.backtrack_max = 0;
    assert!(
        !no_budget.autounmask_backtrack_disabled(false),
        "--backtrack=0 never prints the notice"
    );
    // No changes, no notice -- the predicate's first conjunct.
    assert!(
        !graph_result_real("dev-libs/diamond").autounmask_backtrack_disabled(false),
        "a clean resolve prints no notice"
    );
}

#[test]
fn autounmask_cascade_flip_before_dep_walk_pulls_the_gated_leaf() {
    // Portage's DFS applies the late flip before walking the mid package's deps,
    // so the leaf joins the graph in the same attempt; portuale re-expands the
    // flipped package's newly-gated deps in-walk instead.
    let result = graph_result_autounmask("dev-libs/aucasctop");
    let mut pkgs: Vec<&str> = result.entries.iter().map(|e| e.package.as_str()).collect();
    pkgs.sort_unstable();
    assert_eq!(
        pkgs,
        vec!["aucasclate", "aucascleaf", "aucascmid", "aucasctop"],
        "the in-graph flip lands before the flipped node's walk"
    );
    // Genuine merge-order edge to the leaf: deps re-flattened under the flipped USE.
    let mid = result
        .entries
        .iter()
        .find(|e| e.package == "aucascmid")
        .expect("aucascmid is in the graph");
    assert!(
        mid.deps.iter().any(|d| d.package == "aucascleaf"),
        "aucascmid's re-expanded deps name aucascleaf"
    );
    // The change block still records the flip.
    assert!(
        !result.autounmask_use_changes.is_empty(),
        "the [cascade] flip is still reported"
    );
    // No phantom owners: every `required_by` set is exactly the forward-walk
    // owners (Portage carries no flip-landing edge here either).
    for (pkg, want) in [
        ("aucasctop", vec![]),
        (
            "aucascmid",
            vec![
                ("dev-libs".to_string(), "aucasclate".to_string()),
                ("dev-libs".to_string(), "aucasctop".to_string()),
            ],
        ),
        (
            "aucasclate",
            vec![("dev-libs".to_string(), "aucasctop".to_string())],
        ),
        (
            "aucascleaf",
            vec![("dev-libs".to_string(), "aucascmid".to_string())],
        ),
    ] {
        let entry = result
            .entries
            .iter()
            .find(|e| e.package == pkg)
            .unwrap_or_else(|| panic!("{pkg} is in the graph"));
        let mut got = entry.required_by.clone();
        got.sort_unstable();
        assert_eq!(got, want, "{pkg} has exactly its forward-walk owners");
    }
    // First-encounter flip still pulls the leaf with no delta pass involved.
    let late_result = graph_result_autounmask("dev-libs/aucasclate");
    let mut late_pkgs: Vec<&str> = late_result
        .entries
        .iter()
        .map(|e| e.package.as_str())
        .collect();
    late_pkgs.sort_unstable();
    assert_eq!(
        late_pkgs,
        vec!["aucasclate", "aucascleaf", "aucascmid"],
        "first-encounter flip still pulls the leaf"
    );
    // Whole-walk re-drive agrees with the in-walk delta.
    let bt_result = graph_result_autounmask_backtrack("dev-libs/aucasctop");
    let mut bt_pkgs: Vec<&str> = bt_result
        .entries
        .iter()
        .map(|e| e.package.as_str())
        .collect();
    bt_pkgs.sort_unstable();
    assert_eq!(bt_pkgs, pkgs, "backtrack re-drive and in-walk delta agree");
}

#[test]
fn autounmask_keywords_records_the_dep_chain_for_a_keyword_masked_dependency() {
    let result = graph_result_autounmask("dev-libs/autounmaskdepconsumer");
    // Consumer and unmasked dependency both resolve as New.
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "autounmaskkeywordpkg")
        .expect("keyword-masked dep now resolves");
    assert!(matches!(dep.outcome, PretendOutcome::New { .. }));
    assert_eq!(dep.keyword_mask, Some('~'));

    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    let change = &result.autounmask_keyword_changes[0];
    assert_eq!(change.atom, "=dev-libs/autounmaskkeywordpkg-1.0");
    assert_eq!(change.token, "~amd64");
    assert_eq!(
        change.dep_chain,
        vec![
            "required by dev-libs/autounmaskdepconsumer-1.0::testrepo".to_string(),
            "required by dev-libs/autounmaskdepconsumer (argument)".to_string(),
        ]
    );
}

#[test]
fn autounmask_keyword_backward_cascade_re_resolves_a_slot_to_a_masked_version() {
    // The folded `>=2.0` pulls a masked-only candidate, so its keyword is
    // autounmasked and the slot settles on `2.0` (Portage weighs every atom
    // pulling the slot).
    let result = graph_result_autounmask("dev-libs/kwbacktop");
    let mid: Vec<&GraphEntry> = result
        .entries
        .iter()
        .filter(|e| e.package == "kwbackmid")
        .collect();
    assert_eq!(mid.len(), 1, "exactly one kwbackmid entry: {mid:?}");
    assert_eq!(
        mid[0].outcome,
        PretendOutcome::New {
            version: "2.0".to_string()
        }
    );
    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        result.autounmask_keyword_changes[0].atom,
        "=dev-libs/kwbackmid-2.0"
    );
    assert_eq!(result.autounmask_keyword_changes[0].token, "~amd64");
}

#[test]
fn autounmask_levels_unmask_two_categories_at_once_on_the_same_version() {
    // Level 1 (+license) yields nothing; level 2 (+arch +license) unmasks both
    // and the re-scan picks the higher `2.0` (Portage `_autounmask_levels`).
    let result = graph_result_autounmask("dev-libs/multimaskconsumer");
    let dep = result
        .entries
        .iter()
        .find(|e| e.package == "multimaskdep")
        .expect("multimaskdep entry");
    assert_eq!(
        dep.outcome,
        PretendOutcome::New {
            version: "2.0".to_string()
        }
    );
    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        result.autounmask_keyword_changes[0].atom,
        "=dev-libs/multimaskdep-2.0"
    );
    assert_eq!(result.autounmask_keyword_changes[0].token, "~amd64");
    assert_eq!(result.autounmask_license_changes.len(), 1);
    assert_eq!(
        result.autounmask_license_changes[0].atom,
        ">=dev-libs/multimaskdep-2.0"
    );
    assert_eq!(result.autounmask_license_changes[0].token, "SomeEula");
}

#[test]
fn autounmask_keywords_top_level_dep_chain_is_the_argument_line() {
    // The change sits on the argument package: just the `(argument)` line
    // (Portage never prints the start node).
    let result = graph_result_autounmask("dev-libs/autounmaskkeywordpkg");
    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec!["required by dev-libs/autounmaskkeywordpkg (argument)".to_string(),]
    );
}

#[test]
fn autounmask_use_resolves_a_use_dep_mismatch_and_applies_the_flip_to_the_display() {
    // The flip resolves the entry as `New` with `foo` disabled, recorded in the
    // `>=<cpv>` atom form; the dep chain is just the `(argument)` line.
    let result = graph_result_autounmask("dev-libs/useflagpkg[-foo]");
    let entry = result
        .entries
        .iter()
        .find(|e| e.package == "useflagpkg")
        .expect("useflagpkg resolves via the USE flip");
    assert!(matches!(entry.outcome, PretendOutcome::New { .. }));
    assert_eq!(
        entry
            .use_flags_display
            .iter()
            .find(|(f, _)| f == "foo")
            .map(|(_, on)| *on),
        Some(false),
        "the autounmask flip must be reflected in the -pv USE display"
    );
    assert!(result.autounmask_keyword_changes.is_empty());
    assert_eq!(result.autounmask_use_changes.len(), 1);
    let change = &result.autounmask_use_changes[0];
    assert_eq!(change.atom, ">=dev-libs/useflagpkg-1.0");
    assert_eq!(change.token, "-foo");
    assert_eq!(
        change.dep_chain,
        vec!["required by dev-libs/useflagpkg[-foo] (argument)".to_string(),]
    );
}

#[test]
fn autounmask_use_flip_is_not_blocked_by_a_default_satisfied_missing_flag() {
    // The `(+)` default already stands in for "enabled", so only the
    // genuinely-missing flag is suggested, applied, and recorded.
    let result = graph_result_autounmask("dev-libs/useflagpkg[missingflag,nonexistentflag(+)]");
    let entry = result
        .entries
        .iter()
        .find(|e| e.package == "useflagpkg")
        .expect("useflagpkg resolves via the missingflag flip");
    assert!(matches!(entry.outcome, PretendOutcome::New { .. }));
    assert_eq!(
        entry
            .use_flags_display
            .iter()
            .find(|(f, _)| f == "missingflag")
            .map(|(_, on)| *on),
        Some(true),
    );
    assert_eq!(result.autounmask_use_changes.len(), 1);
    assert_eq!(result.autounmask_use_changes[0].token, "missingflag");
}

#[test]
fn autounmask_use_dependency_records_the_two_line_dep_chain() {
    // `dev-libs/usedeprejectedpkg` RDEPENDs `dev-libs/useflagpkg[-foo]`.
    let result = graph_result_autounmask("dev-libs/usedeprejectedpkg");
    assert_eq!(result.autounmask_use_changes.len(), 1);
    assert_eq!(
        result.autounmask_use_changes[0].dep_chain,
        vec![
            "required by dev-libs/usedeprejectedpkg-1.0::testrepo".to_string(),
            "required by dev-libs/usedeprejectedpkg (argument)".to_string(),
        ]
    );
}

#[test]
fn autounmask_use_parent_flip_fails_like_portage_when_the_child_flag_is_masked() {
    // Masked child flag: Portage reports the bare miss with no rows (live probe,
    // rc 1); the parent keeps its own default.
    let result = graph_result_autounmask("dev-libs/parentflipeqpkg");
    assert!(
        matches!(result.outcome, ResolveOutcome::Aborted { .. }),
        "unexpected outcome: {:?}",
        result.outcome
    );
    let child = result
        .entries
        .iter()
        .find(|e| e.package == "parentflipchildpkg")
        .expect("child entry");
    assert!(
        matches!(child.outcome, PretendOutcome::NoVisibleCandidate),
        "{child:?}"
    );
    assert!(child.parent_use_suggestion.is_none());

    let parent = result
        .entries
        .iter()
        .find(|e| e.package == "parentflipeqpkg")
        .expect("parent entry");
    assert_eq!(
        parent
            .use_flags_display
            .iter()
            .find(|(f, _)| f == "feat")
            .map(|(_, on)| *on),
        Some(true),
        "no parent flip is recorded, so the parent keeps its own +feat default"
    );

    assert!(
        result.autounmask_use_changes.is_empty(),
        "no flip is recorded: {:?}",
        result.autounmask_use_changes
    );
    assert!(result.use_unsat_deps.is_empty());
    assert_eq!(result.plain_miss_deps.len(), 1);
}

#[test]
fn autounmask_use_parent_flip_records_minus_foo_and_fails_for_aup0b() {
    // Upstream parent-probe shape: only the parent-active conditional counts, so
    // `-foo` is recorded and the resolve still fails with the notice (live
    // probe, rc 1).
    let result = graph_result_autounmask("=dev-libs/aup0b-1");
    assert!(
        matches!(result.outcome, ResolveOutcome::Aborted { .. }),
        "unexpected outcome: {:?}",
        result.outcome
    );
    assert_eq!(result.autounmask_use_changes.len(), 1);
    let change = &result.autounmask_use_changes[0];
    assert_eq!(change.atom, ">=dev-libs/aup0b-1");
    assert_eq!(change.token, "-foo");
    assert_eq!(
        change.dep_chain,
        vec!["required by =dev-libs/aup0b-1 (argument)".to_string(),]
    );
    assert!(
        result.autounmask_no_clean_tail,
        "post-failure parent collect pre-empts the clean tail"
    );
    assert!(
        result.autounmask_backtrack_disabled(false),
        "the terminated-early notice prints"
    );
    let child = result
        .entries
        .iter()
        .find(|e| e.package == "aup0d")
        .expect("child entry");
    assert!(
        matches!(child.outcome, PretendOutcome::NoVisibleCandidate),
        "{child:?}"
    );
    assert!(child.parent_use_suggestion.is_some());
    assert!(result.use_unsat_deps.is_empty());
    assert!(result.plain_miss_deps.is_empty());
}

#[test]
fn autounmask_use_parent_flip_pfgraph_reports_the_bare_miss_like_portage() {
    // Masked child flag with a second conditional dep: Portage still reports the
    // bare miss under every flag combination (live probe, rc 1); nothing ever
    // re-resolves, and the parent keeps its own default.
    for result in [
        graph_result_autounmask("dev-libs/pfgraphparent"),
        graph_result_autounmask_backtrack("dev-libs/pfgraphparent"),
        graph_result_autounmask_use_n("dev-libs/pfgraphparent"),
    ] {
        assert!(
            matches!(result.outcome, ResolveOutcome::Aborted { .. }),
            "unexpected outcome: {:?}",
            result.outcome
        );
        let child = result
            .entries
            .iter()
            .find(|e| e.package == "pfgraphchild")
            .expect("child entry");
        assert!(
            matches!(child.outcome, PretendOutcome::NoVisibleCandidate),
            "{child:?}"
        );
        assert!(child.parent_use_suggestion.is_none());

        let parent = result
            .entries
            .iter()
            .find(|e| e.package == "pfgraphparent")
            .expect("parent entry");
        assert_eq!(
            parent
                .use_flags_display
                .iter()
                .find(|(f, _)| f == "pf")
                .map(|(_, on)| *on),
            Some(true),
            "no parent flip is recorded, so the parent keeps its own +pf default"
        );

        assert!(
            result.autounmask_use_changes.is_empty(),
            "no flip is recorded: {:?}",
            result.autounmask_use_changes
        );
        assert!(result.use_unsat_deps.is_empty());
        assert_eq!(result.plain_miss_deps.len(), 1);
        // Still walked but the aborted outcome prints no rows.
    }
}

#[test]
fn existing_slot_use_change_chain_starts_at_the_requester() {
    // The change chain starts at the requester whose atom needed the change, not
    // at the package that pulled it in first.
    let result = graph_result_autounmask("dev-libs/aubreaktop");
    let change = result
        .autounmask_use_changes
        .iter()
        .find(|c| c.token == "brk")
        .expect("the brk change is collected");
    assert_eq!(
        change.dep_chain,
        vec![
            "required by dev-libs/aubreakwant-1.0::testrepo".to_string(),
            "required by dev-libs/aubreaktop-1.0::testrepo".to_string(),
            "required by dev-libs/aubreaktop (argument)".to_string(),
        ]
    );
}

#[test]
fn autounmask_use_breakage_abandons_autounmask_and_re_resolves_clean() {
    // A flag wanted both ways: Portage drops every change and re-resolves clean,
    // so the contradiction surfaces as an ordinary `NoVisibleCandidate`. Only
    // detectable across backtrack re-drives.
    let result = graph_result_autounmask_backtrack("dev-libs/aubreaktop");
    assert!(
        result.autounmask_use_changes.is_empty(),
        "autounmask must be abandoned, not left with a contradictory change: {:?}",
        result.autounmask_use_changes
    );
    let sub = result
        .entries
        .iter()
        .find(|e| e.package == "aubreaksub" && matches!(e.outcome, PretendOutcome::New { .. }))
        .expect("aubreaksub still resolves as New");
    assert_eq!(
        sub.use_flags_display
            .iter()
            .find(|(f, _)| f == "brk")
            .map(|(_, on)| *on),
        Some(false),
        "the reverted graph keeps aubreaksub's default brk off"
    );
    assert!(
        result.entries.iter().any(|e| e.package == "aubreaksub"
            && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)),
        "aubreakwant's [brk] dep is unsatisfiable once autounmask is off"
    );
}

/// Keyword/license/mask changes keep `trigger: None` (Portage renders those
/// without the unsatisfied filter).
#[test]
fn non_use_changes_keep_no_trigger() {
    let keyword = graph_result_autounmask("dev-libs/autounmaskkeywordpkg");
    assert_eq!(keyword.autounmask_keyword_changes.len(), 1);
    assert_eq!(keyword.autounmask_keyword_changes[0].trigger, None);
    let license = graph_result_autounmask("dev-libs/licensemaskedconsumer");
    assert_eq!(license.autounmask_license_changes.len(), 1);
    assert_eq!(license.autounmask_license_changes[0].trigger, None);
    let mask = graph_result_autounmask("dev-libs/maskmaskedconsumer");
    assert_eq!(mask.autounmask_mask_changes.len(), 1);
    assert_eq!(mask.autounmask_mask_changes[0].trigger, None);
}

/// A flip-revealed `||` group defers to the drained graph (#398).
///
/// `pg398-t` pulls `pg398-a` plainly, then `pg398-c`'s `pg398-a[flip]` flips
/// the already-resolved slot on. The flip reveals
/// `|| ( dev-libs/pg398-x dev-libs/pg398-y )` while `pg398-b` (queued behind
/// the trigger) still has to pull `pg398-y` into the graph. Choosing at flip
/// time picks first-listed `pg398-x` with neither alternative visible; Portage
/// 3.0.82.2 (`emerge -pv --autounmask-use=y dev-libs/pg398-t`,
/// `LLM/evidence/2026-10-18-398/v3-t-portage.txt`) merges `y` but never `x`.
#[test]
fn flip_revealed_disjunction_defers_to_the_drained_graph() {
    for result in [
        graph_result_autounmask("dev-libs/pg398-t"),
        graph_result_autounmask_backtrack("dev-libs/pg398-t"),
    ] {
        assert!(
            result.entries.iter().all(|e| e.package != "pg398-x"),
            "the flip-revealed group must not pick x early: {:?}",
            result
                .entries
                .iter()
                .map(|e| &e.package)
                .collect::<Vec<_>>()
        );
        let a = result
            .entries
            .iter()
            .find(|e| e.package == "pg398-a")
            .expect("a is in the graph");
        assert!(
            a.deps.iter().any(|d| d.package == "pg398-y"),
            "the drain-time choice records the A->Y edge like Portage's tree: {:?}",
            a.deps.iter().map(|d| &d.package).collect::<Vec<_>>()
        );
        let mut y_owners: Vec<(String, String)> = result
            .entries
            .iter()
            .find(|e| e.package == "pg398-y")
            .expect("y is in the graph")
            .required_by
            .clone();
        y_owners.sort();
        assert_eq!(
            y_owners,
            vec![
                ("dev-libs".to_string(), "pg398-a".to_string()),
                ("dev-libs".to_string(), "pg398-b".to_string()),
            ],
            "the drain-time choice records both parents, like Portage walking A->Y and B->Y"
        );
        assert!(
            result.entries.iter().any(|e| e.package == "pg398-y"),
            "y still reaches the graph through pg398-b"
        );
        assert_eq!(
            result.autounmask_use_changes.len(),
            1,
            "the flip is still recorded: {:?}",
            result.autounmask_use_changes
        );
    }
}

#[test]
fn pg34_napp_pubgrub_records_the_use_change_gate() {
    // S9 #363: `--solver=pubgrub` must record Portage's `package.use`
    // change instead of silently applying it: `pg34-nlib` carries
    // default-off `f` while its parent needs `nlib[f]`, so the default
    // solver reports one USE change and stays `Complete` (the CLI fails
    // the run later in `_display_autounmask`, Portage's `need_config_change`
    // gate). Portage: depgraph.py need_config_change:11708.
    let default = graph_result_autounmask("dev-libs/pg34-napp");
    assert_eq!(
        default.autounmask_use_changes.len(),
        1,
        "the gate exists on the default path: {:?}",
        default.autounmask_use_changes
    );
    assert!(
        matches!(&default.outcome, ResolveOutcome::Complete),
        "a bare USE-change gate keeps Complete: {:?}",
        default.outcome
    );
    let pubgrub = graph_result_autounmask_pubgrub("dev-libs/pg34-napp");
    assert_eq!(
        pubgrub.autounmask_use_changes,
        default.autounmask_use_changes
    );
    assert_eq!(pubgrub.outcome, default.outcome);
    assert_eq!(
        pubgrub.autounmask_no_clean_tail,
        default.autounmask_no_clean_tail
    );
    assert_eq!(
        pubgrub.autounmask_backtrack_disabled(false),
        default.autounmask_backtrack_disabled(false)
    );
}
