use super::*;

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
