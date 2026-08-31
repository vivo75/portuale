use super::*;

/// Settle-time display continuity: a `slot_operator_rebuild` reinstall re-derives
/// its pair from the built atom and the merged provider, so the block renders
/// even though the firing pass restarted.
#[test]
fn slot_conflict_abi_display_pairs_follow_the_rebuild() {
    let (base, _repos) = abi_probe_harness(true);
    let reinstall = |slot_operator_rebuild: bool| GraphEntry {
        outcome: PretendOutcome::Reinstall {
            version: "1".to_string(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-misc", "abicons", "1")
    };
    let provider = GraphEntry {
        outcome: PretendOutcome::New {
            version: "2".to_string(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        // Only a child of the replacement pairs (Portage).
        required_by: vec![("app-misc".to_string(), "abicons".to_string())],
        ..graph_entry("app-misc", "abiprov", "2")
    };
    let set: BTreeSet<(String, String)> =
        BTreeSet::from([("app-misc".to_string(), "abicons".to_string())]);
    // Rebuilt consumer re-derives its pair.
    assert_eq!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), provider.clone()], &set, true),
        vec![(
            "app-misc/abiprov-2".to_string(),
            "app-misc/abicons-1".to_string()
        )]
    );
    // Provider no longer pulled (USE conditional now off): no pair.
    let unpulled = GraphEntry {
        required_by: Vec::new(),
        ..provider.clone()
    };
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), unpulled], &set, true).is_empty(),
        "a provider the rebuilt consumer no longer pulls is not paired"
    );
    // Not a slot-operator rebuild: no pair.
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(false), provider.clone()], &set, true)
            .is_empty(),
        "an ordinary reinstall renders no ABI pair"
    );
    // Consumer not in the replace set: no pair.
    assert!(
        slot_conflict_abi_display_pairs(
            &base,
            &[reinstall(true), provider.clone()],
            &BTreeSet::new(),
            true
        )
        .is_empty(),
        "only replace-set rebuilds explain themselves"
    );
    // Provider at the bound sub-slot: no shift, no pair.
    let same_sub = GraphEntry {
        sub_slot: Some("1".into()),
        ..provider.clone()
    };
    assert!(
        slot_conflict_abi_display_pairs(&base, &[reinstall(true), same_sub], &set, true).is_empty(),
        "an unshifted provider is not an ABI cause"
    );
    let _ = fs::remove_dir_all(&base);
}
