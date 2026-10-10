use super::*;

/// Backlog #129 (review minor 3): the two `SkippedUpdate` sources
/// (backtrack-mask rows, direct-solve rows) collapse per slot
/// keeping the highest missed pkg, like real `_get_missed_updates`
/// (`lib/_emerge/depgraph.py:1553-1562`) -- a consecutive `dedup()`
/// could not do this. A slot present in both sources renders once,
/// at the higher version, whichever source holds it; equal
/// versions all stay (one row per parent) and exact duplicates
/// collapse to one, with mask rows leading.
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
            atom: atom.to_string(),
            consumer_cpv: "dev-libs/consumer-1:0/0::testrepo".to_string(),
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

/// Backlog #292: a higher pkg replacing a lower one of its slot keeps
/// the slot's first position (real `_get_missed_updates` dict key
/// order, `depgraph.py:1544-1560`): slot X's lower row, block Y, then
/// X's higher row render X, Y -- not Y, X.
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
            atom: format!("={pkg}-{ver}"),
            consumer_cpv: "dev-libs/consumer-1:0/0::testrepo".to_string(),
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

/// Backlog #129 (review round 2): rows naming the same missed pkg
/// render as one block with every parent line inside (real
/// `_show_missed_update_slot_conflicts`,
/// `lib/_emerge/depgraph.py:1650+`, over the single per-`(root,
/// slot_atom)` entry, `:1553-1562`) -- the `blk0b blk0c blk0a
/// --backtrack=0` oracle shape (bed `l0-fx-20260927T125711Z`: one
/// `dev-libs/blk0x:0` block for `-3` with both parents). Groups
/// keep first-seen order, parents keep row order; a different
/// missed version stays its own block.
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
            atom: atom.to_string(),
            consumer_cpv: consumer.to_string(),
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
