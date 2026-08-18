use super::*;

#[test]
fn use_suffix_picks_the_p_or_pv_use_rendering_by_verbosity() {
    let nc = Colorizer::new(false);
    // New entries render the same list at `-p` and `-pv`.
    let new = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "bar -baz",
        "bar -baz",
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &new,
            verbose: false,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar -baz\" "
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &new,
            verbose: true,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar -baz\" "
    );
    // `--quiet` hides USE unless `-v` too.
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &new,
            verbose: false,
            quiet: true,
            alphabetical: false,
            color: &nc
        }),
        ""
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &new,
            verbose: true,
            quiet: true,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar -baz\" "
    );

    // Reinstalls show only the change at `-p`, the full list at `-pv`.
    let reinstall = entry_with_use(
        PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: vec![],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        "bar* -baz",
        "bar*",
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &reinstall,
            verbose: false,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar*\" "
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &reinstall,
            verbose: true,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar* -baz\" "
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &reinstall,
            verbose: true,
            quiet: true,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar* -baz\" "
    );

    // An unchanged reinstall shows nothing at `-p`.
    let unchanged = entry_with_use(
        PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: vec![],
            deps_changed: true,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        "bar -baz",
        "",
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &unchanged,
            verbose: false,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        ""
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &unchanged,
            verbose: true,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar -baz\" "
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &unchanged,
            verbose: true,
            quiet: true,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar -baz\" "
    );
}

#[test]
fn use_suffix_renders_portages_trailing_space_after_each_use_group() {
    // Portage: output_helpers.py, _create_use_string. Every non-empty USE
    // group ends with a trailing space.
    let nc = Colorizer::new(false);
    let reinstall = entry_with_use(
        PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: vec![],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        "bar* -baz",
        "bar*",
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &reinstall,
            verbose: false,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar*\" "
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &reinstall,
            verbose: true,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        " USE=\"bar* -baz\" "
    );
    let bare = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    assert_eq!(
        use_suffix(UseSuffixArgs {
            entry: &bare,
            verbose: false,
            quiet: false,
            alphabetical: false,
            color: &nc
        }),
        ""
    );
}
