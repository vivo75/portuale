use super::*;

#[test]
fn autounmask_suggests_a_keyword_only_when_enabled() {
    // dev-libs/autounmaskkeywordpkg's own KEYWORDS is "~amd64", not
    // accepted by the fixture profile's own ACCEPT_KEYWORDS, and it
    // has no package.accept_keywords entry of its own -- masked by
    // KEYWORDS alone (package.mask/license/properties/restrict all
    // pass), the exact "keyword_masked_only" shape --autounmask's
    // own v1 suggestion targets. With autounmask_suggest_keywords
    // off (the real, correct default -- see resolve_pretend_graph's
    // own doc comment for the full on/off default-resolution logic
    // this mirrors), no suggestion is appended, matching this
    // portuale's own pre-existing behavior exactly.
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
    // Keyword-masked-only -> real `_show_unsatisfied_dep`'s "All
    // ebuilds … have been masked" block (`~amd64 keyword`), not the
    // bare "there are no ebuilds to satisfy" line.
    assert_eq!(
        err_without_suggestion.to_string(),
        "\n!!! All ebuilds that could satisfy \"dev-libs/autounmaskkeywordpkg\" \
         have been masked.\n\
         !!! One of the following masked packages is required to complete your request:\n\
         - dev-libs/autounmaskkeywordpkg-1.0::testrepo (masked by: ~amd64 keyword)\n\
         \nFor more information, see the MASKED PACKAGES section in the emerge\n\
         man page or refer to the Gentoo Handbook.\n"
    );

    // With `--autounmask` explicit, the same keyword-masked-only
    // target now RESOLVES (real portage applies the implicit
    // `=cpv ~arch` change and the graph succeeds) rather than being
    // fatal -- it prints as a normal `New` entry and the change is
    // recorded for the `The following keyword changes are necessary
    // to proceed:` block.
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
    // dev-libs/useflagpkg's own "foo" flag is enabled globally by
    // the fixture profile's own make.conf (see use_dep_enforcement's
    // own tests above -- "-foo" is genuinely unsatisfiable against
    // it). "dev-libs/useflagpkg[-foo]" is masked by this one USE-dep
    // alone (package.mask/license/KEYWORDS all pass), the exact
    // "use_masked_only" shape --autounmask-use's own v1 suggestion
    // targets. With autounmask_suggest_use off (the real default),
    // no suggestion is appended; the abort is real's own USE block
    // (backlog #265(b)), not the bare miss.
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

    // With `--autounmask-use` (on by default), the same
    // `use_masked_only` atom now RESOLVES via the implicit
    // `package.use` flip rather than being fatal.
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
    // `dev-libs/usedeprejectedpkg` RDEPENDs `dev-libs/useflagpkg[-foo]`.
    // With `--autounmask-use` on the dependency RESOLVES (was
    // `NoVisibleCandidate` + a `use_suggestion` before autounmask-use
    // *resolution* shipped).
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
    // `dev-libs/licensemaskedconsumer` RDEPENDs the EULA-masked
    // `dev-libs/licensemaskedpkg` (LICENSE="SomeEula", no
    // package.license unmask). With `--autounmask-license` on the
    // dependency resolves and the missing license is recorded.
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

    // Off by default: `graph_result_real` (no autounmask) leaves the
    // dependency `NoVisibleCandidate` and records nothing.
    let plain = graph_result_real("dev-libs/licensemaskedconsumer");
    assert!(plain.autounmask_license_changes.is_empty());
    assert!(plain.entries.iter().any(|e| e.package == "licensemaskedpkg"
        && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)));
}

#[test]
fn autounmask_keep_masks_n_unmasks_a_package_mask() {
    // `dev-libs/maskmaskedconsumer` RDEPENDs the package.mask'd
    // `dev-libs/hardmaskedpkg`. `graph_result_autounmask` has
    // `autounmask_suggest_masks` on, so the dependency resolves and
    // the `=<cpv>` mask change is recorded (empty token).
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

    // Off by default: no autounmask -> dep stays NoVisibleCandidate.
    let plain = graph_result_real("dev-libs/maskmaskedconsumer");
    assert!(plain.autounmask_mask_changes.is_empty());
    assert!(
        plain.entries.iter().any(|e| e.package == "hardmaskedpkg"
            && matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    );
}

#[test]
fn abort_outcome_membership_and_order() {
    // Slice 3 pins (docs/history/abort-path-spec.md §6, live captures in
    // fixtures/abort-captures/): the outcome is produced on the
    // settled graph while `entries` stays the full graph (the
    // renderer switches to `partial` in Slice 4).
    //
    // Masked-only dep (§4a): empty partial, the report's display
    // atom, the top as the first merge-bound requirer -- and the
    // `-last` sibling identical (declared position is unobservable).
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
    // Unserializable cycle (§4c): the `_serialize_tasks` stuck
    // remainder -- top + both arms, both leaves drained out -- in
    // leaf-drain order, `partial` one entry per member in that order.
    // The autounmask+cycle fixtures (§4d) are the same shape: the
    // autounmask changes only stop real's backtracking at try 0.
    // (The `abort-au-*` tops resolve with `--autounmask-use` on, the
    // CLI default; `graph_result_real` runs with autounmask off.)
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
    // With `--autounmask-use=n` the `[auflag]` dep is unsatisfiable
    // and wins over the cycle (spec §4e third bullet: real prints
    // the "no ebuilds built with USE flags" block, no list).
    assert!(matches!(
        graph_result_autounmask_use_n("dev-libs/abort-au-cycle").outcome,
        ResolveOutcome::Aborted {
            reason: AbortReason::UnsatisfiedAtom { .. },
            ..
        }
    ));
    // Precedence: a walk-time failure beats the serialize-time one
    // in the same graph (oracle `abort-masked-cycle`: no list, no
    // circular block) -- even though the cycle is still detected.
    let result = graph_result_real("dev-libs/abort-masked-cycle");
    assert!(!result.circular_deps.is_empty());
    assert!(matches!(
        result.outcome,
        ResolveOutcome::Aborted {
            reason: AbortReason::MaskedDep { .. },
            ..
        }
    ));
    // Autounmask changes alone never abort (real `_success_without_
    // autounmask` shows the full list); neither do ordinary resolves.
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
    // Backlog #217, both branches of the "terminated early" gate
    // (`GraphResult::autounmask_backtrack_disabled`, real
    // `_autounmask_backtrack_disabled`, `depgraph.py:11752`).
    // Fixture ground truth is live real, captured in
    // `fixtures/abort-captures/`: `abort-au-plain`'s USE block has
    // NO notice (lone change -- `_success_without_autounmask`),
    // `abort-au-cycle`'s and `aucasctop`'s blocks DO (a cycle /
    // a restart-worthy flip on the already-graphed `aucascmid`
    // coincide).
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
    // The cycle shape keeps the notice via the outcome arm, and the
    // restart-wanted shape via the accumulator arm.
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
    // Post-failure parent collects keep the notice too (real
    // `_apply_parent_use_changes`, `depgraph.py:5820`): the aup0b
    // cell records `-foo` on the failed dep and fails with the
    // notice.
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
    // `dev-libs/parentflipeqpkg`: the masked child means no flip is
    // recorded at all (real's untouchable-child `continue`,
    // `depgraph.py:6732-6736`), so there is no change block and no
    // notice -- the bare miss (round-2 correction of the old
    // resolve+rows expectation, which was never live-probed).
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
    // `dev-libs/pfgraphparent`: same masked-child shape as
    // parentflipeqpkg (`use.mask pf`), so the same untouchable-child
    // `continue` (`depgraph.py:6732-6736`) applies -- live real
    // (`localhost/test-portuale:latest`, staged fixtures, g195c
    // probe `LLM/evidence/2026-09-28-g195/g195c-real-probe.txt`)
    // reports the bare miss under default AND `--autounmask-backtrack=y`:
    // `emerge: there are no ebuilds to satisfy
    // "dev-libs/pfgraphchild[pf=]"`, no rows, no block, rc 1.
    // (Corrected under coordinator ruling B20, option (a); the old
    // flip+notice assertions below were a source-reading inference,
    // never a live measurement.)
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
    // The config conjuncts suppress unconditionally, even where the
    // notice would otherwise print: `--autounmask-backtrack=y`
    // opts back in, and `--backtrack=0` leaves real's
    // `_allow_backtracking` False so the flag is never set.
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
    // Backlog #218: real's DFS applies `aucasclate`'s `[cascade]`
    // flip to the still-unwalked `aucascmid` before walking its deps
    // (`_add_dep` only pushes onto `_dep_stack`,
    // `lib/_emerge/depgraph.py:3254-3271`; the flip itself is
    // `_pkg_use_enabled(pkg, target_use)`, `:7669`), so
    // `aucascleaf` joins the graph in the same attempt
    // (live-captured in `fixtures/abort-captures/
    // dev-libs_aucasctop.pretend-debug.stdout`, one continuous walk).
    // Portuale's already-resolved-slot re-check re-expands the
    // flipped package's newly-gated deps in-walk
    // (`expand_resolved_slot_with_flipped_use`), so the default
    // (no-backtrack) graph carries the full 4-package list.
    let result = graph_result_autounmask("dev-libs/aucasctop");
    let mut pkgs: Vec<&str> = result.entries.iter().map(|e| e.package.as_str()).collect();
    pkgs.sort_unstable();
    assert_eq!(
        pkgs,
        vec!["aucasclate", "aucascleaf", "aucascmid", "aucasctop"],
        "the in-graph flip lands before the flipped node's walk"
    );
    // The flipped package carries a real merge-order edge to the
    // leaf (not a stitched-on entry): its deps were re-flattened
    // under the flipped USE.
    let mid = result
        .entries
        .iter()
        .find(|e| e.package == "aucascmid")
        .expect("aucascmid is in the graph");
    assert!(
        mid.deps.iter().any(|d| d.package == "aucascleaf"),
        "aucascmid's re-expanded deps name aucascleaf"
    );
    // The change block still records the flip (the #217 notice gate
    // reads the same accumulator).
    assert!(
        !result.autounmask_use_changes.is_empty(),
        "the [cascade] flip is still reported"
    );
    // L0 20260928T063310Z: the delta never records a phantom owner.
    // `aucascleaf` queues only through the flipped package's
    // re-expansion (a genuinely new slot), and no text-new delta atom
    // re-names a settled slot here -- so every `required_by` set is
    // exactly the forward-walk owners, with no flipped-package
    // residue for `build_digraph`'s fallback to stitch a phantom
    // edge from (real carries no such edge: the flip lands via the
    // later atom, never via a re-walk of the flipped package).
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
    // The fresh-flip shape is untouched: `aucasclate` alone resolves
    // `aucascmid[cascade]` on first encounter, leaf included, with no
    // delta pass involved.
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
    // `--autounmask-backtrack=y` (whole-walk re-drive) agrees on the
    // graph -- the delta only anticipates what the re-drive finds.
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
    // Both the consumer and the (now un-keyword-masked) dependency
    // resolve as New.
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
    // `dev-libs/kwbacktop` RDEPENDs `dev-libs/kwbackmid` (bare -> stable
    // `1.0` wins the slot first) AND `>=dev-libs/kwbackmid-2.0` (only
    // `2.0` exists there, and it is `~amd64`). Slice 6: the slot
    // conflict folds both atoms into `slot_constraints`, and on the
    // retry `resolve_pretend`'s `*_masked_only` fallback fires because
    // no *is_visible* candidate satisfies the folded `>=2.0` -- so
    // `kwbackmid-2.0`'s keyword is autounmasked and the slot settles on
    // `2.0`. Real `_select_pkg` weighing every atom pulling the slot +
    // `_autounmask_levels`.
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
    // `dev-libs/multimaskdep-2.0` is `~amd64` keyword-masked AND
    // `@EULA` license-masked; `multimaskdep-1.0` is only keyword-masked.
    // Real `_autounmask_levels`: level 1 (+license) yields nothing (both
    // still keyword-blocked); level 2 (+~arch +license) unmasks BOTH
    // categories, and re-scanning every version picks the higher `2.0`.
    // Before the per-level re-scan, portuale's flat `keyword_masked_only`
    // fallback excluded `2.0` (it has a license problem too) and settled
    // on `1.0` with only a keyword change.
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
    // Backlog #248: the change sits on the argument package itself,
    // and real `_get_dep_chain` never prints the start node -- just
    // the `(argument)` line (fresh host 3.0.82.2 staged-fixture
    // probe 2026-09-29).
    let result = graph_result_autounmask("dev-libs/autounmaskkeywordpkg");
    assert_eq!(result.autounmask_keyword_changes.len(), 1);
    assert_eq!(
        result.autounmask_keyword_changes[0].dep_chain,
        vec!["required by dev-libs/autounmaskkeywordpkg (argument)".to_string(),]
    );
}

#[test]
fn autounmask_use_resolves_a_use_dep_mismatch_and_applies_the_flip_to_the_display() {
    // `dev-libs/useflagpkg[-foo]` -- `foo` is default-on, the atom
    // wants it off. `--autounmask-use` (on in `graph_result_
    // autounmask`) applies the flip so the entry resolves as `New`,
    // the `-pv` USE display shows `foo` disabled, and the change is
    // recorded with the `>=<cpv>` atom form (real `check_if_latest`
    // for USE, bug #536392). Backlog #248: the dep chain is just the
    // `(argument)` line -- real `_get_dep_chain` never prints the
    // start node (fresh host 3.0.82.2 staged-fixture probe
    // 2026-09-29).
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
    // `dev-libs/useflagpkg[missingflag,nonexistentflag(+)]` -- the atom
    // needs `missingflag` on (it's in IUSE, default-off: a real flip)
    // AND `nonexistentflag(+)` (not in IUSE, its `(+)` default already
    // stands in for "enabled" -- nothing to flip). `suggested_use_flip`
    // must skip the default-satisfied missing flag rather than bail on
    // the whole atom, so the `missingflag` flip is still suggested,
    // applied, and recorded.
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
    // `dev-libs/parentflipeqpkg` (IUSE +feat) RDEPENDs
    // `dev-libs/parentflipchildpkg[feat=]`; the child's own `feat` is
    // `use.mask`'d. Live real (`localhost/test-portuale:latest`,
    // staged fixtures, `emerge --pretend dev-libs/parentflipeqpkg`,
    // rc 1) reports the BARE miss -- no rows, no USE block:
    // `emerge: there are no ebuilds to satisfy
    // "dev-libs/parentflipchildpkg[feat=]"`. Real's untouchable-child
    // `continue` (`lib/_emerge/depgraph.py:6732-6736`) skips the
    // parent probe when the child's needed flag is masked/forced, so
    // no parent flip is ever recorded. (The old resolve+rows+block
    // expectation was verified against the since-removed second
    // Python copy, never live real.)
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
    // Upstream `test_autounmask_parent.py` (`aup0b`): `dev-libs/aup0b`
    // (IUSE `+bar +foo`) DEPENDs `dev-libs/aup0d[foo(-)?,bar(-)?]`;
    // the fixture profile leaves the parent at `{foo}`, so only the
    // `foo(-)?` conditional is parent-active. Live real
    // (`localhost/test-portuale:latest`, staged fixtures,
    // `emerge --pretend --autounmask =dev-libs/aup0b-1`, rc 1)
    // records `-foo` on the parent and FAILS: stdout is the merge
    // header with no rows, stderr is the `-foo` USE block plus the
    // "backtracking has terminated early" notice. Real
    // `_apply_parent_use_changes` (`lib/_emerge/depgraph.py:5820`)
    // re-probes with `violated_conditionals` (`:6779-6798`), which
    // sees only the parent-active `foo`, and the
    // `_success_without_autounmask` tail (`:5793`) returns False.
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
    // `dev-libs/pfgraphparent` (IUSE +pf) RDEPENDs
    // `dev-libs/pfgraphchild[pf=]` AND `pf? ( dev-libs/pfgraphextra )`.
    // The child's `pf` is `use.mask`'d. Live real
    // (`localhost/test-portuale:latest`, staged fixtures,
    // `emerge --pretend dev-libs/pfgraphparent`, rc 1) reports the
    // BARE miss -- no rows, no USE block:
    //
    //     These are the packages that would be merged, in order:
    //
    //     Calculating dependencies  ... done!
    //     Dependency resolution took 0.35 s (backtrack: 0/20).
    //
    //
    //     emerge: there are no ebuilds to satisfy "dev-libs/pfgraphchild[pf=]" for /tmp/fxstage/fixtures/.
    //     (dependency required by "dev-libs/pfgraphparent-1.0::testrepo" [ebuild])
    //     (dependency required by "dev-libs/pfgraphparent" [argument])
    //     rc=1
    //
    // (identical text with `--autounmask-backtrack=y` and with
    // `--autounmask-use=n`; Global-Updates/news/FEATURES noise cut;
    // portuale omits real's staging `for <root>.` suffix, the
    // long-standing `fixture-miss-message-unsuffixed` class.)
    // Real's untouchable-child `continue`
    // (`lib/_emerge/depgraph.py:6732-6736`) skips the parent probe
    // when the child's needed flag is masked/forced, so no parent
    // flip is ever recorded and there is nothing to re-resolve --
    // neither the default single-dep re-resolve (which kept
    // `pf? ( pfgraphextra )` in the list) nor the
    // `--autounmask-backtrack=y` whole-graph re-resolve (which
    // dropped it) ever happens. (The old resolve+rows+block
    // expectation was a source-reading inference, never a live
    // measurement. Renamed from
    // `autounmask_use_parent_flip_re_resolves_the_whole_graph`,
    // which stated the opposite of what real does -- coordinator
    // ruling B20, option (a).)
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
        // `pfgraphextra` is still walked (`pf? (...)` stays active
        // with the parent's +pf unflipped) but the `Aborted` outcome
        // prints no rows -- real's merge list is likewise empty.
    }
}

#[test]
fn existing_slot_use_change_chain_starts_at_the_requester() {
    // Backlog #244: `aubreaktop` pulls `aubreaksub` plain first, then
    // `aubreakwant` needs `aubreaksub[brk]` on the already-resolved
    // package. Real's change chain starts at the requester whose atom
    // needed the change (`aubreakwant`), not at `aubreaktop`, which
    // pulled the package in first (real 3.0.82.2,
    // LLM/evidence/2026-09-28-244/fixture-oracle-2/real/dev-libs_aubreaktop.txt).
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
    // `dev-libs/aubreaktop` RDEPENDs `aubreaksub` (plain, resolves
    // `brk` off first), then `aubreakwant` (needs `aubreaksub[brk]`)
    // and `aubreakunwant` (needs `aubreaksub[-brk]`). Autounmask folds
    // `brk` on for `aubreakwant`; the next pass's `aubreakunwant[-brk]`
    // asks for it back off -- a flag wanted both ways. Real
    // `_autounmask_breakage` (depgraph.py:12262): drop every autounmask
    // change and re-resolve one final pass with suggestion off. So
    // nothing is recorded, `aubreaksub` settles at its default `brk`
    // off, and `aubreakwant`'s now-unsatisfiable `[brk]` shows up as an
    // ordinary (non-fatal) dependency `NoVisibleCandidate` -- exactly
    // what `--autounmask-use=n` would have produced from the start.
    // Needs `--autounmask-backtrack=y`: the contradiction is only
    // detected across the loop's re-drives (the default collect-only
    // path never gets a second pass to notice the flag wanted both
    // ways).
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

/// Backlog #205 R4, negative legs: keyword/license/mask changes keep
/// `trigger: None` (real renders those without the unsatisfied
/// filter), on the same fixture shapes their own tests pin.
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
