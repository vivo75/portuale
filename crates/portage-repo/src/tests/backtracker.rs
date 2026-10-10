use super::*;

#[test]
fn fixture_backtrack_max_zero_disables_slot_conflict_reconciliation() {
    // Same solvable fixture, but `backtrack_max == 0` (real
    // `--backtrack=0`): Backlog #90 (S1) -- no conflict forms at all
    // (the old consumer graphs 1.0 first, the bare dep reuses it),
    // so bt0 settles silently on 1.0 with rc 0, exactly real's S0
    // matrix row. The pre-S1 expectation (one recorded conflict) is
    // retired: that conflict was the forward-order phantom.
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
    // A single retry is enough to reconcile this one-step conflict.
    let one = graph_result_real_backtrack("dev-libs/slotconflictparent", 1);
    assert_eq!(one.slot_conflicts, vec![]);
}

#[test]
fn check_runtime_pkg_mask_discards_fully_masked_parent_sets() {
    // Real `Backtracker._check_runtime_pkg_mask` (bug 375573),
    // C3 oracle for the discard predicate itself (no fixture can
    // reach it end to end yet -- needs a 3-pass parent-masking
    // chain, see `docs/history/023-oracle.md`).
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
    // A mask whose every parent is itself masked is discarded.
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
    // `c-1.0`'s parent `d` is unmasked, but `a`'s parent `b-2.0` is
    // still fully masked -- still discarded.
    assert!(!check_runtime_pkg_mask(&masks));
    // Unmask `b-2.0` by dropping its entry: `a`'s parent is free.
    masks.insert(("dev-libs".to_string(), "b".to_string()), vec![]);
    assert!(check_runtime_pkg_mask(&masks));
    // Masks with no parents (real's 692746 arm) never disqualify,
    // and missing-dependency entries are skipped entirely.
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

/// #249: real `dep_zapdeps` chains `circular_dependency.get(parent)`
/// and `.get(virt_parent)` (`portage/dep/dep_check.py:673-678`); a
/// virtual's `||` is expanded inline under the package that pulled it,
/// so the puller's circular record demotes the cycle-closing branch.
/// The pmtest fixture `dev-util/u249make` (bug 703440's cycle with the
/// `||` inside `virtual/u249make`): real's ResolverPlayground merges the
/// bootstrap after one circular restart and aborts under
/// `--backtrack=0` (portuale `LLM/evidence/2026-09-30-249/u249.txt`).
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

/// Backlog #221: circular `||` backtracking (upstream
/// `test_circular_choices.py`, bugs 703440 and 705986). Each cell's
/// first pass takes the in-graph `||` branch and strands the
/// merge-order walk; the restart demotes the cycle-closing branch
/// (`circular_atom`) and merges real's exact set in real's exact
/// order. `--backtrack=0` keeps real's circular failure everywhere
/// (probed against 3rdparty/portage 3.0.82.2's `ResolverPlayground`,
/// which fails all three cells with `{"--backtrack": 0}`).
#[test]
fn circular_choices_backtrack_to_the_cycle_breaking_branch() {
    let names_of = |result: &GraphResult| {
        result
            .entries
            .iter()
            .map(|e| e.package.clone())
            .collect::<Vec<_>>()
    };
    // pg1 (bug 703440): `[bootstrap, jsoncpp, cmake]`.
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
    // pg4 (bug 705986): `[exe-bin, pypy]`, no `+low-memory` rescue.
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
    // pg4 requesting the exe itself: real merges it after its own
    // buildtime dep (`[exe-bin, pypy, exe]`).
    let result = graph_result_real("dev-libs/ccd4b");
    assert!(matches!(result.outcome, ResolveOutcome::Complete));
    assert_eq!(names_of(&result), vec!["ccd4c", "ccd4a", "ccd4b"]);
    // pg5 (`testDirectVirtualCircularDependency`): the
    // runtime-closing ring still backtracks to `-bin` first.
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

/// g221c (L0 `dev-lang/rust` regression): a slot-pinned `||` build
/// dep over an older slot of the package itself vs the `-bin`
/// package, the older slot carrying its own self buildtime edge.
/// The first pass self-picks and strands; the retry demotes the
/// recorded self branch and must take `-bin` -- real's exact set in
/// real's exact order after exactly one retry (grounded in
/// 3rdparty/portage 3.0.82.2 `ResolverPlayground`,
/// `/tmp/opencode/g221c/probe_slcirc.py`: backtracking on merges
/// `[slcirc-bin-2.0, slcirc-2.0]` after `backtracking try 1`,
/// `--backtrack=0` fails circular). The g221b reconciliation broke
/// this: the slot-blind in-graph check ranked the older-slot branch
/// `Installed`, beating `-bin` and re-closing the cycle through the
/// older slot's self edge (rc 1, `slcirc-1.0 depends on slcirc-1.0`).
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

/// Backlog #221 (I3): the unsolved-cycle-with-backtracking-ON
/// leg of `assemble_result`'s persisting-ring report. `dev-libs/sbrA`
/// BDEPENDs+RDEPENDs on `dev-libs/sbrB` (the dual edge softens the
/// hard reporter's arm to `(true, true)`) while `sbrB` BDEPENDs back,
/// so the hard reporter stays empty but the walk strands: the retry
/// re-strands identically (no `||` branch to switch to -- the
/// demotion has nothing to demote), the ring is unsolved, and the
/// settled pass reports it through the persisting-ring leg instead
/// of settling `Complete`. Grounded in real runs
/// (`/tmp/opencode/g221b/probe_sbr.py`, 3rdparty portage 3.0.82.2
/// `ResolverPlayground`, debug `backtracking try` count): backtracking
/// on fails after exactly 1 retry, `--backtrack=0` fails with none.
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
    // `--backtrack=0` fails the same way (real never retries there).
    assert!(
        !graph_result_real_backtrack("dev-libs/sbrA", 0)
            .circular_deps
            .is_empty()
    );
}

/// Backlog #161 S2: `params_equal` is real's parameter-only `__eq__`
/// (every accumulator agrees; `backtrack_config`, `depth` and
/// `mask_steps` are search position, not state). Each conjunct is
/// load-bearing: one differing field must make two params unequal,
/// while the excluded position fields never do.
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
    // Backlog #129 (S1): first-seen mask order is render order,
    // not search state -- same masks in any arrival order dedup.
    let mut p = base.clone();
    p.mask_order
        .push((("a".to_string(), "b".to_string()), "!=a/b-1.0".to_string()));
    assert!(params_equal(&base, &p), "mask_order excluded");
}

/// Backlog #129 (S1): the missed-update derivation only runs on a
/// clean settle. At `--backtrack=0` the mg2top root pass keeps its
/// slot conflicts (highest-first picks the `-2`s), so no mask
/// trial state exists and neither notice may fire.
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
