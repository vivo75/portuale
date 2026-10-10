use super::*;

#[test]
fn root_suffix_annotates_each_entry_with_its_own_root() {
    // Backlog #242 Slice D: real `output.py:841-862` appends unquoted
    // `to <EROOT>` on every merge row whose OWN root is not `/` (S0 map
    // `LLM/evidence/2026-10-02-242-display-sites.md` §1a) — the #206
    // cut reopened per display site.
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

    // Single-root `/`: everything bare (L0).
    assert_eq!(root_suffix(&target_entry(), slash, None), "");
    // Staged target: target rows carry the target root (reopened cut).
    assert_eq!(root_suffix(&target_entry(), stage, None), "to /mnt/stage3");
    // Dual roots: each side annotates its own root...
    assert_eq!(
        root_suffix(&target_entry(), stage, Some(running)),
        "to /mnt/stage3"
    );
    assert_eq!(
        root_suffix(&running_entry(), stage, Some(running)),
        "to /tmp/running"
    );
    // ...and a `/`-rooted row stays bare even under a staged target
    // (real's blk0x/cyc0y rows, S0 §1a).
    assert_eq!(root_suffix(&running_entry(), stage, Some(slash)), "");
    // Defensive: a running entry with no running root in hand stays bare.
    assert_eq!(root_suffix(&running_entry(), stage, None), "");
}

#[test]
fn attr_display_field_carries_the_seventh_mask_column_unless_quiet() {
    // Real `set_pkg_info` fills the mask column `if
    // self.include_mask_str()` (`verbosity > 1`), and real default
    // `emerge -p` verbosity is 2 -- so the field is 7 columns even
    // without `-v`, dropping to 6 only under `--quiet` (verbosity 1,
    // `include_mask` false). A plain reinstall with no keyword/hard
    // mask -> `I N S f U D` = `  R   ` then a bare space.
    let nc = Colorizer::new(false);
    let plain = attr_display_field(
        false, false, false, false, true, false, false, false, false, false, None, true, &nc,
    );
    assert_eq!(plain, "  R    ");
    assert_eq!(plain.chars().count(), 7);

    // Same entry under `--quiet`: 6 columns, no mask slot.
    let quiet = attr_display_field(
        false, false, false, false, true, false, false, false, false, false, None, false, &nc,
    );
    assert_eq!(quiet, "  R   ");
    assert_eq!(quiet.chars().count(), 6);

    // A `~arch`-only-visible New -> the 7th column is `~`
    // (`gen_mask_str` "unstable"), not a space.
    let masked = attr_display_field(
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        Some('~'),
        true,
        &nc,
    );
    assert_eq!(masked, " N    ~");
    assert_eq!(masked.chars().count(), 7);
}

#[test]
fn use_suffix_picks_the_p_or_pv_use_rendering_by_verbosity() {
    let nc = Colorizer::new(false);
    // A `New` entry: the resolver renders the same full list into
    // both fields (`is_new` renders every flag regardless of
    // `all_flags`), so `-p` and `-pv` show the same USE line.
    let new = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "bar -baz",
        "bar -baz",
    );
    assert_eq!(
        use_suffix(&new, false, false, false, &nc),
        " USE=\"bar -baz\" "
    );
    assert_eq!(
        use_suffix(&new, true, false, false, &nc),
        " USE=\"bar -baz\" "
    );
    // `--quiet` (verbosity 1): no USE line at all unless `-v` too.
    assert_eq!(use_suffix(&new, false, true, false, &nc), "");
    // `-pvq`: `print_use_string` true again, `all_flags` true -> the
    // full list (like `-pv`).
    assert_eq!(
        use_suffix(&new, true, true, false, &nc),
        " USE=\"bar -baz\" "
    );

    // A `Reinstall` with one flipped flag: `-pv` shows the full list,
    // plain `-p` shows only the change.
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
        use_suffix(&reinstall, false, false, false, &nc),
        " USE=\"bar*\" "
    );
    assert_eq!(
        use_suffix(&reinstall, true, false, false, &nc),
        " USE=\"bar* -baz\" "
    );
    // `-pvq`: full list, same as `-pv`.
    assert_eq!(
        use_suffix(&reinstall, true, true, false, &nc),
        " USE=\"bar* -baz\" "
    );

    // A `Reinstall` with nothing changed: no USE line at `-p`.
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
    assert_eq!(use_suffix(&unchanged, false, false, false, &nc), "");
    assert_eq!(
        use_suffix(&unchanged, true, false, false, &nc),
        " USE=\"bar -baz\" "
    );
    // `-pvq`: `all_flags` true -> the full list shows even though
    // nothing changed.
    assert_eq!(
        use_suffix(&unchanged, true, true, false, &nc),
        " USE=\"bar -baz\" "
    );
}

#[test]
fn nomerge_row_carries_the_use_suffix_when_the_package_has_one() {
    // #154 (`--tree dev-libs/gpcyclec`): the ancestor occurrence of
    // a merge node renders through the nomerge arm, which appends
    // the same USE suffix as a merge row. Real
    // `resolver/output.py`: `Display.__call__` runs `_display_use`
    // for every package (no ordered/merge gate) and
    // `print_messages` appends `" " + verboseadd`; `verbose_size`
    // is merge-gated so no size suffix ever follows on this arm.
    let nc = Colorizer::new(false);
    // The gpcyclec cell shape: a `New` entry whose `-p` USE display
    // is `x` (gpcyclea-1.0, IUSE=x, enabled).
    let gpcyclea = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "x",
        "x",
    );
    let use_str = use_suffix(&gpcyclea, false, false, false, &nc);
    assert_eq!(use_str, " USE=\"x\" ");
    assert_eq!(
        nomerge_row("dev-libs", "gpcyclea", "1.0", " ", false, &use_str),
        "[nomerge       ]  dev-libs/gpcyclea-1.0 USE=\"x\" "
    );
    // A USE-less package (empty display, e.g. `dev-libs/diamond`):
    // the tail is empty and the row is byte-identical to the
    // pre-#154 rendering, so no existing pin moves.
    let bare = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    let bare_use = use_suffix(&bare, false, false, false, &nc);
    assert_eq!(bare_use, "");
    assert_eq!(
        nomerge_row("dev-libs", "diamond", "1.0", "", false, &bare_use),
        "[nomerge       ] dev-libs/diamond-1.0"
    );
    // `--quiet` keeps the established narrow pad.
    assert_eq!(
        nomerge_row("dev-libs", "diamond", "1.0", " ", true, &bare_use),
        "[nomerge      ]  dev-libs/diamond-1.0"
    );
}

#[test]
fn use_suffix_renders_portages_trailing_space_after_each_use_group() {
    // Backlog #272: real `_create_use_string`
    // (`output_helpers.py:336-338`) returns `f'{name}="{ret}" '` --
    // one trailing space per non-empty group -- and `print_messages`
    // (`output.py:542-544`) joins it with a single leading space, so
    // every painted merge-list row ends with `" `, at `-p` and `-v`
    // alike. Grounded on host real 3.0.82.2 (`/usr/sbin/emerge`)
    // staged-fixture probes (`--pretend --autounmask
    // "dev-libs/useflagpkg[missingflag]"`, New and installed-USE
    // reinstall shapes): real's changed row reads
    // `... USE="missingflag*" ` where portuale printed
    // `... USE="missingflag*"`.
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
    // Plain `-p`: the changed flag paints, with real's trailing space.
    assert_eq!(
        use_suffix(&reinstall, false, false, false, &nc),
        " USE=\"bar*\" "
    );
    // `-pv`: the full list, same trailing space.
    assert_eq!(
        use_suffix(&reinstall, true, false, false, &nc),
        " USE=\"bar* -baz\" "
    );
    // Bare rows stay bare: an empty display still renders nothing.
    let bare = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    assert_eq!(use_suffix(&bare, false, false, false, &nc), "");
}
