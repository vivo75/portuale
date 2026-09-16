use super::*;

#[test]
fn tree_simulation_solves_a_stalled_replacement_row() {
    // The C0 p2b shape in miniature: owner O merges carrying a satisfied
    // Replacement row that tree mode stalls on, then solves via the
    // scheduled uninstall. Expected value from Portage (`TEST/findings/l0.md`).
    let plain = |atom: &str, cat: &str, pkg: &str| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: cat.to_string(),
        package: pkg.to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut owner = new_entry("dev-libs", "bparent", "1.1", Vec::new());
    owner.slot = Some("0".into());
    owner.sub_slot = Some("0".into());
    owner.blockers = vec![crate::BlockerConflict {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        matched_category: "dev-libs".to_string(),
        matched_package: "blocked".to_string(),
        matched_version: "1.0".to_string(),
        unsolvable: false,
        satisfied_by: Some(crate::BlockerSatisfiedBy::Replacement {
            cp: ("dev-libs".to_string(), "blocked".to_string()),
            slot: "0".to_string(),
        }),
        tree_scheduled_uninstall: false,
    }];
    let mut replacement = new_entry(
        "dev-libs",
        "blocked",
        "2.0",
        vec![plain("dev-libs/bparent", "dev-libs", "bparent")],
    );
    replacement.slot = Some("0".into());
    replacement.sub_slot = Some("0".into());
    // A third, unrelated root merge: without it the replacement would be
    // the lone leaf of round two and the len==1 shortcut would take it
    // immediately (no stall, no row -- same as Portage).
    let other = new_entry("dev-libs", "other", "1.0", Vec::new());
    let entries = vec![owner, replacement, other];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let config = portage_profile::Config::default();
    let solved = tree_solved_replacements(TreeSolvedReplacementsArgs {
        entries: &entries,
        top_level_atoms: &["dev-libs/blocked".to_string()],
        config: &config,
        root,
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    assert_eq!(
        solved,
        vec![(0, 0)],
        "the stalled Replacement row is solved in tree mode: {solved:?}"
    );
}

#[test]
fn tree_solved_replacements_skips_a_same_version_row() {
    // A Replacement row whose merge-bound entry resolves at the same
    // version as the installed one is not a pending row.
    let mut owner = new_entry("dev-libs", "bparent", "1.1", Vec::new());
    owner.slot = Some("0".into());
    owner.sub_slot = Some("0".into());
    owner.blockers = vec![crate::BlockerConflict {
        atom_str: "!<dev-libs/blocked-2.0".to_string(),
        strong: false,
        matched_category: "dev-libs".to_string(),
        matched_package: "blocked".to_string(),
        matched_version: "1.0".to_string(),
        unsolvable: false,
        satisfied_by: Some(crate::BlockerSatisfiedBy::Replacement {
            cp: ("dev-libs".to_string(), "blocked".to_string()),
            slot: "0".to_string(),
        }),
        tree_scheduled_uninstall: false,
    }];
    let mut replacement = new_entry(
        "dev-libs",
        "blocked",
        "1.0",
        vec![runtime_edge("dev-libs/bparent", "dev-libs", "bparent")],
    );
    replacement.slot = Some("0".into());
    replacement.sub_slot = Some("0".into());
    let other = new_entry("dev-libs", "other", "1.0", Vec::new());
    let entries = vec![owner, replacement, other];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let config = portage_profile::Config::default();
    let solved = tree_solved_replacements(TreeSolvedReplacementsArgs {
        entries: &entries,
        top_level_atoms: &["dev-libs/blocked".to_string()],
        config: &config,
        root,
        implicit_system_deps: true,
        repos: &[],
        dynamic_deps: true,
        circular: &HashMap::new(),
    });
    assert!(
        solved.is_empty(),
        "the same-version entry is not a replacement row: {solved:?}"
    );
}
