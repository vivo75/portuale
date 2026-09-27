use super::*;

/// The merge progress counters: only merging outcomes count, and each merging
/// entry's `cur` is its 1-based ordinal in merge order. The map is positional,
/// so two merging entries for the same cat/pkg keep their own `cur`.
/// Expected value from Portage source.
#[test]
fn merge_progress_map_counts_only_merging_entries_in_order() {
    let mut slot1 = source_entry(
        "same-pkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    slot1.slot = Some("1".into());
    let mut slot2 = source_entry(
        "same-pkg",
        PretendOutcome::Upgrade {
            from: "0.9".into(),
            to: "1.0".into(),
        },
    );
    slot2.slot = Some("2".into());
    let entries = vec![
        slot1,
        source_entry(
            "b-pkg",
            PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            },
        ),
        slot2,
        source_entry(
            "d-pkg",
            PretendOutcome::Uninstall {
                version: "1.0".into(),
            },
        ),
    ];
    let map = merge_progress_map(&entries);
    assert_eq!(map.len(), 4);
    // The two same-cat/pkg entries print their own `cur`.
    assert_eq!(map[0], mrg_director::MergeProgress { cur: 1, max: 2 });
    assert_eq!(map[2], mrg_director::MergeProgress { cur: 2, max: 2 });
    // Non-merging entries carry the never-printed placeholder.
    assert_eq!(map[1], mrg_director::MergeProgress::single());
    assert_eq!(map[3], mrg_director::MergeProgress::single());
    // And the lines really read `(1 of 2)` / `(2 of 2)`.
    let plain = crate::color::Colorizer::new(false);
    let root = std::path::Path::new("/");
    assert_eq!(
        emerging_line(&entries[0], "1.0", map[0], root, &plain),
        ">>> Emerging (1 of 2) dev-libs/same-pkg-1.0::testrepo"
    );
    assert_eq!(
        emerging_line(&entries[2], "1.0", map[2], root, &plain),
        ">>> Emerging (2 of 2) dev-libs/same-pkg-1.0::testrepo"
    );
}

/// The exact progress-line strings, colour escapes included: `ROOT != "/"`
/// appends ` for {root}` (Emerging) vs ` to {root}` (Installing/Completed).
/// Expected value from Portage source.
#[test]
fn merge_progress_lines_match_portage_exact_text() {
    use std::path::Path;
    let plain = crate::color::Colorizer::new(false);
    let root = Path::new("/");
    let two_of_three = mrg_director::MergeProgress { cur: 2, max: 3 };
    let a = source_entry(
        "a-pkg",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let mut b = source_entry(
        "b-pkg",
        PretendOutcome::New {
            version: "2.0".into(),
        },
    );
    b.source = CandidateSource::Binary;

    // Source, colour off, `ROOT == "/"`: the bare shape.
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Emerging (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );
    // Binary: `Emerging binary`, same counters.
    assert_eq!(
        emerging_line(&b, "2.0", two_of_three, root, &plain),
        ">>> Emerging binary (2 of 3) dev-libs/b-pkg-2.0::testrepo"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Installing (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );
    assert_eq!(
        completed_line(&a, "1.0", two_of_three, root, &plain),
        ">>> Completed (2 of 3) dev-libs/a-pkg-1.0::testrepo"
    );

    // `ROOT != "/"`: `for` on Emerging, `to` on Installing/Completed.
    let alt = Path::new("/altroot");
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Emerging (2 of 3) dev-libs/a-pkg-1.0::testrepo for /altroot"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Installing (2 of 3) dev-libs/a-pkg-1.0::testrepo to /altroot"
    );
    assert_eq!(
        completed_line(&a, "1.0", two_of_three, alt, &plain),
        ">>> Completed (2 of 3) dev-libs/a-pkg-1.0::testrepo to /altroot"
    );

    // Colour on: only the two counters and the `cpv::repo` wear escapes; the
    // action word, parens, `of`, and `for ROOT` stay plain.
    let live = crate::color::Colorizer::new(true);
    assert_eq!(
        emerging_line(&a, "1.0", two_of_three, root, &live),
        ">>> Emerging (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[32mdev-libs/a-pkg-1.0::testrepo\x1b[39;49;00m"
    );
    assert_eq!(
        emerging_line(&b, "2.0", two_of_three, root, &live),
        ">>> Emerging binary (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[35mdev-libs/b-pkg-2.0::testrepo\x1b[39;49;00m"
    );
    assert_eq!(
        installing_line(&a, "1.0", two_of_three, root, &live),
        ">>> Installing (\x1b[33;01m2\x1b[39;49;00m of \x1b[33;01m3\x1b[39;49;00m) \x1b[32mdev-libs/a-pkg-1.0::testrepo\x1b[39;49;00m"
    );
}
