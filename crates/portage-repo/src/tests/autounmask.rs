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
