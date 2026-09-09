use super::*;

#[test]
fn record_slot_conflict_merges_same_triple_and_appends_new() {
    // One collision handler per conflicting slot: a second atom on the
    // same triple re-renders the record instead of appending a duplicate.
    let mk = |atom: &str, current: &str| SlotConflict {
        category: "dev-libs".to_string(),
        package: "t".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: atom.to_string(),
        instances: vec![
            SlotConflictInstance {
                version: "2.0".to_string(),
                sub_slot: "0".to_string(),
                repo_name: "r".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
            SlotConflictInstance {
                version: current.to_string(),
                sub_slot: "0".to_string(),
                repo_name: "r".to_string(),
                use_display: Vec::new(),
                parents: Vec::new(),
                installed: false,
            },
        ],
    };
    let mut v = vec![mk("atom-1", "1.0")];
    record_slot_conflict(&mut v, mk("atom-2", "1.0"));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].conflicting_atom, "atom-2");
    record_slot_conflict(&mut v, mk("atom-3", "1.5"));
    assert_eq!(v.len(), 2);
}
