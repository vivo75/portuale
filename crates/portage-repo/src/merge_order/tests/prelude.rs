use super::*;

#[test]
fn select_dep_target_keeps_the_flag_carrier_a_later_flip_broke() {
    // Backlog #244 (`aub0`, argv `c a b`): B's `d[foo]` landed on
    // d-0 (foo on by default); A's autounmask `-foo` then flipped
    // d-0, so no instance satisfies `d[foo]` any more. Real keeps
    // B's edge on d-0 -- the conflict block lists B under it -- so the
    // USE narrowing falls back to the instances whose IUSE carries
    // `foo`, not to the vercmp-highest d-1 (which lacks the flag).
    let rt = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut d0 = new_entry("t", "d", "0", Vec::new());
    d0.slot = Some("0".into());
    d0.sub_slot = Some("0".into());
    d0.use_flags_display = vec![("foo".into(), false)];
    let mut d1 = new_entry("t", "d", "1", Vec::new());
    d1.slot = Some("0".into());
    d1.sub_slot = Some("0".into());
    d1.use_flags_display = vec![("bar".into(), false)];
    let b = new_entry("t", "b", "0", vec![plain_edge("t/d[foo]", "t", "d", rt)]);
    let a = new_entry("t", "a", "0", vec![plain_edge("t/d[-foo]", "t", "d", rt)]);
    let entries = vec![d0, d1, b, a];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let none = HashSet::new();
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
        "B stays on the flipped d-0"
    );
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
        "A's own atom is satisfied by d-0 as displayed"
    );
}

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
    // A version-qualified atom matches only the merge-bound 2.0, so
    // the single-match ranking path picks it.
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
    // installed candidate and still picks the merge-bound one.
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
fn select_dep_target_elimination_keeps_the_last_on_a_version_tie() {
    // Two same-version candidates: elimination drops the first and
    // keeps the last; the single-match shortcut must keep the
    // vercmp tie on the first.
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
    x1.slot = Some("0".into());
    x1.sub_slot = Some("0".into());
    let mut x2 = new_entry("dev-libs", "x", "1.0", Vec::new());
    x2.slot = Some("1".into());
    x2.sub_slot = Some("1".into());
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain_edge("dev-libs/x", "dev-libs", "x", runtime)],
    );
    let entries = vec![x1, x2, user];
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
        "two candidates take the elimination path, which keeps the last"
    );
}

#[test]
fn select_dep_target_elimination_orders_installed_before_merge_bound() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // Installed 2.0 first: it is eliminated first, the merge-bound
    // 1.0 survives -- a version compare would keep the installed one.
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
    // The mirrored order: merge-bound 1.0 first in the entry array,
    // installed 2.0 second. The `(false, true)` arm must still sort
    // the installed one first.
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
    // An NVC candidate neither eliminates nor is eliminated: it
    // stays alive and wins the ranking (its empty version ties, and
    // it is first), where the elimination would otherwise remove it
    // and leave an installed candidate.
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

#[test]
fn select_dep_target_runs_elimination_for_three_candidates() {
    // Three matching slots and two kept sibling atoms: elimination
    // removes the two candidates every sibling set also matches,
    // leaving x1; the rank-only path would pick the vercmp-highest
    // x3.
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
    x1.slot = Some("0".into());
    x1.sub_slot = Some("0".into());
    let mut x2 = new_entry("dev-libs", "x", "2.0", Vec::new());
    x2.slot = Some("1".into());
    x2.sub_slot = Some("1".into());
    let mut x3 = new_entry("dev-libs", "x", "3.0", Vec::new());
    x3.slot = Some("2".into());
    x3.sub_slot = Some("2".into());
    // Two separate `||` groups (different keys), so both branches
    // are kept: the bare atom matches all three slots, the `:0`
    // atom only x1.
    let bare = DepEdge {
        atom: "dev-libs/x".to_string(),
        evaluated: "dev-libs/x".to_string(),
        category: "dev-libs".to_string(),
        package: "x".to_string(),
        priority: runtime,
        disjunctive: true,
        alt: Some((0, 0)),
        key: 0,
    };
    let qualified = DepEdge {
        atom: "dev-libs/x:0".to_string(),
        evaluated: "dev-libs/x:0".to_string(),
        category: "dev-libs".to_string(),
        package: "x".to_string(),
        priority: runtime,
        disjunctive: true,
        alt: Some((0, 0)),
        key: 3,
    };
    let user = new_entry("dev-libs", "user", "1.0", vec![bare, qualified]);
    let entries = vec![x1, x2, x3, user];
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
        "the elimination leaves only the candidate no sibling set fully shares"
    );
}

#[test]
fn rank_best_prefers_merge_bound_then_vercmp() {
    let entries = vec![
        new_entry("dev-libs", "a", "1.0", Vec::new()),
        new_entry("dev-libs", "a", "2.0", Vec::new()),
        new_entry("dev-libs", "a", "1.5", Vec::new()),
    ];
    // A merge-bound 1.0 beats an installed 2.0.
    let installed = vec![false, true, true];
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, [0usize, 1].into_iter()),
        Some(0)
    );
    // Among installed candidates, vercmp picks the highest, not the
    // first or the string-highest.
    let installed = vec![true, true, true];
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, [0usize, 1, 2].into_iter()),
        Some(1)
    );
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, [1usize, 0].into_iter()),
        Some(1)
    );
    // A version tie keeps the first candidate.
    let tied = vec![
        new_entry("dev-libs", "b", "1.0", Vec::new()),
        new_entry("dev-libs", "b", "1.0", Vec::new()),
    ];
    assert_eq!(
        DigraphPrelude::rank_best(&tied, &[true, true], [0usize, 1].into_iter()),
        Some(0)
    );
    // One candidate, and the empty set.
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, [2usize].into_iter()),
        Some(2)
    );
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, std::iter::empty()),
        None
    );
}

#[test]
fn rank_best_prefers_a_merge_bound_candidate_over_a_newer_installed_one() {
    let entries = vec![
        new_entry("dev-libs", "a", "2.0", Vec::new()),
        new_entry("dev-libs", "a", "1.0", Vec::new()),
    ];
    let installed = vec![true, false];
    // Installed first in the iterator, merge-bound second: the
    // `(true, false)` arm picks the merge-bound one even though the
    // installed candidate's version is higher.
    assert_eq!(
        DigraphPrelude::rank_best(&entries, &installed, [0usize, 1].into_iter()),
        Some(1)
    );
}
