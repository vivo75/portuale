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

#[test]
fn slot_conflict_colorized_spans_keep_markers_aligned() {
    use crate::color::Colorizer;
    let atom_str = ">=dev-libs/t-2.0";
    let ge = parse_atom(atom_str).unwrap();
    let idx = slot_conflict_caret_idx(atom_str, &ge, true, false, &[]);
    let (plain, shifted) = colorize_marked_spans(atom_str, &idx, &Colorizer::new(false));
    assert_eq!(plain, atom_str);
    assert_eq!(shifted, idx);
    // Each caret must land on visible text, never inside an escape.
    let (colored, shifted) = colorize_marked_spans(atom_str, &idx, &Colorizer::new(true));
    assert!(colored.len() > atom_str.len());
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

#[test]
fn skip_caret_line_pads_to_the_atom_and_marks_a_violated_use_dep() {
    // Portage: resolver/output.py, format_unmatched_atom (marker is atom-long;
    // `[nginx]` is marked when the missed package has it disabled).
    let none: Vec<(String, String)> = Vec::new();
    let line = skip_conflict_caret_line("<a/b-2.0:=", "0", "0", true, &none);
    assert_eq!(line, "^    ^^^  ");
    assert_eq!(line.len(), "<a/b-2.0:=".len());
    let off = vec![("USE".to_string(), "-nginx".to_string())];
    let line = skip_conflict_caret_line("a/b[nginx]", "0", "0", false, &off);
    assert_eq!(line, "    ^^^^^ ");
    let on = vec![("USE".to_string(), "nginx".to_string())];
    let line = skip_conflict_caret_line("a/b[nginx]", "0", "0", false, &on);
    assert_eq!(line.trim(), "");
    assert_eq!(line.len(), "a/b[nginx]".len());
}
