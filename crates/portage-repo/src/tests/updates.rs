use super::*;

/// The inverted `installed_cp_sources` map answers what the old
/// per-call command loop did, on whatever `profiles/updates/` exist.
#[test]
fn installed_cp_sources_map_matches_the_command_loop() {
    for (cat, pkg) in [
        ("dev-libs", "cachetest-src"),
        ("sys-libs", "glibc"),
        ("virtual", "libc"),
    ] {
        let mut expected = vec![(cat.to_string(), pkg.to_string())];
        for cmd in global_package_updates() {
            if let UpdateCmd::Move { old, .. } = cmd {
                let mapped = apply_updates_to_cp(&old.0, &old.1);
                if mapped == expected[0] && !expected.contains(old) {
                    expected.push(old.clone());
                }
            }
        }
        assert_eq!(installed_cp_sources(cat, pkg), expected);
    }
}

/// A two-command `move` cycle replays back to its start, so neither
/// side files the other as a source. The second command still renames its
/// own old name away (Portage threads the entry through every command in
/// order: `update_dbentries`' loop, `move_ent`'s sequential renames), so a
/// query for it scans nothing.
#[test]
fn installed_cp_sources_cycle_emits_no_duplicate() {
    let cmds = vec![
        UpdateCmd::Move {
            old: ("a".to_string(), "x".to_string()),
            new: ("b".to_string(), "x".to_string()),
        },
        UpdateCmd::Move {
            old: ("b".to_string(), "x".to_string()),
            new: ("a".to_string(), "x".to_string()),
        },
    ];
    assert_eq!(
        installed_cp_sources_with("a", "x", &cmds),
        vec![
            ("a".to_string(), "x".to_string()),
            ("b".to_string(), "x".to_string())
        ]
    );
    assert_eq!(
        installed_cp_sources_with("b", "x", &cmds),
        Vec::<(String, String)>::new()
    );
}

/// A `move` renames the vdb entry (Portage: vartree.py, `move_ent`): a vdb
/// query for the pre-move name must not scan its own stale directory, only
/// names whose chain lands on the queried one do.
#[test]
fn moved_away_cp_sources_drop_the_stale_pre_move_dir() {
    let cmds = vec![UpdateCmd::Move {
        old: ("dev-libs".to_string(), "oldmovepkg".to_string()),
        new: ("dev-libs".to_string(), "newmovepkg".to_string()),
    }];
    // The pre-move name resolves away: nothing to scan, not even itself.
    assert_eq!(
        installed_cp_sources_with("dev-libs", "oldmovepkg", &cmds),
        Vec::<(String, String)>::new()
    );
    // The post-move name keeps itself first, then the stale directory.
    assert_eq!(
        installed_cp_sources_with("dev-libs", "newmovepkg", &cmds),
        vec![
            ("dev-libs".to_string(), "newmovepkg".to_string()),
            ("dev-libs".to_string(), "oldmovepkg".to_string()),
        ]
    );
}

/// A chained move (`a -> b -> c`): only the final name scans anything; the
/// intermediate name's directory was renamed away twice over.
#[test]
fn chained_move_sources_land_only_on_the_final_name() {
    let cmds = vec![
        UpdateCmd::Move {
            old: ("dev-libs".to_string(), "a".to_string()),
            new: ("dev-libs".to_string(), "b".to_string()),
        },
        UpdateCmd::Move {
            old: ("dev-libs".to_string(), "b".to_string()),
            new: ("dev-libs".to_string(), "c".to_string()),
        },
    ];
    assert_eq!(
        installed_cp_sources_with("dev-libs", "a", &cmds),
        Vec::<(String, String)>::new()
    );
    assert_eq!(
        installed_cp_sources_with("dev-libs", "b", &cmds),
        Vec::<(String, String)>::new()
    );
    assert_eq!(
        installed_cp_sources_with("dev-libs", "c", &cmds),
        vec![
            ("dev-libs".to_string(), "c".to_string()),
            ("dev-libs".to_string(), "a".to_string()),
            ("dev-libs".to_string(), "b".to_string()),
        ]
    );
}

/// A `slotmove` never renames the entry: the queried name keeps scanning
/// itself, with no extra sources.
#[test]
fn slotmove_only_sources_keep_the_queried_dir() {
    let cmds = vec![UpdateCmd::SlotMove {
        cp: ("dev-libs".to_string(), "slotmovepkg".to_string()),
        old_slot: "0".to_string(),
        new_slot: "1".to_string(),
    }];
    assert_eq!(
        installed_cp_sources_with("dev-libs", "slotmovepkg", &cmds),
        vec![("dev-libs".to_string(), "slotmovepkg".to_string())]
    );
}

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

/// The dep-string rewrite is lazy: `None` when no move touched a
/// token, `Some(rewritten)` otherwise, whitespace preserved.
#[test]
fn apply_updates_to_dep_string_reports_unchanged_and_rewrites_only_moved_tokens() {
    let old = ("dev-libs".to_string(), "oldname".to_string());
    let new = ("dev-libs".to_string(), "newname".to_string());
    let cmds = vec![UpdateCmd::Move {
        old: old.clone(),
        new: new.clone(),
    }];
    let targets: HashMap<String, HashSet<String>> =
        HashMap::from([(old.0.clone(), HashSet::from([old.1.clone()]))]);
    // No token's cp is a move target: unchanged, no allocation.
    assert_eq!(
        apply_updates_to_dep_string_with("dev-libs/keep >=dev-libs/keep-1.2:0=\n", &cmds, &targets),
        None
    );
    // The moved token is rewritten; newlines, indentation, operators
    // and the `|| ( )` group all survive.
    assert_eq!(
        apply_updates_to_dep_string_with(
            "dev-libs/keep\n  >=dev-libs/oldname-1.2:0= || ( dev-libs/oldname:0 )",
            &cmds,
            &targets
        ),
        Some("dev-libs/keep\n  >=dev-libs/newname-1.2:0= || ( dev-libs/newname:0 )".to_string())
    );
    // A conditional marker is never parsed as an atom.
    assert_eq!(
        apply_updates_to_dep_string_with("foo? ( dev-libs/foo )", &cmds, &targets),
        None
    );
    // A slotmove rewrites only a slot-qualified token.
    let slot_cp = ("dev-libs".to_string(), "baz".to_string());
    let slot_cmds = vec![UpdateCmd::SlotMove {
        cp: slot_cp.clone(),
        old_slot: "0".to_string(),
        new_slot: "1".to_string(),
    }];
    let slot_targets: HashMap<String, HashSet<String>> =
        HashMap::from([(slot_cp.0.clone(), HashSet::from([slot_cp.1.clone()]))]);
    assert_eq!(
        apply_updates_to_dep_string_with(
            "dev-libs/baz:0 dev-libs/baz-tools:0",
            &slot_cmds,
            &slot_targets
        ),
        Some("dev-libs/baz:1 dev-libs/baz-tools:0".to_string())
    );
}
