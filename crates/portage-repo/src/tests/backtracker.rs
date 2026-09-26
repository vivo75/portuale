use super::*;

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
