use super::*;

#[test]
fn slot_conflict_reason_and_caret_span() {
    let others = vec![ConflictOther {
        cpv: "dev-libs/t-1.0:0/0::r".to_string(),
        iuse: ["x".to_string()].into_iter().collect(),
        use_flags: ["x".to_string()].into_iter().collect(),
        installed: false,
    }];
    let ge = parse_atom(">=dev-libs/t-2.0").unwrap();
    assert_eq!(
        slot_conflict_reasons(&ge, &others),
        (vec![SlotConflictReason::Version("ge")], false,)
    );
    let bare = parse_atom("dev-libs/t").unwrap();
    assert!(slot_conflict_reasons(&bare, &others).0.is_empty());
    let lt = parse_atom("<dev-libs/t-2.0").unwrap();
    assert!(slot_conflict_reasons(&lt, &others).0.is_empty());
    let ok = parse_atom(">=dev-libs/t-1.0[x]").unwrap();
    assert!(slot_conflict_reasons(&ok, &others).0.is_empty());
    let others_off = vec![ConflictOther {
        cpv: "dev-libs/t-1.0:0/0::r".to_string(),
        iuse: ["x".to_string()].into_iter().collect(),
        use_flags: HashSet::new(),
        installed: false,
    }];
    assert_eq!(
        slot_conflict_reasons(&ok, &others_off),
        (vec![SlotConflictReason::Use("x".to_string())], false,)
    );
    let others_gone = vec![ConflictOther {
        cpv: "dev-libs/t-1.0:0/0::r".to_string(),
        iuse: HashSet::new(),
        use_flags: HashSet::new(),
        installed: false,
    }];
    assert_eq!(
        slot_conflict_reasons(&ok, &others_gone),
        (vec![SlotConflictReason::Use("x".to_string())], true,)
    );

    // Portage: slot_collision.py, _prepare_conflict_msg: a version mismatch
    // skips the USE check even when the USE deps would also fail.
    let both = parse_atom(">=dev-libs/t-2.0[x]").unwrap();
    assert_eq!(
        slot_conflict_reasons(&both, &others_off),
        (vec![SlotConflictReason::Version("ge")], false,)
    );

    // Caret covers the operator and the version token.
    let idx = slot_conflict_caret_idx(">=dev-libs/t-2.0", &ge, true, false, &[]);
    let ver_start = ">=dev-libs/t-2.0".rfind("2.0").unwrap();
    let expected: std::collections::HashSet<usize> =
        [0, 1, ver_start, ver_start + 1, ver_start + 2]
            .into_iter()
            .collect();
    assert_eq!(idx, expected);
    // Caret covers the violated `[x]` token.
    let idx = slot_conflict_caret_idx(">=dev-libs/t-1.0[x]", &ok, false, false, &["x".to_string()]);
    let use_start = ">=dev-libs/t-1.0[x]".find('x').unwrap();
    let expected: std::collections::HashSet<usize> = [use_start].into_iter().collect();
    assert_eq!(idx, expected);
}
