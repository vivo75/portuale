use super::*;

#[test]
fn resolve_blockers_matches_a_graph_resolved_package_with_no_installed_candidates() {
    let entries = vec![
        graph_entry("dev-libs", "owner", "1.0"),
        graph_entry("dev-libs", "target", "2.0"),
    ];
    let pending = vec![PendingBlocker {
        atom_str: "!!dev-libs/target".to_string(),
        strong: true,
        buildtime: true,
        target_category: "dev-libs".to_string(),
        target_package: "target".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert_eq!(
        conflicts,
        vec![FiledBlocker {
            owner: ("dev-libs".to_string(), "owner".to_string()),
            owner_version: "1.0".to_string(),
            conflict: BlockerConflict {
                atom_str: "!!dev-libs/target".to_string(),
                strong: true,
                matched_category: "dev-libs".to_string(),
                matched_package: "target".to_string(),
                matched_version: "2.0".to_string(),
                // Merge-bound match with a merging parent: unresolved outright.
                unsolvable: true,
                satisfied_by: None,
                tree_scheduled_uninstall: false,
            },
        }]
    );
}

#[test]
fn resolve_blockers_skips_a_blocker_matching_its_own_owner() {
    let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
    let pending = vec![PendingBlocker {
        atom_str: "!dev-libs/owner".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "owner".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
}

#[test]
fn resolve_blockers_drops_a_use_dep_blocker_the_target_does_not_satisfy() {
    // Unsatisfied use-dep: the block is a satisfied no-op (Portage omits the line).
    let mut target = graph_entry("dev-libs", "target", "2.0");
    target.use_flags_display = vec![("wantblock".to_string(), false)];
    let entries = vec![graph_entry("dev-libs", "owner", "1.0"), target];
    let call = |atom: &str| {
        resolve_blockers(
            Path::new("/nonexistent-root-for-this-test"),
            &[PendingBlocker {
                atom_str: atom.to_string(),
                strong: true,
                buildtime: true,
                target_category: "dev-libs".to_string(),
                target_package: "target".to_string(),
                owner_key: ("dev-libs".to_string(), "owner".to_string()),
                owner_version: "1.0".to_string(),
                owner_merging: true,
                owner_installed: false,
            }],
            &entries,
            &HashSet::new(),
            false,
        )
    };
    assert!(call("!!dev-libs/target[wantblock]").is_empty());
    // `(+)` default stands in as enabled, so `-gone` is unmet.
    assert!(call("!!dev-libs/target[-gone(+)]").is_empty());
    // A plain blocker still fires, as does a satisfied use-dep.
    assert_eq!(call("!!dev-libs/target").len(), 1);
    assert_eq!(call("!!dev-libs/target[-wantblock]").len(), 1);
}

#[test]
fn resolve_blockers_returns_nothing_when_the_target_matches_nothing() {
    let entries = vec![graph_entry("dev-libs", "owner", "1.0")];
    let pending = vec![PendingBlocker {
        atom_str: "!dev-libs/nonexistent".to_string(),
        strong: false,
        buildtime: false,
        target_category: "dev-libs".to_string(),
        target_package: "nonexistent".to_string(),
        owner_key: ("dev-libs".to_string(), "owner".to_string()),
        owner_version: "1.0".to_string(),
        owner_merging: true,
        owner_installed: false,
    }];
    let conflicts = resolve_blockers(
        Path::new("/nonexistent-root-for-this-test"),
        &pending,
        &entries,
        &HashSet::new(),
        false,
    );
    assert!(conflicts.is_empty());
}
