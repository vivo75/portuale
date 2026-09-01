use super::*;

#[test]
fn parse_updates_content_keeps_move_and_slotmove_drops_the_rest() {
    let cmds = parse_updates_content(
        "move dev-libs/foo dev-libs/bar\n\
         # a comment line, not a directive\n\
         slotmove dev-libs/baz 0 1\n\
         move dev-libs/foo dev-libs/foo-1.0\n\
         frobnicate dev-libs/x dev-libs/y\n\
         move only-two-tokens\n\
         slotmove dev-libs/qux 0\n",
    );
    assert_eq!(
        cmds,
        vec![
            UpdateCmd::Move {
                old: ("dev-libs".into(), "foo".into()),
                new: ("dev-libs".into(), "bar".into()),
            },
            UpdateCmd::SlotMove {
                cp: ("dev-libs".into(), "baz".into()),
                old_slot: "0".into(),
                new_slot: "1".into(),
            },
        ]
    );
}

#[test]
fn apply_move_and_slotmove_to_a_token_preserve_the_other_atom_parts() {
    let old = ("dev-libs".to_string(), "foo".to_string());
    let new = ("sys-libs".to_string(), "bar".to_string());
    assert_eq!(
        apply_move_to_token("dev-libs/foo", &old, &new),
        "sys-libs/bar"
    );
    assert_eq!(
        apply_move_to_token(">=dev-libs/foo-1.2:3=", &old, &new),
        ">=sys-libs/bar-1.2:3="
    );
    assert_eq!(
        apply_move_to_token("!dev-libs/foo", &old, &new),
        "!sys-libs/bar"
    );
    // A different cp is untouched.
    assert_eq!(
        apply_move_to_token("dev-libs/other", &old, &new),
        "dev-libs/other"
    );

    let cp = ("dev-libs".to_string(), "baz".to_string());
    assert_eq!(
        apply_slotmove_to_token("dev-libs/baz:0", &cp, "0", "1"),
        "dev-libs/baz:1"
    );
    assert_eq!(
        apply_slotmove_to_token("dev-libs/baz:0/0=", &cp, "0", "1"),
        "dev-libs/baz:1/1="
    );
    // A versioned atom or a non-matching slot is left alone.
    // Portage: portage.update, update_dbentry
    assert_eq!(
        apply_slotmove_to_token("=dev-libs/baz-1.0:0", &cp, "0", "1"),
        "=dev-libs/baz-1.0:0"
    );
    assert_eq!(
        apply_slotmove_to_token("dev-libs/baz:2", &cp, "0", "1"),
        "dev-libs/baz:2"
    );
}
