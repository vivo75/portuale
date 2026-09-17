use super::*;

#[test]
fn replacement_satisfied_blocker_rows_are_hidden_or_inline() {
    // A `Replacement` row prints inline after the replacement it waits on and
    // counts only then; otherwise it hides and is not counted.
    let nc = Colorizer::new(false);
    let replacement_owner = |entries: &[GraphEntry]| {
        let mut owner = entry_with_use(
            PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "1.1".into(),
            },
            "",
            "",
        );
        owner.package = "bparent".into();
        owner.blockers = vec![BlockerConflict {
            atom_str: "!<dev-libs/blocked-2.0".into(),
            strong: false,
            matched_category: "dev-libs".into(),
            matched_package: "blocked".into(),
            matched_version: "1.0".into(),
            unsolvable: false,
            satisfied_by: Some(BlockerSatisfiedBy::Replacement {
                cp: ("dev-libs".into(), "blocked".into()),
                slot: "0".into(),
            }),
            tree_scheduled_uninstall: false,
        }];
        let _ = entries;
        owner
    };
    let replacement = || {
        let mut e = entry_with_use(
            PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "2.0".into(),
            },
            "",
            "",
        );
        e.package = "blocked".into();
        e.deps = vec![portage_repo::DepEdge {
            atom: "~dev-libs/bparent-1.1".into(),
            evaluated: "~dev-libs/bparent-1.1".to_string(),
            category: "dev-libs".into(),
            package: "bparent".into(),
            priority: portage_repo::DepPriority {
                buildtime: true,
                ..portage_repo::DepPriority::default()
            },
            disjunctive: false,
            alt: None,
            key: 4,
        }];
        e
    };

    let mut entry = entry_with_use(
        PretendOutcome::New {
            version: "2.0".into(),
        },
        "",
        "",
    );
    entry.blockers = vec![
        replacement_owner(&[]).blockers[0].clone(),
        BlockerConflict {
            atom_str: "!dev-libs/other".into(),
            strong: false,
            matched_category: "dev-libs".into(),
            matched_package: "other".into(),
            matched_version: "1.0".into(),
            unsolvable: false,
            satisfied_by: Some(BlockerSatisfiedBy::Uninstall {
                cpv: "dev-libs/other-1.0".into(),
                anchor: ("dev-libs".into(), "owner".into()),
            }),
            tree_scheduled_uninstall: false,
        },
    ];
    let entries = [entry];
    let lines = trailing_blocker_lines(TrailingBlockerLinesArgs {
        entries: &entries,
        root: Path::new("/nonexistent"),
        owner_index: 0,
        include_mask: true,
        columns: false,
        color: &nc,
        circular: &HashMap::new(),
    });
    assert_eq!(lines.len(), 1, "only the uninstall-satisfied row trails");
    assert!(
        lines[0].starts_with("[blocks b      ] dev-libs/other ("),
        "the trailing row is the Uninstall one: {lines:?}"
    );
    assert!(
        collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
            entries: &entries,
            root: Path::new("/nonexistent"),
            quiet: false,
            columns: false,
            color: &nc,
            circular: &HashMap::new()
        })
        .is_empty()
    );
    let summary = package_counters_summary(PackageCountersSummaryArgs {
        entries: &entries,
        root: Path::new("/nonexistent"),
        top_level_pkgs: &HashSet::new(),
        onlydeps: false,
        color: &nc,
        orphans: &[],
        tree: false,
        circular: &HashMap::new(),
    });
    assert!(
        summary.contains("Conflict: 1 block (all satisfied)"),
        "the hidden row is not counted: {summary}"
    );

    let entries = [
        replacement_owner(&[]),
        entry_with_use(
            PretendOutcome::New {
                version: "1.0".into(),
            },
            "",
            "",
        ),
        replacement(),
    ];
    let inline = collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
        entries: &entries,
        root: Path::new("/nonexistent"),
        quiet: false,
        columns: false,
        color: &nc,
        circular: &HashMap::new(),
    });
    assert_eq!(inline.len(), 1, "the wait prints one inline row");
    assert_eq!(inline[0].0, 2, "after the replacement entry");
    assert!(
        inline[0]
            .1
            .starts_with("[blocks b      ] <dev-libs/blocked-2.0 ("),
        "{inline:?}"
    );
    assert_eq!(
        count_blocker_rows(&entries, Path::new("/nonexistent"), false, &HashMap::new()),
        (1, 0)
    );

    // `--columns` suppresses the line but not the count.
    let entries = [replacement_owner(&[]), replacement()];
    assert!(
        collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
            entries: &entries,
            root: Path::new("/nonexistent"),
            quiet: false,
            columns: true,
            color: &nc,
            circular: &HashMap::new()
        })
        .is_empty()
    );
    assert_eq!(
        count_blocker_rows(&entries, Path::new("/nonexistent"), false, &HashMap::new()),
        (1, 0)
    );
}

#[test]
fn replacement_wait_follows_the_selected_disjunctive_branch() {
    // Portage: dep_check.py, dep_zapdeps. A wait through the kept `||`
    // branch prints; a suppressed branch is not followed.
    let nc = Colorizer::new(false);
    let owner = || {
        let mut e = entry_with_use(
            PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "1.1".into(),
            },
            "",
            "",
        );
        e.package = "bparent".into();
        e.blockers = vec![BlockerConflict {
            atom_str: "!<dev-libs/blocked-2.0".into(),
            strong: false,
            matched_category: "dev-libs".into(),
            matched_package: "blocked".into(),
            matched_version: "1.0".into(),
            unsolvable: false,
            satisfied_by: Some(BlockerSatisfiedBy::Replacement {
                cp: ("dev-libs".into(), "blocked".into()),
                slot: "0".into(),
            }),
            tree_scheduled_uninstall: false,
        }];
        e
    };
    let replacement = |deps: Vec<portage_repo::DepEdge>| {
        let mut e = entry_with_use(
            PretendOutcome::Upgrade {
                from: "1.0".into(),
                to: "2.0".into(),
            },
            "",
            "",
        );
        e.package = "blocked".into();
        e.deps = deps;
        e
    };
    let alt_edge = |pkg: &str, version: &str, branch: u32| portage_repo::DepEdge {
        atom: format!("~dev-libs/{pkg}-{version}"),
        evaluated: format!("~dev-libs/{pkg}-{version}").clone(),
        category: "dev-libs".into(),
        package: pkg.into(),
        priority: portage_repo::DepPriority {
            runtime: true,
            ..portage_repo::DepPriority::default()
        },
        disjunctive: true,
        alt: Some((0, branch)),
        key: 4,
    };

    let entries = [
        owner(),
        replacement(vec![
            alt_edge("bparent", "1.1", 0),
            alt_edge("absent", "1.0", 1),
        ]),
    ];
    let inline = collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
        entries: &entries,
        root: Path::new("/nonexistent"),
        quiet: false,
        columns: false,
        color: &nc,
        circular: &HashMap::new(),
    });
    assert_eq!(inline.len(), 1, "the kept disjunctive wait prints a row");
    assert_eq!(inline[0].0, 1, "after the replacement entry");

    let mut other = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    other.package = "other".into();
    let entries = [
        owner(),
        replacement(vec![
            alt_edge("other", "1.0", 0),
            alt_edge("bparent", "1.1", 1),
        ]),
        other,
    ];
    assert!(
        collect_inline_blocker_lines(CollectInlineBlockerLinesArgs {
            entries: &entries,
            root: Path::new("/nonexistent"),
            quiet: false,
            columns: false,
            color: &nc,
            circular: &HashMap::new()
        })
        .is_empty(),
        "a suppressed wait branch is not followed"
    );
}

#[test]
fn kept_alt_for_display_is_derived_once_and_shared() {
    // The shared kept set must equal the per-call derivation it replaces,
    // and stays empty when no `Replacement` row can consult it.
    let root = Path::new("/nonexistent");
    let plain = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    assert!(
        kept_alt_for_display(&[plain], root, 0..1, &HashMap::new()).is_empty(),
        "no Replacement row means no derivation"
    );

    let mut owner = entry_with_use(
        PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "1.1".into(),
        },
        "",
        "",
    );
    owner.package = "bparent".into();
    let blocker = BlockerConflict {
        atom_str: "!<dev-libs/blocked-2.0".into(),
        strong: false,
        matched_category: "dev-libs".into(),
        matched_package: "blocked".into(),
        matched_version: "1.0".into(),
        unsolvable: false,
        satisfied_by: Some(BlockerSatisfiedBy::Replacement {
            cp: ("dev-libs".into(), "blocked".into()),
            slot: "0".into(),
        }),
        tree_scheduled_uninstall: false,
    };
    owner.blockers = vec![blocker.clone()];
    let mut replacement = entry_with_use(
        PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        "",
        "",
    );
    replacement.package = "blocked".into();
    replacement.deps = vec![portage_repo::DepEdge {
        atom: "~dev-libs/bparent-1.1".to_string(),
        evaluated: "~dev-libs/bparent-1.1".to_string().clone(),
        category: "dev-libs".into(),
        package: "bparent".into(),
        priority: portage_repo::DepPriority {
            runtime: true,
            ..portage_repo::DepPriority::default()
        },
        disjunctive: true,
        alt: Some((0, 0)),
        key: 4,
    }];
    let entries = [owner, replacement];
    let shared = kept_alt_for_display(&entries, root, 0..entries.len(), &HashMap::new());
    assert_eq!(
        kept_alt_for_display(&entries, root, [0], &HashMap::new()),
        shared
    );
    assert!(kept_alt_for_display(&entries, root, [1], &HashMap::new()).is_empty());
    let on_demand = portage_repo::kept_alt_branches(&entries, root, &HashMap::new());
    assert_eq!(
        shared, on_demand,
        "the shared set is the derivation it replaces"
    );
    assert_eq!(
        replacement_wait_index(&entries, root, 0, &blocker, &shared),
        Some(1)
    );
    assert_eq!(
        replacement_wait_index(&entries, root, 0, &blocker, &[]),
        None
    );
}
