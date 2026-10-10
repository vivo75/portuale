use super::*;

#[test]
fn slot_conflict_reason_and_caret_span() {
    let others = vec![ConflictOther {
        cpv: "dev-libs/t-1.0:0/0::r".to_string(),
        iuse: ["x".to_string()].into_iter().collect(),
        use_flags: ["x".to_string()].into_iter().collect(),
        installed: false,
    }];
    // ">=dev-libs/t-2.0" does not accept 1.0 -> ("version", "ge")
    let ge = parse_atom(">=dev-libs/t-2.0").unwrap();
    assert_eq!(
        slot_conflict_reasons(&ge, &others),
        (vec![SlotConflictReason::Version("ge")], false,)
    );
    // a bare atom accepts any version -> no reason
    let bare = parse_atom("dev-libs/t").unwrap();
    assert!(slot_conflict_reasons(&bare, &others).0.is_empty());
    // "<dev-libs/t-2.0" DOES accept 1.0 -> no reason against this other
    let lt = parse_atom("<dev-libs/t-2.0").unwrap();
    assert!(slot_conflict_reasons(&lt, &others).0.is_empty());
    // ">=dev-libs/t-1.0[x]" accepts 1.0 on version+slot but x is on
    // here, so no reason either...
    let ok = parse_atom(">=dev-libs/t-1.0[x]").unwrap();
    assert!(slot_conflict_reasons(&ok, &others).0.is_empty());
    // ...while x off the other side is a violated use key ...
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
    // ... and x gone from IUSE is an unconditional one.
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

    // Real's `elif` chain (`slot_collision.py:307-397`): a
    // version-mismatched pair skips the USE check even when the
    // atom's USE deps would also fail against the other instance.
    let both = parse_atom(">=dev-libs/t-2.0[x]").unwrap();
    assert_eq!(
        slot_conflict_reasons(&both, &others_off),
        (vec![SlotConflictReason::Version("ge")], false,)
    );

    // caret span: under ">=" (idx 0,1) and under "2.0" (rfind)
    let idx = slot_conflict_caret_idx(">=dev-libs/t-2.0", &ge, true, false, &[]);
    let ver_start = ">=dev-libs/t-2.0".rfind("2.0").unwrap();
    let expected: std::collections::HashSet<usize> =
        [0, 1, ver_start, ver_start + 1, ver_start + 2]
            .into_iter()
            .collect();
    assert_eq!(idx, expected);
    // caret span: under the violated "[x]" token
    let idx = slot_conflict_caret_idx(">=dev-libs/t-1.0[x]", &ok, false, false, &["x".to_string()]);
    let use_start = ">=dev-libs/t-1.0[x]".find('x').unwrap();
    let expected: std::collections::HashSet<usize> = [use_start].into_iter().collect();
    assert_eq!(idx, expected);
}

#[test]
fn slot_conflict_colorized_spans_keep_markers_aligned() {
    use crate::color::Colorizer;
    let atom_str = ">=dev-libs/t-2.0";
    let ge = parse_atom(atom_str).unwrap();
    let idx = slot_conflict_caret_idx(atom_str, &ge, true, false, &[]);
    // Color off: identity -- same string, same indices.
    let (plain, shifted) = colorize_marked_spans(atom_str, &idx, &Colorizer::new(false));
    assert_eq!(plain, atom_str);
    assert_eq!(shifted, idx);
    // Color on: every marked char wrapped, and each caret lands on
    // visible text (never inside an escape), spelling the violated
    // spans back out.
    let (colored, shifted) = colorize_marked_spans(atom_str, &idx, &Colorizer::new(true));
    assert!(colored.len() > atom_str.len());
    // Map each raw offset to its visible char (None inside escapes).
    let mut visible_at: Vec<Option<char>> = Vec::new();
    let mut in_esc = false;
    for c in colored.chars() {
        if c == '\x1b' {
            in_esc = true;
            visible_at.push(None);
        } else if in_esc {
            visible_at.push(None);
            if c == 'm' {
                in_esc = false;
            }
        } else {
            visible_at.push(Some(c));
        }
    }
    let stripped: String = visible_at.iter().filter_map(|c| *c).collect();
    assert_eq!(stripped, atom_str);
    let mut ordered: Vec<usize> = shifted.iter().copied().collect();
    ordered.sort_unstable();
    let careted: String = ordered
        .iter()
        .filter_map(|k| visible_at.get(*k).copied().flatten())
        .collect();
    assert_eq!(ordered.len(), idx.len());
    assert_eq!(careted, ">=2.0");
}
