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

/// How the conflict was rendered: the two flags the explanation reads.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Plain,
    VersionConflict,
    Unspecific,
}

fn explanation_for(
    solutions: &portage_repo::SlotConflictSolutions,
    shown: Shown,
    color: bool,
) -> Option<String> {
    let color = Colorizer::new(color);
    slot_conflict_explanation(SlotConflictExplanationArgs {
        solutions,
        is_a_version_conflict: shown == Shown::VersionConflict,
        conflict_is_unspecific: shown == Shown::Unspecific,
        newuse_and_update: false,
        color: &color,
    })
}

fn use_change(cpv: &str, flags: &[(&str, bool)]) -> portage_repo::UseChange {
    portage_repo::UseChange {
        cpv: cpv.to_string(),
        flags: flags.iter().map(|(f, v)| ((*f).to_string(), *v)).collect(),
    }
}

/// Portage 3.0.83 `get_explanation` (`slot_collision.py:728-784`), upstream
/// `testSlotCollision` case 6 (`sci-libs/L sci-libs/M`): one solution, one
/// conflict, plain text with `--color=n`.
#[test]
fn explanation_lists_the_one_change_with_three_space_indent() {
    let sols = portage_repo::SlotConflictSolutions {
        conflict_count: 1,
        solution_count: 1,
        changes: vec![vec![
            use_change("sci-libs/K-1", &[("foo", false)]),
            use_change("sci-libs/M-1", &[("foo", false)]),
        ]],
    };
    assert_eq!(
        explanation_for(&sols, Shown::Plain, false).as_deref(),
        Some(
            "It might be possible to solve this slot collision\n\
             by applying all of the following changes:\n   \
             - sci-libs/K-1 (Change USE: -foo)\n   \
             - sci-libs/M-1 (Change USE: -foo)\n\n"
        )
    );
}

/// The flags are `red` for `+` and `blue` for `-` (`slot_collision.py:766`),
/// as the host `emerge --color=y` capture of the cgp keeper shows.
#[test]
fn explanation_colours_the_flags() {
    let sols = portage_repo::SlotConflictSolutions {
        conflict_count: 1,
        solution_count: 1,
        changes: vec![vec![use_change(
            "dev-libs/cgp0x-2.8.0",
            &[("icu", false), ("bar", true)],
        )]],
    };
    let text = explanation_for(&sols, Shown::Plain, true).unwrap();
    assert!(
        text.contains(
            "   - dev-libs/cgp0x-2.8.0 (Change USE: \x1b[34;01m-icu\x1b[39;49;00m \
             \x1b[31;01m+bar\x1b[39;49;00m)\n"
        ),
        "{text:?}"
    );
}

/// Quirk Q22, upstream case 7: two solutions that minimise to one change keep
/// the "one of the following solutions" header over a single change.
#[test]
fn explanation_header_counts_solutions_but_body_counts_changes() {
    let sols = portage_repo::SlotConflictSolutions {
        conflict_count: 1,
        solution_count: 2,
        changes: vec![vec![use_change("sci-libs/Q-1", &[("foo", true)])]],
    };
    assert_eq!(
        explanation_for(&sols, Shown::Plain, false).as_deref(),
        Some(
            "It might be possible to solve this slot collision\n\
             by applying one of the following solutions:\n   \
             - sci-libs/Q-1 (Change USE: +foo)\n\n"
        )
    );
}

/// With several minimal changes each prints under `Solution: Apply all of:`,
/// and several conflicts say "these slot collisions".
#[test]
fn explanation_lists_each_of_several_changes() {
    let sols = portage_repo::SlotConflictSolutions {
        conflict_count: 2,
        solution_count: 2,
        changes: vec![
            vec![use_change("a/b-1", &[("x", true)])],
            vec![use_change("a/c-1", &[("y", false)])],
        ],
    };
    assert_eq!(
        explanation_for(&sols, Shown::Plain, false).as_deref(),
        Some(
            "It might be possible to solve these slot collisions\n\
             by applying one of the following solutions:\n  \
             Solution: Apply all of:\n     \
             - a/b-1 (Change USE: +x)\n\n  \
             Solution: Apply all of:\n     \
             - a/c-1 (Change USE: -y)\n\n"
        )
    );
}

/// A version or slot conflict gets no explanation (`:731-732`); no solution
/// gets none either (`:741-743`); an unspecific conflict gets the `--newuse`
/// hint even without solutions (`:734-739`), unless both options are given.
#[test]
fn explanation_is_absent_for_version_conflicts_and_empty_searches() {
    let sols = portage_repo::SlotConflictSolutions {
        conflict_count: 1,
        solution_count: 1,
        changes: vec![vec![use_change("a/b-1", &[("x", true)])]],
    };
    assert_eq!(explanation_for(&sols, Shown::VersionConflict, false), None);
    let none = portage_repo::SlotConflictSolutions::default();
    assert_eq!(explanation_for(&none, Shown::Plain, false), None);
    assert_eq!(
        explanation_for(&none, Shown::Unspecific, false).as_deref(),
        Some(
            "!!! Enabling --newuse and --update might solve this conflict.\n\
             !!! If not, it might help emerge to give a more specific suggestion.\n\n"
        )
    );
    let color = Colorizer::new(false);
    assert_eq!(
        slot_conflict_explanation(SlotConflictExplanationArgs {
            solutions: &none,
            is_a_version_conflict: false,
            conflict_is_unspecific: true,
            newuse_and_update: true,
            color: &color,
        }),
        None
    );
}
