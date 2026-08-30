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
