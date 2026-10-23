use super::*;

#[test]
fn root_suffix_annotates_each_entry_with_its_own_root() {
    // Portage: output.py. Each merge row carries its own non-`/` root as unquoted `to <EROOT>`.
    use std::path::Path;
    let target_entry = || {
        entry_with_use(
            PretendOutcome::New {
                version: "1.0".into(),
            },
            "",
            "",
        )
    };
    let running_entry = || {
        let mut e = entry_with_use(
            PretendOutcome::New {
                version: "1.0".into(),
            },
            "",
            "",
        );
        e.targets_running_root = true;
        e
    };
    let stage = Path::new("/mnt/stage3");
    let slash = Path::new("/");
    let running = Path::new("/tmp/running");

    assert_eq!(root_suffix(&target_entry(), slash, None), "");
    assert_eq!(root_suffix(&target_entry(), stage, None), "to /mnt/stage3");
    assert_eq!(
        root_suffix(&target_entry(), stage, Some(running)),
        "to /mnt/stage3"
    );
    assert_eq!(
        root_suffix(&running_entry(), stage, Some(running)),
        "to /tmp/running"
    );
    assert_eq!(root_suffix(&running_entry(), stage, Some(slash)), "");
    assert_eq!(root_suffix(&running_entry(), stage, None), "");
}

#[test]
fn attr_display_field_carries_the_seventh_mask_column_unless_quiet() {
    // Portage: output.py, set_pkg_info. The mask column shows unless `--quiet`.
    let nc = Colorizer::new(false);
    let plain = attr_display_field(AttrDisplayFieldArgs {
        interactive: false,
        new: false,
        force_reinstall: false,
        new_slot: false,
        replace: true,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        remote_binary: false,
        new_version: false,
        downgrade: false,
        mask: None,
        include_mask: true,
        color: &nc,
    });
    assert_eq!(plain, "  R    ");
    assert_eq!(plain.chars().count(), 7);

    let quiet = attr_display_field(AttrDisplayFieldArgs {
        interactive: false,
        new: false,
        force_reinstall: false,
        new_slot: false,
        replace: true,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        remote_binary: false,
        new_version: false,
        downgrade: false,
        mask: None,
        include_mask: false,
        color: &nc,
    });
    assert_eq!(quiet, "  R   ");
    assert_eq!(quiet.chars().count(), 6);

    let masked = attr_display_field(AttrDisplayFieldArgs {
        interactive: false,
        new: true,
        force_reinstall: false,
        new_slot: false,
        replace: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        remote_binary: false,
        new_version: false,
        downgrade: false,
        mask: Some('~'),
        include_mask: true,
        color: &nc,
    });
    assert_eq!(masked, " N    ~");
    assert_eq!(masked.chars().count(), 7);
}

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
fn nomerge_row_carries_the_use_suffix_when_the_package_has_one() {
    // Portage: resolver/output.py, Display. Nomerge rows carry the same USE
    // suffix as merge rows, never a size suffix.
    let nc = Colorizer::new(false);
    let gpcyclea = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "x",
        "x",
    );
    let use_str = use_suffix(UseSuffixArgs {
        entry: &gpcyclea,
        verbose: false,
        quiet: false,
        alphabetical: false,
        color: &nc,
    });
    assert_eq!(use_str, " USE=\"x\" ");
    assert_eq!(
        nomerge_row(NomergeRowArgs {
            category: "dev-libs",
            package: "gpcyclea",
            disp_ver: "1.0",
            indent: " ",
            quiet: false,
            tail: &use_str,
            color: &nc,
            color_key: "PKG_NOMERGE",
        }),
        "[nomerge       ]  dev-libs/gpcyclea-1.0 USE=\"x\" "
    );
    let bare = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    let bare_use = use_suffix(UseSuffixArgs {
        entry: &bare,
        verbose: false,
        quiet: false,
        alphabetical: false,
        color: &nc,
    });
    assert_eq!(bare_use, "");
    assert_eq!(
        nomerge_row(NomergeRowArgs {
            category: "dev-libs",
            package: "diamond",
            disp_ver: "1.0",
            indent: "",
            quiet: false,
            tail: &bare_use,
            color: &nc,
            color_key: "PKG_NOMERGE",
        }),
        "[nomerge       ] dev-libs/diamond-1.0"
    );
    assert_eq!(
        nomerge_row(NomergeRowArgs {
            category: "dev-libs",
            package: "diamond",
            disp_ver: "1.0",
            indent: " ",
            quiet: true,
            tail: &bare_use,
            color: &nc,
            color_key: "PKG_NOMERGE",
        }),
        "[nomerge      ]  dev-libs/diamond-1.0"
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

#[test]
fn nomerge_row_colours_word_and_package_with_the_row_colour() {
    // Portage `Display.pkgprint`: `[` + colour("nomerge".ljust(13)) + " ] "
    // + colour(pkg); `PKG_NOMERGE` is teal (LLM/evidence/2026-10-23-506,
    // shape 02, `ROOT=/` so the tail is the bare space before oldbest).
    let c = Colorizer::new(true);
    assert_eq!(
        nomerge_row(NomergeRowArgs {
            category: "dev-libs",
            package: "usecyclea",
            disp_ver: "1.0",
            indent: "",
            quiet: false,
            tail: " ",
            color: &c,
            color_key: "PKG_NOMERGE",
        }),
        "[\x1b[36mnomerge      \x1b[39;49;00m ] \x1b[36mdev-libs/usecyclea-1.0\x1b[39;49;00m "
    );
}
