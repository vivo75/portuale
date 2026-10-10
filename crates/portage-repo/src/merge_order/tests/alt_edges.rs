use super::*;

#[test]
fn resolved_dep_targets_ranks_by_vercmp_not_by_string_order() {
    // #82: `print_tree` carried a private copy of this narrowing
    // whose tie-break was `jv.cmp(bv)`, a **string** compare, under
    // which `1.9` outranks `1.10`. Real resolves a slot-unqualified
    // atom through `_select_pkg_highest_available`, i.e. the
    // vercmp-highest of the matching graph nodes -- the container
    // oracle for the shape is `TEST/findings/l0.md` "#82"
    // (`dev-libs/treeslotuser`).
    let plain = |cat: &str, pkg: &str| DepEdge {
        atom: format!("{cat}/{pkg}"),
        evaluated: format!("{cat}/{pkg}").clone(),
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
    let mut low = new_entry("dev-libs", "slotted", "1.9", Vec::new());
    low.slot = Some("0".into());
    low.sub_slot = Some("0".into());
    let mut high = new_entry("dev-libs", "slotted", "1.10", Vec::new());
    high.slot = Some("1".into());
    high.sub_slot = Some("1".into());
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain("dev-libs", "slotted")],
    );
    // `low` first in the array, so "first on ties" would pick it if
    // the ranking were not comparing versions at all.
    let entries = vec![low, high, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let targets = resolved_dep_targets(&entries, root, &HashMap::new());
    assert_eq!(
        targets[2][0],
        Some(1),
        "the unqualified atom resolves to 1.10, not to the \
         string-highest 1.9"
    );
    // `build_digraph` shares the ranking, so its edge agrees.
    let g = build_digraph(
        &entries,
        &["dev-libs/user".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
    assert_eq!(
        children,
        vec![1],
        "the scheduling graph picks the same instance: {children:?}"
    );
}

#[test]
fn unqualified_atom_collapses_onto_the_slot_qualified_sibling_target() {
    // #85: real's `_minimize_children` (`depgraph.py:4751-4856`).
    // `user` pulls slot `:0` (selects 1.9) and the bare cp (selects
    // 1.10, the vercmp-highest). Real eliminates the redundant 1.10
    // selection -- the `:0` atom matches only 1.9 while the bare atom
    // matches both -- so the bare atom resolves to 1.9 and the
    // scheduler graph holds no `user → 1.10` edge (real's own
    // `--debug` digraph dump for `dev-libs/treeslotuser` shows only
    // the 1.9 and parent edges). Without the elimination the extra
    // edge doubles 1.10's parent count and `_merge_order_bias` flips
    // the pair to `1.10, 1.9`.
    let plain = |atom: &str| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: "dev-libs".to_string(),
        package: "slotted".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut low = new_entry("dev-libs", "slotted", "1.9", Vec::new());
    low.slot = Some("0".into());
    low.sub_slot = Some("0".into());
    let mut high = new_entry("dev-libs", "slotted", "1.10", Vec::new());
    high.slot = Some("1".into());
    high.sub_slot = Some("1".into());
    let user = new_entry(
        "dev-libs",
        "user",
        "1.0",
        vec![plain("dev-libs/slotted:0"), plain("dev-libs/slotted")],
    );
    let entries = vec![low, high, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let targets = resolved_dep_targets(&entries, root, &HashMap::new());
    assert_eq!(
        targets[2],
        vec![Some(0), Some(0)],
        "both atoms resolve to 1.9: the bare atom's 1.10 selection is redundant"
    );
    let g = build_digraph(
        &entries,
        &["dev-libs/user".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[2].iter().map(|&(c, _)| c).collect();
    assert_eq!(
        children,
        vec![0],
        "one edge, to 1.9 -- no extra parent for 1.10: {children:?}"
    );
}

#[test]
fn resolved_dep_targets_reports_an_installed_target_as_no_tree_edge() {
    // #82: the single `merge_bound_only` difference between the two
    // callers. `build_digraph` edges to an installed (nomerge) node
    // the way real's digraph does; `print_tree` must not, because
    // portuale renders no line for one and the edge would open a
    // hole in the tree.
    let dep = DepEdge {
        atom: "dev-libs/onlyinstalled".to_string(),
        evaluated: "dev-libs/onlyinstalled".to_string().clone(),
        category: "dev-libs".to_string(),
        package: "onlyinstalled".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut installed = new_entry("dev-libs", "onlyinstalled", "1.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![new_entry("dev-libs", "user", "1.0", vec![dep]), installed];
    let root = Path::new("/nonexistent-root-for-unit-test");
    assert_eq!(
        resolved_dep_targets(&entries, root, &HashMap::new())[0][0],
        None
    );
    let g = build_digraph(
        &entries,
        &["dev-libs/user".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[0].iter().map(|&(c, _)| c).collect();
    assert_eq!(children, vec![1], "the scheduling graph keeps the edge");
}

#[test]
fn kept_alt_branches_agrees_with_build_digraph_on_a_multi_branch_group() {
    // #76 B1: real `dep_zapdeps`' all-in-graph bin outranks
    // all-installed, so the kept branch is the merge-bound one; the
    // public helper must report exactly that (B0 q2), and
    // `build_digraph` -- which shares `suppressed_alt_edges` -- must
    // record an edge only to it.
    let owner_deps = vec![
        DepEdge {
            atom: "~dev-libs/newdep-2".to_string(),
            evaluated: "~dev-libs/newdep-2".to_string().clone(),
            category: "dev-libs".to_string(),
            package: "newdep".to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 0)),
            key: 3,
        },
        DepEdge {
            atom: "dev-libs/olddep".to_string(),
            evaluated: "dev-libs/olddep".to_string().clone(),
            category: "dev-libs".to_string(),
            package: "olddep".to_string(),
            priority: DepPriority {
                runtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 1)),
            key: 3,
        },
    ];
    let mut olddep = new_entry("dev-libs", "olddep", "1.0", Vec::new());
    olddep.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![
        new_entry("app-misc", "owner", "1.0", owner_deps),
        new_entry("dev-libs", "newdep", "2", Vec::new()),
        olddep,
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let kept = kept_alt_branches(&entries, root, &HashMap::new());
    assert_eq!(
        kept[0],
        HashSet::from([0usize]),
        "the in-graph branch is kept over the installed one"
    );
    let g = build_digraph(
        &entries,
        &["app-misc/owner".to_string()],
        root,
        &HashMap::new(),
    );
    let children: Vec<usize> = g.children[0].iter().map(|&(c, _)| c).collect();
    assert!(
        children.contains(&1),
        "an edge to the kept branch must exist: {children:?}"
    );
    assert!(
        !children.contains(&2),
        "no edge to the suppressed branch: {children:?}"
    );
    // The #53 self-branch exclusion the helper shares.
    let mogo_deps = vec![
        DepEdge {
            atom: ">=dev-lang/mogo-1.24".to_string(),
            evaluated: ">=dev-lang/mogo-1.24".to_string().clone(),
            category: "dev-lang".to_string(),
            package: "mogo".to_string(),
            priority: DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 0)),
            key: 4,
        },
        DepEdge {
            atom: ">=dev-lang/mogoboot-1.24".to_string(),
            evaluated: ">=dev-lang/mogoboot-1.24".to_string().clone(),
            category: "dev-lang".to_string(),
            package: "mogoboot".to_string(),
            priority: DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
            disjunctive: true,
            alt: Some((0, 1)),
            key: 4,
        },
    ];
    let entries = vec![
        new_entry("dev-lang", "mogo", "1.26", mogo_deps),
        new_entry("dev-lang", "mogoboot", "1.24", Vec::new()),
    ];
    let kept = kept_alt_branches(&entries, root, &HashMap::new());
    assert_eq!(
        kept[0],
        HashSet::from([1usize]),
        "the circular self branch is not kept (#53)"
    );
}

#[test]
fn select_dep_target_eliminates_with_suppressed_sibling_sets() {
    // A suppressed `||` sibling must not join the minimize universe:
    // with the sibling's set {x1} counted, the shared-by-all test
    // keeps x1 and drops x2, flipping the pick away from the
    // vercmp-highest merge-bound target.
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
    let bare = DepEdge {
        atom: "dev-libs/x".to_string(),
        evaluated: "dev-libs/x".to_string(),
        category: "dev-libs".to_string(),
        package: "x".to_string(),
        priority: runtime,
        disjunctive: true,
        alt: Some((0, 0)),
        key: 3,
    };
    let qualified = DepEdge {
        atom: "dev-libs/x:0".to_string(),
        evaluated: "dev-libs/x:0".to_string(),
        category: "dev-libs".to_string(),
        package: "x".to_string(),
        priority: runtime,
        disjunctive: true,
        alt: Some((0, 1)),
        key: 3,
    };
    let user = new_entry("dev-libs", "user", "1.0", vec![bare, qualified]);
    let entries = vec![x1, x2, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let pre = digraph_prelude(&entries, root);
    let suppressed = suppressed_alt_edges(&entries, &pre, root, &HashMap::new());
    assert_eq!(
        suppressed[2],
        HashSet::from([1usize]),
        "the qualified branch is the suppressed one"
    );
    assert_eq!(
        pre.select_dep_target(SelectDepTargetArgs {
            entries: &entries,
            from: 2,
            from_deps: &entries[2].deps,
            ei: 0,
            suppressed: &suppressed[2],
            merge_bound_only: false
        }),
        Some(1),
        "the suppressed sibling's set must not enter the elimination"
    );
}

// -----------------------------------------------------------------
// #144 follow-up: the survivors of the first cluster run
// -----------------------------------------------------------------

#[test]
fn suppressed_alt_edges_requires_every_atom_of_a_branch_to_match() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let alt_edge = |atom: &str, cat: &str, pkg: &str, branch: u32| {
        let mut e = plain_edge(atom, cat, pkg, runtime);
        e.disjunctive = true;
        e.alt = Some((0, branch));
        e.key = 3;
        e
    };
    // Branch 0 is only *partly* installed (one installed atom, one
    // merge-bound); branch 1 is fully installed. The all-installed
    // bin must pick branch 1.
    let mut i1 = new_entry("dev-libs", "i1", "1.0", Vec::new());
    i1.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let mut i2 = new_entry("dev-libs", "i2", "1.0", Vec::new());
    i2.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let mut i3 = new_entry("dev-libs", "i3", "1.0", Vec::new());
    i3.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let owner = new_entry(
        "app-misc",
        "owner",
        "1.0",
        vec![
            alt_edge("dev-libs/i1", "dev-libs", "i1", 0),
            alt_edge("dev-libs/m1", "dev-libs", "m1", 0),
            alt_edge("dev-libs/i2", "dev-libs", "i2", 1),
            alt_edge("dev-libs/i3", "dev-libs", "i3", 1),
        ],
    );
    let entries = vec![
        owner,
        i1,
        new_entry("dev-libs", "m1", "1.0", Vec::new()),
        i2,
        i3,
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    assert_eq!(
        kept_alt_branches(&entries, root, &HashMap::new())[0],
        HashSet::from([2usize, 3]),
        "only a branch whose every atom matches installed wins the all-installed bin"
    );
    // A branch whose second atom matches nothing at all is not an
    // `all_any` candidate; with no other branch resolving, nothing
    // is suppressed.
    let owner = new_entry(
        "app-misc",
        "owner",
        "1.0",
        vec![
            alt_edge("dev-libs/i1", "dev-libs", "i1", 0),
            alt_edge("dev-libs/absent", "dev-libs", "absent", 0),
            alt_edge("dev-libs/absent2", "dev-libs", "absent2", 1),
        ],
    );
    let mut i1 = new_entry("dev-libs", "i1", "1.0", Vec::new());
    i1.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    let entries = vec![owner, i1];
    assert_eq!(
        kept_alt_branches(&entries, root, &HashMap::new())[0],
        HashSet::from([0usize, 1, 2]),
        "no branch fully resolves, so every branch is kept"
    );
}
