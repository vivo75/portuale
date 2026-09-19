use super::*;

#[test]
fn select_dep_target_narrows_and_ranks_like_portage() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut installed = new_entry("dev-libs", "slotted", "1.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    installed.slot = Some("0".into());
    installed.sub_slot = Some("0".into());
    let mut merged = new_entry("dev-libs", "slotted", "2.0", Vec::new());
    merged.slot = Some("0".into());
    merged.sub_slot = Some("0".into());
    let root = Path::new("/nonexistent-root-for-unit-test");
    let none = HashSet::new();
    // A version-qualified atom matches only the merge-bound 2.0.
    let entries = vec![
        installed.clone(),
        merged.clone(),
        new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge(
                ">=dev-libs/slotted-2",
                "dev-libs",
                "slotted",
                runtime,
            )],
        ),
    ];
    let pre = digraph_prelude(&entries, root);
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(1)
    );
    // A bare atom matches both, but `merge_bound_only` drops the
    // installed candidate.
    let entries = vec![
        installed.clone(),
        merged,
        new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge(
                "dev-libs/slotted",
                "dev-libs",
                "slotted",
                runtime,
            )],
        ),
    ];
    let pre = digraph_prelude(&entries, root);
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: true
        }),
        Some(1),
        "both candidates match; merge_bound_only still prefers the merged one"
    );
    // The only match is installed and `merge_bound_only` drops it.
    let entries = vec![
        installed,
        new_entry(
            "dev-libs",
            "user",
            "1.0",
            vec![plain_edge(
                "dev-libs/slotted",
                "dev-libs",
                "slotted",
                runtime,
            )],
        ),
    ];
    let pre = digraph_prelude(&entries, root);
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 1,
            from_deps: &entries[1].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: true
        }),
        None,
        "the only match is installed and merge_bound_only drops it"
    );
}

#[test]
fn select_dep_target_elimination_orders_installed_before_merge_bound() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // Installed 2.0 first: it is eliminated first, so the merge-bound
    // 1.0 survives (a version compare would keep the installed one).
    let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "2.0".into(),
    };
    let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
    );
    let entries = vec![installed, merged, user.clone()];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let pre = digraph_prelude(&entries, root);
    let none = HashSet::new();
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(1),
        "the installed candidate is eliminated first"
    );
    // Mirrored order: the `(false, true)` arm still sorts the installed
    // one first.
    let mut installed = new_entry("dev-libs", "x", "2.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "2.0".into(),
    };
    let merged = new_entry("dev-libs", "x", "1.0", Vec::new());
    let entries = vec![merged, installed, user];
    let pre = digraph_prelude(&entries, root);
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(0),
        "the merge-bound candidate survives elimination"
    );
}

#[test]
fn select_dep_target_keeps_nvc_out_of_the_elimination() {
    // An NVC candidate neither eliminates nor is eliminated.
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut nvc = new_entry("dev-libs", "x", "1.0", Vec::new());
    nvc.outcome = PretendOutcome::NoVisibleCandidate;
    let mut a = new_entry("dev-libs", "x", "1.0", Vec::new());
    a.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let mut b = new_entry("dev-libs", "x", "1.0", Vec::new());
    b.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
    );
    let entries = vec![nvc, a, b, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let pre = digraph_prelude(&entries, root);
    let none = HashSet::new();
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 3,
            from_deps: &entries[3].deps,
            ei: 0,
            suppressed: &none,
            merge_bound_only: false
        }),
        Some(0),
        "the NVC candidate stays in the ranking"
    );
}
