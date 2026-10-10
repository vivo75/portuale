use super::*;

#[test]
fn self_referential_disjunctive_bdepend_skips_the_self_branch() {
    // #53: `dev-lang/mogo`'s own BDEPEND is
    // `|| ( >=mogo-1.24 >=mogoboot-1.24 )`. The resolver picks the
    // non-circular `mogoboot` branch (L0 finding D, `b5b256f`) --
    // `alt_suppressed` must agree, not let the self branch win via
    // its trivial "matches the owner's own not-yet-merged graph
    // node" match, which used to add a permanent `mogo -> mogo`
    // buildtime self-edge and demote the real edge to `mogoboot`
    // down to the weaker `required_by`-fallback `runtime` priority.
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
            evaluated: ">=dev-lang/mogoboot-1.24".to_string(),
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
    let g = build_digraph(
        &entries,
        &["dev-lang/mogo".to_string()],
        Path::new("/nonexistent-root-for-unit-test"),
        &HashMap::new(),
    );
    assert!(
        g.children[0].iter().all(|&(c, _)| c != 0),
        "mogo must not depend on itself: {:?}",
        g.children[0]
    );
    let to_boot = g.children[0].iter().find(|&&(c, _)| c == 1);
    assert!(
        to_boot.is_some(),
        "mogo must depend on mogoboot: {:?}",
        g.children[0]
    );
    let (_, prios) = to_boot.unwrap();
    assert!(
        prios.iter().any(|p| p.buildtime),
        "the mogo -> mogoboot edge must be the real buildtime one, \
         not the weaker required_by-fallback edge: {prios:?}"
    );
}

#[test]
fn nvc_entries_do_not_participate_in_minimize_elimination() {
    // #85 follow-up (`opartlya` disclosure order): a
    // `NoVisibleCandidate` entry is never a *selected* package --
    // real's `_select_package` returns None for its atom, so it
    // never enters `_minimize_children`'s elimination. Here the
    // bare atom matches both the NVC entry and the installed 1.0;
    // without the NVC exclusion the ascending-version elimination
    // drops the NVC entry (its empty version sorts first) and the
    // edge moves to the installed entry, flipping the merge order
    // of the disclosure rows. With it, the pre-#85 ranking stands
    // and the NVC entry keeps the edge.
    let plain = |atom: &str| DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: "dev-libs".to_string(),
        package: "partly".to_string(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 3,
    };
    let mut nvc = new_entry("dev-libs", "partly", "1.0", Vec::new());
    nvc.outcome = PretendOutcome::NoVisibleCandidate;
    nvc.slot = Some("0".into());
    nvc.sub_slot = Some("0".into());
    let mut installed = new_entry("dev-libs", "partly", "1.0", Vec::new());
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    installed.slot = Some("0".into());
    installed.sub_slot = Some("0".into());
    let user = new_entry("dev-libs", "user", "1.0", vec![plain("dev-libs/partly")]);
    // NVC first, like the resolver records it. `resolved_dep_targets`
    // is the tree view (`merge_bound_only`), which never edges to an
    // installed node -- assert on `build_digraph`, the scheduler
    // view, where the edge lives.
    let entries = vec![nvc, installed, user];
    let root = Path::new("/nonexistent-root-for-unit-test");
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
        "the NVC entry keeps the edge: {children:?}"
    );
}

#[test]
fn build_digraph_drops_self_edges_except_unsatisfied_buildtime() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // A runtime self-edge is dropped.
    let entries = vec![new_entry(
        "dev-libs",
        "selfish",
        "1.0",
        vec![plain_edge(
            "dev-libs/selfish",
            "dev-libs",
            "selfish",
            runtime,
        )],
    )];
    let g = build_digraph(
        &entries,
        &["dev-libs/selfish".to_string()],
        root,
        &HashMap::new(),
    );
    assert!(
        g.children[0].is_empty(),
        "runtime self-edge dropped: {:?}",
        g.children[0]
    );
    // An unsatisfied build-time self-edge is kept.
    let entries = vec![new_entry(
        "dev-libs",
        "selfish",
        "1.0",
        vec![plain_edge(
            "dev-libs/selfish",
            "dev-libs",
            "selfish",
            DepPriority {
                buildtime: true,
                ..DepPriority::default()
            },
        )],
    )];
    let g = build_digraph(
        &entries,
        &["dev-libs/selfish".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![0],
        "unsatisfied buildtime self-edge kept: {:?}",
        g.children[0]
    );
}

#[test]
fn build_digraph_skips_a_blocker_and_a_nonmatching_top_atom() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let a = new_entry("dev-libs", "a", "1.0", Vec::new());
    let b = new_entry("dev-libs", "b", "1.0", Vec::new());
    let entries = vec![a, b];
    // A blocker top-level atom is skipped; the second atom seeds.
    let g = build_digraph(
        &entries,
        &["!dev-libs/a".to_string(), "dev-libs/b".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.order,
        vec![1, 0],
        "b seeds, a is appended unwalked: {:?}",
        g.order
    );
    // A version-qualified top atom seeds only its matching instance.
    let a1 = new_entry("dev-libs", "a", "1.0", Vec::new());
    let mut a2 = new_entry("dev-libs", "a", "2.0", Vec::new());
    a2.slot = Some("2".into());
    a2.sub_slot = Some("2".into());
    let entries = vec![a1, a2];
    let g = build_digraph(
        &entries,
        &["<dev-libs/a-2".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.order,
        vec![0, 1],
        "1.0 is seeded, 2.0 appended: {:?}",
        g.order
    );
}

#[test]
fn build_digraph_walks_inline_before_deferred_disjunctive_edges() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut deferred = plain_edge("dev-libs/late", "dev-libs", "late", runtime);
    deferred.disjunctive = true;
    deferred.alt = Some((0, 0));
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![
            plain_edge("dev-libs/early", "dev-libs", "early", runtime),
            deferred,
        ],
    );
    let entries = vec![
        owner,
        new_entry("dev-libs", "early", "1.0", Vec::new()),
        new_entry("dev-libs", "late", "1.0", Vec::new()),
    ];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.order,
        vec![0, 1, 2],
        "inline edge discovered before the deferred disjunctive bundle: {:?}",
        g.order
    );
}

#[test]
fn build_digraph_uninstall_pairs_the_owner_before_the_removal() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![plain_edge("dev-libs/leaf", "dev-libs", "leaf", runtime)],
    );
    let leaf = new_entry("dev-libs", "leaf", "1.0", Vec::new());
    let mut removal = new_entry("dev-libs", "old", "1.0", Vec::new());
    removal.outcome = PretendOutcome::Uninstall {
        version: "1.0".into(),
    };
    removal.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let entries = vec![owner, leaf, removal];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![1],
        "the forward edge is the owner's only child: {:?}",
        g.children[0]
    );
    let reverse = g.children[2]
        .iter()
        .find(|(c, _)| *c == 0)
        .expect("the removal must carry the owner as its child");
    assert_eq!(reverse.1.len(), 1);
    assert!(
        reverse.1[0].buildtime && reverse.1[0].runtime && !reverse.1[0].satisfied,
        "the reverse edge is the hard buildtime+runtime one: {:?}",
        reverse.1
    );
}

#[test]
fn build_digraph_required_by_fallback_adds_only_the_missing_owner_edge() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    // The forward walk missed the owner, so the fallback supplies it.
    let mut dep = new_entry("dev-libs", "dep", "1.0", Vec::new());
    dep.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let owner = new_entry("dev-libs", "owner", "1.0", Vec::new());
    let entries = vec![owner.clone(), dep.clone()];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    let fallback = g.children[0]
        .iter()
        .find(|(c, _)| *c == 1)
        .expect("fallback edge owner -> dep");
    assert!(
        fallback.1[0].runtime && !fallback.1[0].satisfied,
        "fallback priority: {:?}",
        fallback.1
    );
    // With an unrelated child already on the owner, the fallback
    // still appears (the skip is keyed to the removal's own index).
    let leaf = new_entry("dev-libs", "leaf", "1.0", Vec::new());
    let entries = vec![owner, dep, leaf];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string(), "dev-libs/leaf".to_string()],
        root,
        &HashMap::new(),
    );
    assert!(
        g.children[0].iter().any(|(c, _)| *c == 1),
        "fallback edge still added beside an unrelated child: {:?}",
        g.children[0]
    );
}

#[test]
fn build_digraph_fallback_skips_a_cp_the_owner_already_edged() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    // The owner's forward edge resolves the slot-qualified atom to
    // slot 1; slot 2's own required_by names the owner, but the
    // cp-level fallback must not add a second edge to it.
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![plain_edge("dev-libs/x:1", "dev-libs", "x", runtime)],
    );
    let mut x1 = new_entry("dev-libs", "x", "1.0", Vec::new());
    x1.slot = Some("1".into());
    x1.sub_slot = Some("1".into());
    let mut x2 = new_entry("dev-libs", "x", "2.0", Vec::new());
    x2.slot = Some("2".into());
    x2.sub_slot = Some("2".into());
    x2.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let entries = vec![owner, x1, x2];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.children[0].iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![1],
        "no fallback edge to the other slot of the same cp: {:?}",
        g.children[0]
    );
    // A same-category different-package child does not count as the
    // same cp: the fallback must still be added.
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![plain_edge("dev-libs/y", "dev-libs", "y", runtime)],
    );
    let y = new_entry("dev-libs", "y", "1.0", Vec::new());
    let mut x = new_entry("dev-libs", "x", "1.0", Vec::new());
    x.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let entries = vec![owner, y, x];
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string(), "dev-libs/y".to_string()],
        root,
        &HashMap::new(),
    );
    assert!(
        g.children[0].iter().any(|(c, _)| *c == 2),
        "an unrelated same-category child is not the same cp: {:?}",
        g.children[0]
    );
}

#[test]
fn build_digraph_skips_a_self_named_required_by() {
    let root = Path::new("/nonexistent-root-for-unit-test");
    let mut a = new_entry("dev-libs", "a", "1.0", Vec::new());
    a.required_by = vec![("dev-libs".to_string(), "a".to_string())];
    let entries = vec![a];
    let g = build_digraph(&entries, &["dev-libs/a".to_string()], root, &HashMap::new());
    assert!(
        g.children[0].is_empty(),
        "a self fallback edge is skipped: {:?}",
        g.children[0]
    );
}

#[test]
fn build_digraph_expands_disjunctive_bundles_by_their_own_key() {
    // Two deferred bundles from different dep keys must each expand
    // only their own key's edges: RDEPEND's bundle is queued first,
    // DEPEND's second, and the LIFO pop expands DEPEND's before
    // RDEPEND's.
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut rdep = plain_edge("dev-libs/b", "dev-libs", "b", runtime);
    rdep.disjunctive = true;
    rdep.alt = Some((0, 0));
    rdep.key = 0;
    let mut dep = plain_edge("dev-libs/c", "dev-libs", "c", runtime);
    dep.disjunctive = true;
    dep.alt = Some((0, 0));
    dep.key = 3;
    let owner = new_entry(
        "dev-libs",
        "owner",
        "1.0",
        vec![
            plain_edge("dev-libs/a", "dev-libs", "a", runtime),
            rdep,
            dep,
        ],
    );
    let entries = vec![
        owner,
        new_entry("dev-libs", "a", "1.0", Vec::new()),
        new_entry("dev-libs", "b", "1.0", Vec::new()),
        new_entry("dev-libs", "c", "1.0", Vec::new()),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert_eq!(
        g.order,
        vec![0, 1, 3, 2],
        "inline a, then DEPEND's bundle (c), then RDEPEND's (b)"
    );
}

#[test]
fn build_digraph_required_by_fallback_marks_an_installed_target_satisfied() {
    let mut dep = new_entry("dev-libs", "dep", "1.0", Vec::new());
    dep.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".into(),
    };
    dep.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let owner = new_entry("dev-libs", "owner", "1.0", Vec::new());
    let entries = vec![owner, dep];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let g = build_digraph(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    let fallback = g.children[0]
        .iter()
        .find(|(c, _)| *c == 1)
        .expect("fallback edge owner -> dep");
    assert!(
        fallback.1[0].runtime && fallback.1[0].satisfied,
        "an installed target's fallback edge is satisfied: {:?}",
        fallback.1
    );
}

#[test]
fn deep_system_deps_follows_only_runtime_edges() {
    let config = portage_profile::Config {
        system_packages: vec!["dev-libs/sys".to_string()],
        ..portage_profile::Config::default()
    };
    let entries = vec![
        new_entry("dev-libs", "sys", "1.0", Vec::new()),
        new_entry("dev-libs", "rt", "1.0", Vec::new()),
        new_entry("dev-libs", "bt", "1.0", Vec::new()),
        new_entry("dev-libs", "post", "1.0", Vec::new()),
    ];
    let g = test_graph(
        4,
        &[
            (0, 1, prio(2)), // runtime
            (0, 2, prio(1)), // buildtime
            (1, 3, prio(4)), // runtime_post
        ],
    );
    assert_eq!(
        deep_system_deps(&g, &entries, &config),
        vec![true, true, false, true]
    );
}

#[test]
fn seed_toolchain_asap_pins_the_provider_lookup() {
    // Two new providers, plus a duplicate atom for the same provider
    // and an installed one that must not be seeded.
    let entries = vec![
        {
            let mut e = new_entry(
                "virtual",
                "os-headers",
                "1.0",
                vec![
                    rdepend_edge("sys-kernel/headers", "sys-kernel", "headers"),
                    rdepend_edge(">=sys-kernel/headers-1", "sys-kernel", "headers"),
                    rdepend_edge("sys-kernel/oldheaders", "sys-kernel", "oldheaders"),
                ],
            );
            e.outcome = PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            };
            e
        },
        new_entry("sys-kernel", "headers", "6.0", Vec::new()),
        {
            let mut e = new_entry(
                "virtual",
                "libc",
                "1.0",
                vec![
                    rdepend_edge("sys-kernel/headers", "sys-kernel", "headers"),
                    rdepend_edge("sys-libs/libc", "sys-libs", "libc"),
                ],
            );
            e.outcome = PretendOutcome::AlreadyInstalled {
                version: "1.0".into(),
            };
            e
        },
        new_entry("sys-libs", "other", "1.0", Vec::new()),
        new_entry("sys-libs", "libc", "2.0", Vec::new()),
        {
            let mut e = new_entry("sys-kernel", "oldheaders", "5.0", Vec::new());
            e.outcome = PretendOutcome::AlreadyInstalled {
                version: "5.0".into(),
            };
            e
        },
    ];
    // os-headers first, then libc; the duplicate atom is deduped and
    // the installed provider is skipped.
    assert_eq!(seed_toolchain_asap(&entries), vec![1, 4]);
    // A reinstall provider is not seeded.
    let mut entries = entries;
    entries[1].outcome = PretendOutcome::Reinstall {
        version: "6.0".into(),
        changed_flags: vec!["flip".into()],
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    assert_eq!(seed_toolchain_asap(&entries), vec![4]);
}

#[test]
fn gather_deps_collects_the_filtered_closure_and_rejects_escapes() {
    let g = test_graph(4, &[(0, 1, prio(2)), (1, 2, prio(2)), (0, 3, prio(32))]);
    let mergeable: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
    assert_eq!(
        gather_deps(&g, 0, None, &mergeable),
        Some([0, 1, 2, 3].into_iter().collect())
    );
    // The optional child 3 escapes the mergeable set.
    let mergeable: HashSet<usize> = [0, 1, 2].into_iter().collect();
    assert_eq!(gather_deps(&g, 0, None, &mergeable), None);
    // The filter drops the escaping optional edge.
    assert_eq!(
        gather_deps(&g, 0, Some(n_ignore_optional), &mergeable),
        Some([0, 1, 2].into_iter().collect())
    );
    // A deeper escape is rejected too.
    let mergeable: HashSet<usize> = [0].into_iter().collect();
    assert_eq!(gather_deps(&g, 0, None, &mergeable), None, "1 escapes");
}
