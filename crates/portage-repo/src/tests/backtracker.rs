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
