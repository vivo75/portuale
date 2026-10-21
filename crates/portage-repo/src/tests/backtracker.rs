use super::*;

#[test]
fn fixture_backtrack_max_zero_disables_slot_conflict_reconciliation() {
    // `backtrack_max == 0` (Portage `--backtrack=0`): no conflict forms, so bt0
    // settles silently on 1.0 with rc 0.
    let result = graph_result_real_backtrack("dev-libs/slotconflictparent", 0);
    assert_eq!(result.slot_conflicts, vec![]);
    let target = result
        .entries
        .iter()
        .find(|e| e.package == "slotconflicttarget")
        .expect("slotconflicttarget entry");
    assert_eq!(
        target.outcome,
        PretendOutcome::New {
            version: "1.0".to_string()
        }
    );
    // A single retry reconciles this one-step conflict.
    let one = graph_result_real_backtrack("dev-libs/slotconflictparent", 1);
    assert_eq!(one.slot_conflicts, vec![]);
}

#[test]
fn check_runtime_pkg_mask_discards_fully_masked_parent_sets() {
    // Portage `Backtracker._check_runtime_pkg_mask` (bug 375573): oracle for the
    // discard predicate itself.
    let entry = |neg: &str, reason: MaskReason| MaskEntry {
        neg: neg.to_string(),
        reason,
    };
    let slot =
        |parents: Vec<((String, String, String), String)>| MaskReason::SlotConflict { parents };
    let parent = |cp: (&str, &str, &str), atom: &str| {
        (
            (cp.0.to_string(), cp.1.to_string(), cp.2.to_string()),
            atom.to_string(),
        )
    };
    // Every parent masked: discarded.
    let mut masks: HashMap<(String, String), Vec<MaskEntry>> = HashMap::new();
    masks.insert(
        ("dev-libs".to_string(), "a".to_string()),
        vec![entry(
            "!=dev-libs/a-2.0",
            slot(vec![parent(("dev-libs", "b", "2.0"), ">=dev-libs/b-2.0")]),
        )],
    );
    masks.insert(
        ("dev-libs".to_string(), "b".to_string()),
        vec![entry(
            "!=dev-libs/b-2.0",
            slot(vec![parent(("dev-libs", "c", "1.0"), "dev-libs/c")]),
        )],
    );
    assert!(!check_runtime_pkg_mask(&masks));
    // One unmasked parent anywhere keeps the node.
    masks.insert(
        ("dev-libs".to_string(), "c".to_string()),
        vec![entry(
            "!=dev-libs/c-1.0",
            slot(vec![parent(("dev-libs", "d", "1.0"), "dev-libs/d")]),
        )],
    );
    // `c-1.0`'s parent `d` is free but `b-2.0` is still fully masked.
    assert!(!check_runtime_pkg_mask(&masks));
    // Unmask `b-2.0`: `a`'s parent is free.
    masks.insert(("dev-libs".to_string(), "b".to_string()), vec![]);
    assert!(check_runtime_pkg_mask(&masks));
    // Parentless masks never disqualify (Portage's 692746 arm); missing-dependency
    // entries are skipped entirely.
    let mut masks2: HashMap<(String, String), Vec<MaskEntry>> = HashMap::new();
    masks2.insert(
        ("dev-libs".to_string(), "e".to_string()),
        vec![
            entry("!=dev-libs/e-2.0", slot(vec![])),
            MaskEntry {
                neg: "!=dev-libs/e-1.0".to_string(),
                reason: MaskReason::MissingDependency {
                    parent: ("dev-libs".to_string(), "f".to_string()),
                    atom: "dev-libs/f".to_string(),
                },
            },
        ],
    );
    assert!(check_runtime_pkg_mask(&masks2));
}

/// A virtual's `||` demotes through its puller's circular record: the merge order
/// below holds, and `--backtrack=0` keeps the cycle.
#[test]
fn virtual_or_choice_is_demoted_through_its_pullers_circular_record() {
    let ok = graph_result_real("dev-util/u249make");
    let cpvs: Vec<String> = ok
        .entries
        .iter()
        .filter_map(merge_bound_cpv)
        .filter(|c| c.contains("u249"))
        .collect();
    assert_eq!(
        cpvs,
        vec![
            "dev-util/u249make-bootstrap-1".to_string(),
            "virtual/u249make-0".to_string(),
            "dev-libs/u249json-1".to_string(),
            "dev-util/u249make-1".to_string(),
        ]
    );
    let bt0 = graph_result_real_backtrack("dev-util/u249make", 0);
    assert!(
        matches!(
            bt0.outcome,
            ResolveOutcome::Aborted {
                reason: AbortReason::UnserializableCycle { .. },
                ..
            }
        ),
        "--backtrack=0 keeps the cycle"
    );
}

/// Circular `||` backtracking: the restart demotes the cycle-closing branch and
/// merges Portage's set in Portage's order; `--backtrack=0` keeps the failure.
#[test]
fn circular_choices_backtrack_to_the_cycle_breaking_branch() {
    let names_of = |result: &GraphResult| {
        result
            .entries
            .iter()
            .map(|e| e.package.clone())
            .collect::<Vec<_>>()
    };
    // pg1 (bug 703440).
    let result = graph_result_real("dev-libs/ccd1c");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
    assert_eq!(names_of(&result), vec!["ccd1b", "ccd1a", "ccd1c"]);
    assert!(
        !graph_result_real_backtrack("dev-libs/ccd1c", 0)
            .circular_deps
            .is_empty()
    );
    // pg4 (bug 705986).
    let result = graph_result_real("dev-libs/ccd4a");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
    assert_eq!(names_of(&result), vec!["ccd4c", "ccd4a"]);
    assert!(result.autounmask_use_changes.is_empty());
    assert!(
        !graph_result_real_backtrack("dev-libs/ccd4a", 0)
            .circular_deps
            .is_empty()
    );
    // pg4 requesting the exe itself.
    let result = graph_result_real("dev-libs/ccd4b");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert_eq!(names_of(&result), vec!["ccd4c", "ccd4a", "ccd4b"]);
    // pg5: the runtime-closing ring still backtracks to `-bin` first.
    let result = graph_result_real("dev-libs/ccd5a");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
    assert_eq!(names_of(&result), vec!["ccd5b", "ccd5v", "ccd5a"]);
    assert!(
        !graph_result_real_backtrack("dev-libs/ccd5a", 0)
            .circular_deps
            .is_empty()
    );
}

/// Slot-pinned `||` over the package's own older slot vs `-bin`: the retry must
/// take `-bin` after exactly one retry.
#[test]
fn circular_slot_pinned_branch_prefers_bin_over_older_slot() {
    let result = graph_result_real("dev-libs/slcirc");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert!(
        result.circular_deps.is_empty(),
        "{:?}",
        result.circular_deps
    );
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| format!(
                "{}-{}",
                e.package,
                match &e.outcome {
                    PretendOutcome::New { version }
                    | PretendOutcome::AlreadyInstalled { version } => version.clone(),
                    PretendOutcome::Reinstall { version, .. } => version.clone(),
                    PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } =>
                        to.clone(),
                    PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } =>
                        "?".to_string(),
                }
            ))
            .collect::<Vec<_>>(),
        vec!["slcirc-bin-2.0", "slcirc-2.0"],
    );
    assert_eq!(result.backtrack_restarts, 1);
    assert!(
        !graph_result_real_backtrack("dev-libs/slcirc", 0)
            .circular_deps
            .is_empty()
    );
}

/// Unsolved ring with backtracking on: the retry re-strands identically (nothing
/// to demote), and the settled pass reports the persisting ring.
#[test]
fn softened_ring_reports_the_persisting_cycle_with_backtracking_on() {
    let result = graph_result_real("dev-libs/sbrA");
    assert!(
        !matches!(result.outcome, ResolveOutcome::Complete),
        "softened ring must stay rc 1 with backtracking on"
    );
    assert_eq!(
        result.circular_deps,
        vec![vec![
            "dev-libs/sbrA-1.0".to_string(),
            "dev-libs/sbrB-1.0".to_string()
        ]],
        "{:?}",
        result.circular_deps
    );
    // `--backtrack=0` fails the same way.
    assert!(
        !graph_result_real_backtrack("dev-libs/sbrA", 0)
            .circular_deps
            .is_empty()
    );
}

/// `params_equal` compares accumulator state only; search position never compares
/// unequal, and every conjunct is load-bearing.
#[test]
fn backtrack_params_equal_compares_every_accumulator() {
    let base = BacktrackParams::default();
    assert!(params_equal(&base, &base));
    let differs = |p: BacktrackParams| !params_equal(&base, &p);

    let mut p = base.clone();
    p.autounmask_suggest_keywords = true;
    assert!(differs(p), "suggest_keywords");
    let mut p = base.clone();
    p.autounmask_suggest_use = true;
    assert!(differs(p), "suggest_use");
    let mut p = base.clone();
    p.autounmask_suggest_license = true;
    assert!(differs(p), "suggest_license");
    let mut p = base.clone();
    p.autounmask_suggest_masks = true;
    assert!(differs(p), "suggest_masks");
    let mut p = base.clone();
    p.missing_dep_masked.insert("!=a/b-1.0".to_string());
    assert!(differs(p), "missing_dep_masked");
    let mut p = base.clone();
    p.reverse_dep_masked
        .insert((("a".to_string(), "b".to_string()), "atom".to_string()));
    assert!(differs(p), "reverse_dep_masked");
    let pin = RevDepPin {
        cp: ("a".to_string(), "b".to_string()),
        atom: "=a/b-1.0".to_string(),
        raw_atom: "=a/b-1.0".to_string(),
        consumer: ("a".to_string(), "b".to_string(), "1.0".to_string()),
    };
    let mut p = base.clone();
    p.dropped_pins.push(pin.clone());
    assert!(differs(p), "dropped_pins");
    let mut p = base.clone();
    p.reverse_dep_pins.push(pin.clone());
    assert!(differs(p), "reverse_dep_pins");
    let mut p = base.clone();
    p.slot_constraints
        .insert(("a".to_string(), "b".to_string()), vec!["atom".to_string()]);
    assert!(differs(p), "slot_constraints");
    let mut p = base.clone();
    p.runtime_pkg_mask.insert(
        ("a".to_string(), "b".to_string()),
        vec![MaskEntry {
            neg: "!=a/b-1.0".to_string(),
            reason: MaskReason::MissingDependency {
                parent: ("a".to_string(), "b".to_string()),
                atom: "x".to_string(),
            },
        }],
    );
    assert!(differs(p), "runtime_pkg_mask");
    let mut p = base.clone();
    p.autounmask_use_config.insert(
        ("a".to_string(), "b".to_string()),
        HashMap::from([("flag".to_string(), true)]),
    );
    assert!(differs(p), "autounmask_use_config");
    let mut p = base.clone();
    p.autounmask_use_change_records.push(AutounmaskChange {
        atom: ">=a/b-1.0".to_string(),
        token: "flag".to_string(),
        dep_chain: Vec::new(),
        trigger: None,
        mask_entry: None,
    });
    assert!(differs(p), "autounmask_use_change_records");
    let mut p = base.clone();
    p.autounmask_use_broke = true;
    assert!(differs(p), "autounmask_use_broke");
    let mut p = base.clone();
    p.autounmask_disabled = true;
    assert!(differs(p), "autounmask_disabled");
    let mut p = base.clone();
    p.slot_operator_replace_installed
        .insert(("a".to_string(), "b".to_string()));
    assert!(differs(p), "slot_operator_replace_installed");
    let mut p = base.clone();
    p.slot_operator_undone
        .insert(("a".to_string(), "b".to_string()));
    assert!(differs(p), "slot_operator_undone");

    // Search position is not state: these never compare unequal.
    let mut p = base.clone();
    p.backtrack_config = Some(test_config());
    assert!(params_equal(&base, &p), "backtrack_config excluded");
    let mut p = base.clone();
    p.depth = 5;
    assert!(params_equal(&base, &p), "depth excluded");
    let mut p = base.clone();
    p.mask_steps = 3;
    assert!(params_equal(&base, &p), "mask_steps excluded");
    // First-seen mask order is render order, not state.
    let mut p = base.clone();
    p.mask_order
        .push((("a".to_string(), "b".to_string()), "!=a/b-1.0".to_string()));
    assert!(params_equal(&base, &p), "mask_order excluded");
}

/// The missed-update derivation runs only on a clean settle.
#[test]
fn backtrack_missed_updates_stays_silent_without_a_clean_settle() {
    let result = graph_result_real_backtrack("dev-libs/mg2top", 0);
    assert!(
        !result.slot_conflicts.is_empty(),
        "bt0 keeps the conflicts instead of settling"
    );
    assert!(
        result.skipped_updates.is_empty(),
        "no WARNING rows without trial state: {:?}",
        result.skipped_updates
    );
    assert!(
        result.skipped_missing_deps.is_empty(),
        "no abbreviated slots without trial state: {:?}",
        result.skipped_missing_deps
    );
}
