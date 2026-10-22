use super::*;

/// Both `SkippedUpdate` sources collapse per slot keeping the highest
/// missed pkg (Portage `_get_missed_updates`); equal versions stay per
/// parent, exact duplicates collapse, mask rows lead.
#[test]
fn collapse_skipped_updates_keeps_the_highest_version_per_slot() {
    fn row(pkg: &str, ver: &str, atom: &str) -> SkippedUpdate {
        SkippedUpdate {
            category: "dev-libs".to_string(),
            package: pkg.to_string(),
            slot: "0".to_string(),
            skipped_version: ver.to_string(),
            skipped_sub_slot: "0".to_string(),
            skipped_repo: "testrepo".to_string(),
            skipped_use: Vec::new(),
            skipped_build_id: None,
            atom: atom.to_string(),
            consumer_cpv: "dev-libs/consumer-1:0/0::testrepo".to_string(),
            consumer_build_id: None,
            consumer_installed: false,
            consumer_use: Vec::new(),
            missed_targets_running_root: false,
            consumer_targets_running_root: false,
        }
    }
    // Mask source leads (first-seen order), direct source follows.
    let out = collapse_skipped_updates(vec![
        row("mgxc", "2", "=dev-libs/mgxc-1"),
        row("mgfc", "2", "=dev-libs/mgfc-2"),
        row("mgxc", "1.5", "=dev-libs/mgxc-1.5"),
        row("mgfc", "3.0", "=dev-libs/mgfc-1"),
        row("mgfc", "3.0", "=dev-libs/mgfc-1"),
        row("mgxd", "2", "=dev-libs/mgxd-2"),
        row("mgxd", "2", "<dev-libs/mgxd-3"),
    ]);
    assert_eq!(
        out.iter()
            .map(|s| (
                s.package.as_str(),
                s.skipped_version.as_str(),
                s.atom.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("mgxc", "2", "=dev-libs/mgxc-1"),
            ("mgfc", "3.0", "=dev-libs/mgfc-1"),
            ("mgxd", "2", "=dev-libs/mgxd-2"),
            ("mgxd", "2", "<dev-libs/mgxd-3"),
        ]
    );
}

/// A higher pkg replacing a lower one keeps the slot's first position
/// (Portage `_get_missed_updates` dict key order).
#[test]
fn collapse_skipped_updates_keeps_a_slots_first_position() {
    fn row(pkg: &str, ver: &str) -> SkippedUpdate {
        SkippedUpdate {
            category: "dev-libs".to_string(),
            package: pkg.to_string(),
            slot: "0".to_string(),
            skipped_version: ver.to_string(),
            skipped_sub_slot: "0".to_string(),
            skipped_repo: "testrepo".to_string(),
            skipped_use: Vec::new(),
            skipped_build_id: None,
            atom: format!("={pkg}-{ver}"),
            consumer_cpv: "dev-libs/consumer-1:0/0::testrepo".to_string(),
            consumer_build_id: None,
            consumer_installed: false,
            consumer_use: Vec::new(),
            missed_targets_running_root: false,
            consumer_targets_running_root: false,
        }
    }
    let out = collapse_skipped_updates(vec![row("x", "1"), row("y", "1"), row("x", "2")]);
    assert_eq!(
        out.iter()
            .map(|s| (s.package.as_str(), s.skipped_version.as_str()))
            .collect::<Vec<_>>(),
        vec![("x", "2"), ("y", "1")]
    );
}

/// Rows naming the same missed pkg render as one block with every
/// parent inside (Portage `_show_missed_update_slot_conflicts`).
#[test]
fn group_skipped_updates_merges_parents_of_the_same_missed_pkg() {
    fn row(ver: &str, atom: &str, consumer: &str) -> SkippedUpdate {
        SkippedUpdate {
            category: "dev-libs".to_string(),
            package: "blk0x".to_string(),
            slot: "0".to_string(),
            skipped_version: ver.to_string(),
            skipped_sub_slot: "0".to_string(),
            skipped_repo: "testrepo".to_string(),
            skipped_use: Vec::new(),
            skipped_build_id: None,
            atom: atom.to_string(),
            consumer_cpv: consumer.to_string(),
            consumer_build_id: None,
            consumer_installed: false,
            consumer_use: Vec::new(),
            missed_targets_running_root: false,
            consumer_targets_running_root: false,
        }
    }
    let rows = vec![
        row("3", "<dev-libs/blk0x-2", "dev-libs/blk0b-1:0/0::testrepo"),
        row("3", "<dev-libs/blk0x-3", "dev-libs/blk0c-1:0/0::testrepo"),
        row("2", "<dev-libs/blk0x-2", "dev-libs/blk0b-1:0/0::testrepo"),
    ];
    let groups = group_skipped_updates(&rows);
    assert_eq!(groups.len(), 2, "same missed pkg renders one block");
    assert_eq!(
        groups[0]
            .iter()
            .map(|s| s.atom.as_str())
            .collect::<Vec<_>>(),
        vec!["<dev-libs/blk0x-2", "<dev-libs/blk0x-3"],
        "both parents stay inside, in row order",
    );
    assert_eq!(groups[1].len(), 1);
    assert_eq!(groups[1][0].skipped_version, "2");
}
