use super::*;

#[test]
fn tree_simulation_solves_a_stalled_replacement_row() {
    // Backlog #81 S1: the C0 p2b shape in miniature. Owner O merges
    // (1.0 -> 1.1) carrying a satisfied Replacement row for the
    // installed 1.0 that R=blocked-2.0 replaces in-slot. Both nodes
    // are roots, so tree mode's one-at-a-time selection stalls on
    // round one with the blocker pending, schedules the uninstall,
    // and solves the row when the uninstall is selected -- while the
    // flat greedy pop would drain both without stalling. (Real's
    // verdict for the full shape is in `TEST/findings/l0.md` "#75
    // C0"; the flat-hidden half is `replacement_wait_index`'s call.)
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
    // A third, unrelated root merge: without it the replacement
    // would be the lone leaf of round two and real's len==1 shortcut
    // would take it immediately (no stall, no row -- same as real).
    // With two roots left, tree mode's one-at-a-time scan skips both
    // and the stuck branch fires while the blocker is pending.
    let other = new_entry("dev-libs", "other", "1.0", Vec::new());
    let entries = vec![owner, replacement, other];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let config = portage_profile::Config::default();
    let solved = tree_solved_replacements(
        &entries,
        &["dev-libs/blocked".to_string()],
        &config,
        root,
        true,
        &[],
        true,
        &HashMap::new(),
    );
    assert_eq!(
        solved,
        vec![(0, 0)],
        "the stalled Replacement row is solved in tree mode: {solved:?}"
    );
}

#[test]
fn tree_schedule_stuck_pins_guards_and_node_bookkeeping() {
    let mut walked = new_entry("dev-libs", "walkedinst", "1.0", Vec::new());
    walked.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    walked.slot = Some("0".into());
    let mut otherinst = new_entry("dev-libs", "otherinst", "1.0", Vec::new());
    otherinst.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    otherinst.slot = Some("0".into());
    let entries = vec![
        new_entry("dev-libs", "bparent", "1.1", Vec::new()),
        new_entry("dev-libs", "other", "2.0", Vec::new()),
        walked,
        otherinst,
    ];
    let mut g = test_graph(2, &[]);
    let row = |repl: usize, cp: (&str, &str), strong: bool| PendingUninstall {
        repl,
        inst_cp: (cp.0.to_string(), cp.1.to_string()),
        inst_slot: "0".to_string(),
        inst_version: "1.0".to_string(),
        owner: 1,
        blocker: 0,
        strong,
        node: None,
    };
    let mut ts = TreeStuck {
        pending: vec![
            // Walked installed instance: skipped.
            row(2, ("dev-libs", "walkedinst"), false),
            // Strong row: hidden.
            row(0, ("dev-libs", "blocked"), true),
            // Eligible row: schedules.
            row(0, ("dev-libs", "blocked"), false),
        ],
        scheduled: HashSet::new(),
        solved: Vec::new(),
        progressed: false,
    };
    tree_schedule_stuck(&mut g, &entries, &mut ts);
    assert!(
        ts.pending[0].node.is_none(),
        "a walked installed instance is skipped"
    );
    assert!(ts.pending[1].node.is_none(), "a strong row stays hidden");
    let node = ts.pending[2].node.expect("the soft eligible row schedules");
    assert_eq!(g.n, 3);
    // #131: the synthetic node is the replacement's parent (real's
    // edge reversal), not its child -- the replacement is picked
    // single the next round instead of rejoining a later batch.
    assert!(
        g.parents[node].is_empty(),
        "the synthetic node has no parents"
    );
    assert!(
        g.children[node].iter().any(|(c, _)| *c == 0),
        "the synthetic node carries the replacement as its child"
    );
    assert!(
        g.parents[0].contains(&node),
        "the replacement waits on the synthetic node"
    );
    assert!(g.installed[node]);
    assert!(ts.scheduled.contains(&node));
    assert!(ts.progressed);
    // A second call does not reschedule the same row.
    ts.progressed = false;
    tree_schedule_stuck(&mut g, &entries, &mut ts);
    assert_eq!(g.n, 3);
    assert!(!ts.progressed);
}

#[test]
fn tree_note_selection_pins_the_verdict_lifecycle() {
    let entries = vec![
        new_entry("dev-libs", "repl", "1.0", Vec::new()),
        new_entry("dev-libs", "other", "1.0", Vec::new()),
    ];
    let pending = |node: Option<usize>, scheduled: bool| {
        let mut ts = TreeStuck {
            pending: vec![PendingUninstall {
                repl: 0,
                inst_cp: ("dev-libs".to_string(), "inst".to_string()),
                inst_slot: "0".to_string(),
                inst_version: "1.0".to_string(),
                owner: 1,
                blocker: 0,
                strong: false,
                node,
            }],
            scheduled: HashSet::new(),
            solved: Vec::new(),
            progressed: false,
        };
        if scheduled {
            ts.scheduled.insert(node.unwrap());
        }
        ts
    };
    // Selecting the synthetic uninstall records the verdict once.
    let mut g = test_graph(2, &[]);
    let mut ts = pending(Some(1), true);
    tree_note_selection(&mut g, &entries, &mut ts, 1);
    assert_eq!(ts.solved, vec![(1, 0)]);
    assert!(g.alive[1], "the uninstall selection leaves the node alive");
    tree_note_selection(&mut g, &entries, &mut ts, 1);
    assert_eq!(ts.solved, vec![(1, 0)], "the verdict is not duplicated");
    // Selecting the replacement kills the synthetic node and solves.
    let mut g = test_graph(2, &[]);
    let mut ts = pending(Some(1), true);
    tree_note_selection(&mut g, &entries, &mut ts, 0);
    assert!(
        !g.alive[1],
        "the replacement's merge removes the uninstall node"
    );
    assert_eq!(ts.solved, vec![(1, 0)]);
    // An unscheduled pending node is not touched.
    let mut g = test_graph(2, &[]);
    let mut ts = pending(Some(1), false);
    tree_note_selection(&mut g, &entries, &mut ts, 1);
    assert!(ts.solved.is_empty(), "an unscheduled row records nothing");
}

#[test]
fn tree_solved_replacements_skips_a_same_version_row() {
    // A Replacement row whose merge-bound entry resolves at the same
    // version as the matched installed one is not a pending row.
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
    let solved = tree_solved_replacements(
        &entries,
        &["dev-libs/blocked".to_string()],
        &config,
        root,
        true,
        &[],
        true,
        &HashMap::new(),
    );
    assert!(
        solved.is_empty(),
        "the same-version entry is not a replacement row: {solved:?}"
    );
}
