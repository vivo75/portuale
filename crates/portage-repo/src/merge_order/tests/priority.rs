use super::*;

#[test]
fn dep_edges_carry_the_evaluated_atom_for_candidates_display() {
    // #138 (Phase 5b S3): real's `--debug` prints `Depstring:` raw
    // and `Candidates:` evaluated -- the *evaluated* form must ride
    // the edge beside the raw token. The parent was built -flip, so
    // `~dev-libs/x-1.0[flip=]` evaluates to `[-flip]` (oracle:
    // `logs/l111-s0-20260921/real-rest-debug.log`, bed
    // `l0-fx-20260922T192350Z` arm A).
    let mut metadata = HashMap::new();
    metadata.insert(
        "RDEPEND".to_string(),
        "~dev-libs/deepusedepchild-1.0[flip=]".to_string(),
    );
    let use_flags: HashSet<String> = HashSet::new();
    let edges = dep_edges_from_metadata(&metadata, &use_flags, &["RDEPEND"], false);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].atom, "~dev-libs/deepusedepchild-1.0[flip=]");
    assert_eq!(edges[0].evaluated, "~dev-libs/deepusedepchild-1.0[-flip]");
}

#[test]
fn key_priority_pins_each_dep_key_ladder() {
    let runtime = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    assert_eq!(key_priority("RDEPEND", false), runtime);
    assert_eq!(key_priority("RDEPEND", true), runtime);
    assert_eq!(key_priority("IDEPEND", false), runtime);
    assert_eq!(key_priority("IDEPEND", true), runtime);
    assert_eq!(
        key_priority("PDEPEND", false),
        DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        }
    );
    let buildtime = DepPriority {
        buildtime: true,
        ..DepPriority::default()
    };
    assert_eq!(key_priority("DEPEND", false), buildtime);
    assert_eq!(key_priority("BDEPEND", false), buildtime);
    // `built` only flips the build-time keys' `optional`.
    assert_eq!(
        key_priority("DEPEND", true),
        DepPriority {
            buildtime: true,
            optional: true,
            ..DepPriority::default()
        }
    );
    assert_eq!(
        key_priority("BDEPEND", true),
        DepPriority {
            buildtime: true,
            optional: true,
            ..DepPriority::default()
        }
    );
}

#[test]
fn dep_edges_from_metadata_pins_every_key_and_priority() {
    let metadata = dep_metadata(&[
        ("RDEPEND", "dev-libs/rdep"),
        ("IDEPEND", "dev-libs/idep"),
        ("PDEPEND", "dev-libs/pdep"),
        ("DEPEND", "dev-libs/dep"),
        ("BDEPEND", "dev-libs/bdep"),
    ]);
    let keys = ["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"];
    let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &keys, false);
    assert_eq!(edges.len(), 5);
    // Real's own deps-tuple key order.
    let by_key: HashMap<u8, &DepEdge> = edges.iter().map(|e| (e.key, e)).collect();
    assert_eq!(by_key.len(), 5, "one edge per key: {edges:#?}");
    assert_eq!(by_key[&0].package, "rdep");
    assert_eq!(by_key[&1].package, "idep");
    assert_eq!(by_key[&2].package, "pdep");
    assert_eq!(by_key[&3].package, "dep");
    assert_eq!(by_key[&4].package, "bdep");
    assert_eq!(
        by_key[&0].priority,
        DepPriority {
            runtime: true,
            ..DepPriority::default()
        }
    );
    assert_eq!(
        by_key[&1].priority,
        DepPriority {
            runtime: true,
            ..DepPriority::default()
        }
    );
    assert_eq!(
        by_key[&2].priority,
        DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        }
    );
    assert_eq!(
        by_key[&3].priority,
        DepPriority {
            buildtime: true,
            ..DepPriority::default()
        }
    );
    assert_eq!(
        by_key[&4].priority,
        DepPriority {
            buildtime: true,
            ..DepPriority::default()
        }
    );
    // `built` marks only the build-time keys optional.
    let built = dep_edges_from_metadata(&metadata, &HashSet::new(), &keys, true);
    assert!(
        built
            .iter()
            .filter(|e| e.key >= 3)
            .all(|e| e.priority.optional)
    );
    assert!(
        built
            .iter()
            .filter(|e| e.key < 3)
            .all(|e| !e.priority.optional)
    );
    // The same atom named by two keys keeps both priorities, and a
    // duplicate within one key is deduped.
    let metadata = dep_metadata(&[
        ("RDEPEND", "dev-libs/a dev-libs/a"),
        ("DEPEND", "dev-libs/a"),
    ]);
    let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &["RDEPEND", "DEPEND"], false);
    assert_eq!(edges.len(), 2, "two keys keep both priorities: {edges:#?}");
    assert!(edges.iter().any(|e| e.priority.runtime));
    assert!(edges.iter().any(|e| e.priority.buildtime));
}

#[test]
fn dep_edges_from_metadata_pins_blockers_slot_ops_and_disjunctions() {
    let metadata = dep_metadata(&[(
        "RDEPEND",
        "!dev-libs/blocked dev-libs/plain dev-libs/slotop:= || ( dev-libs/alt-a dev-libs/alt-b )",
    )]);
    let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &["RDEPEND"], false);
    let atoms: HashSet<&str> = edges.iter().map(|e| e.atom.as_str()).collect();
    assert!(
        !atoms.contains("!dev-libs/blocked"),
        "a blocker is never a merge-order edge: {atoms:?}"
    );
    assert_eq!(
        edges.len(),
        4,
        "plain + slot-op + two alternatives: {edges:#?}"
    );
    let plain = edges.iter().find(|e| e.atom == "dev-libs/plain").unwrap();
    assert!(
        plain.priority.runtime && !plain.priority.runtime_slot_op,
        "a plain atom is not slot-operator: {:?}",
        plain.priority
    );
    let slotop = edges
        .iter()
        .find(|e| e.atom == "dev-libs/slotop:=")
        .unwrap();
    assert!(
        slotop.priority.runtime && slotop.priority.runtime_slot_op,
        "`:=` promotes the runtime key: {:?}",
        slotop.priority
    );
    let alt_a = edges.iter().find(|e| e.atom == "dev-libs/alt-a").unwrap();
    let alt_b = edges.iter().find(|e| e.atom == "dev-libs/alt-b").unwrap();
    assert_eq!(alt_a.alt, Some((0, 0)));
    assert_eq!(alt_b.alt, Some((0, 1)));
    assert!(alt_a.disjunctive && alt_b.disjunctive);
    // A build-time `:=` promotes `buildtime_slot_op`, not runtime.
    let metadata = dep_metadata(&[("DEPEND", "dev-libs/dep:=")]);
    let edges = dep_edges_from_metadata(&metadata, &HashSet::new(), &["DEPEND"], false);
    assert_eq!(edges.len(), 1);
    assert!(
        edges[0].priority.buildtime && edges[0].priority.buildtime_slot_op,
        "{:?}",
        edges[0].priority
    );
    assert!(!edges[0].priority.runtime && !edges[0].priority.runtime_slot_op);
}

#[test]
fn split_disjunctive_pins_group_and_branch_bookkeeping() {
    // Inline atoms only.
    assert_eq!(
        split_disjunctive(&tokens(&["dev-libs/a", "dev-libs/b"])),
        (tokens(&["dev-libs/a", "dev-libs/b"]), vec![])
    );
    // One `||` group, bare alternatives: each atom is its own branch.
    assert_eq!(
        split_disjunctive(&tokens(&["||", "(", "dev-libs/a", "dev-libs/b", ")"])),
        (
            vec![],
            vec![
                ("dev-libs/a".to_string(), 0, 0),
                ("dev-libs/b".to_string(), 0, 1),
            ]
        )
    );
    // A nested multi-atom branch keeps one branch number for every
    // atom inside it.
    assert_eq!(
        split_disjunctive(&tokens(&[
            "||",
            "(",
            "(",
            "dev-libs/a",
            "dev-libs/b",
            ")",
            "dev-libs/c",
            ")"
        ])),
        (
            vec![],
            vec![
                ("dev-libs/a".to_string(), 0, 0),
                ("dev-libs/b".to_string(), 0, 0),
                ("dev-libs/c".to_string(), 0, 1),
            ]
        )
    );
    // A second `||` group counts separately.
    assert_eq!(
        split_disjunctive(&tokens(&[
            "||",
            "(",
            "dev-libs/a",
            ")",
            "||",
            "(",
            "dev-libs/b",
            ")"
        ])),
        (
            vec![],
            vec![
                ("dev-libs/a".to_string(), 0, 0),
                ("dev-libs/b".to_string(), 1, 0),
            ]
        )
    );
    // A `virtual/*` atom outside a group is deferred with the
    // sentinel group/branch; anything else stays inline.
    assert_eq!(
        split_disjunctive(&tokens(&["virtual/foo", "dev-libs/a"])),
        (
            tokens(&["dev-libs/a"]),
            vec![("virtual/foo".to_string(), u32::MAX, u32::MAX)]
        )
    );
    // After the group closes, atoms are inline again; a `||` with no
    // following group is not a deferral.
    assert_eq!(
        split_disjunctive(&tokens(&["||", "(", "dev-libs/a", ")", "dev-libs/b"])),
        (
            tokens(&["dev-libs/b"]),
            vec![("dev-libs/a".to_string(), 0, 0)]
        )
    );
    assert_eq!(
        split_disjunctive(&tokens(&["||", "dev-libs/a"])),
        (tokens(&["dev-libs/a"]), vec![])
    );
}

#[test]
fn ignore_predicates_pin_every_ladder_rung() {
    // The expected value of every `ignore_priority` predicate for
    // every combination of `DepPriority` flags. Mirrors the ladder's
    // own documented rule; any deleted `!`, flipped connective, or
    // constant-return mutant disagrees on some word.
    fn expect_n_optional(p: &DepPriority) -> bool {
        p.optional
    }
    fn expect_n_runtime_post(p: &DepPriority) -> bool {
        p.optional || p.runtime_post
    }
    fn expect_n_runtime(p: &DepPriority) -> bool {
        !p.runtime_slot_op && (p.optional || !p.buildtime)
    }
    fn expect_s_satisfied_runtime_post(p: &DepPriority) -> bool {
        if p.optional {
            return true;
        }
        if !p.satisfied {
            return false;
        }
        if p.buildtime || p.runtime {
            return false;
        }
        p.runtime_post
    }
    fn expect_s_runtime_post(p: &DepPriority) -> bool {
        if p.optional {
            return true;
        }
        if p.buildtime || p.runtime {
            return false;
        }
        p.runtime_post
    }
    fn expect_s_satisfied_runtime(p: &DepPriority) -> bool {
        if p.optional {
            return true;
        }
        if p.buildtime {
            return false;
        }
        if !p.runtime {
            return true;
        }
        p.satisfied
    }
    fn expect_s_satisfied_buildtime(p: &DepPriority) -> bool {
        if p.optional {
            return true;
        }
        if p.buildtime_slot_op {
            return false;
        }
        p.satisfied
    }
    fn expect_s_satisfied_buildtime_slot_op(p: &DepPriority) -> bool {
        if p.optional {
            return true;
        }
        if p.satisfied {
            return true;
        }
        !p.buildtime && !p.runtime
    }
    fn expect_s_runtime(p: &DepPriority) -> bool {
        (!p.runtime_slot_op || p.satisfied) && (p.satisfied || p.optional || !p.buildtime)
    }
    for word in 0..128u64 {
        let p = prio(word);
        assert_eq!(
            n_ignore_optional(&p),
            expect_n_optional(&p),
            "n_ignore_optional {word}"
        );
        assert_eq!(
            n_ignore_runtime_post(&p),
            expect_n_runtime_post(&p),
            "n_ignore_runtime_post {word}"
        );
        assert_eq!(
            n_ignore_runtime(&p),
            expect_n_runtime(&p),
            "n_ignore_runtime {word}"
        );
        assert_eq!(
            s_ignore_optional(&p),
            expect_n_optional(&p),
            "s_ignore_optional {word}"
        );
        assert_eq!(
            s_ignore_satisfied_runtime_post(&p),
            expect_s_satisfied_runtime_post(&p),
            "s_ignore_satisfied_runtime_post {word}"
        );
        assert_eq!(
            s_ignore_runtime_post(&p),
            expect_s_runtime_post(&p),
            "s_ignore_runtime_post {word}"
        );
        assert_eq!(
            s_ignore_satisfied_runtime(&p),
            expect_s_satisfied_runtime(&p),
            "s_ignore_satisfied_runtime {word}"
        );
        assert_eq!(
            s_ignore_satisfied_buildtime(&p),
            expect_s_satisfied_buildtime(&p),
            "s_ignore_satisfied_buildtime {word}"
        );
        assert_eq!(
            s_ignore_satisfied_buildtime_slot_op(&p),
            expect_s_satisfied_buildtime_slot_op(&p),
            "s_ignore_satisfied_buildtime_slot_op {word}"
        );
        assert_eq!(
            s_ignore_runtime(&p),
            expect_s_runtime(&p),
            "s_ignore_runtime {word}"
        );
    }
}

#[test]
fn ignore_name_pins_every_traced_filter() {
    assert_eq!(ignore_name(None), "none");
    assert_eq!(ignore_name(Some(n_ignore_optional)), "ignore_optional");
    assert_eq!(
        ignore_name(Some(n_ignore_runtime_post)),
        "ignore_runtime_post"
    );
    assert_eq!(ignore_name(Some(n_ignore_runtime)), "ignore_runtime");
    assert_eq!(ignore_name(Some(s_ignore_optional)), "ignore_optional");
    assert_eq!(
        ignore_name(Some(s_ignore_satisfied_runtime_post)),
        "ignore_satisfied_runtime_post"
    );
    assert_eq!(
        ignore_name(Some(s_ignore_runtime_post)),
        "ignore_runtime_post"
    );
    assert_eq!(
        ignore_name(Some(s_ignore_satisfied_runtime)),
        "ignore_satisfied_runtime"
    );
    assert_eq!(
        ignore_name(Some(s_ignore_satisfied_buildtime)),
        "ignore_satisfied_buildtime"
    );
    assert_eq!(
        ignore_name(Some(s_ignore_satisfied_buildtime_slot_op)),
        "ignore_satisfied_buildtime_slot_op"
    );
    assert_eq!(ignore_name(Some(s_ignore_runtime)), "ignore_runtime");
}

#[test]
fn priority_ranges_pin_their_named_rungs() {
    // Every ladder index resolves to the predicate of its documented
    // name (behavioral comparison, so a textually-identical pair
    // folding to one address still agrees).
    for word in 0..128u64 {
        let p = prio(word);
        assert_eq!(
            NORMAL.ig(1).unwrap()(&p),
            n_ignore_optional(&p),
            "NORMAL.ig(1) {word}"
        );
        assert_eq!(
            NORMAL.ig(2).unwrap()(&p),
            n_ignore_runtime_post(&p),
            "NORMAL.ig(2) {word}"
        );
        assert_eq!(
            NORMAL.ig(3).unwrap()(&p),
            n_ignore_runtime(&p),
            "NORMAL.ig(3) {word}"
        );
        assert_eq!(
            SATISFIED.ig(1).unwrap()(&p),
            s_ignore_optional(&p),
            "SATISFIED.ig(1)"
        );
        assert_eq!(
            SATISFIED.ig(2).unwrap()(&p),
            s_ignore_satisfied_runtime_post(&p),
            "SATISFIED.ig(2) {word}"
        );
        assert_eq!(
            SATISFIED.ig(3).unwrap()(&p),
            s_ignore_runtime_post(&p),
            "SATISFIED.ig(3) {word}"
        );
        assert_eq!(
            SATISFIED.ig(4).unwrap()(&p),
            s_ignore_satisfied_runtime(&p),
            "SATISFIED.ig(4) {word}"
        );
        assert_eq!(
            SATISFIED.ig(5).unwrap()(&p),
            s_ignore_satisfied_buildtime(&p),
            "SATISFIED.ig(5) {word}"
        );
        assert_eq!(
            SATISFIED.ig(6).unwrap()(&p),
            s_ignore_satisfied_buildtime_slot_op(&p),
            "SATISFIED.ig(6) {word}"
        );
        assert_eq!(
            SATISFIED.ig(7).unwrap()(&p),
            s_ignore_runtime(&p),
            "SATISFIED.ig(7)"
        );
        assert_eq!(
            NORMAL.ig_medium().unwrap()(&p),
            n_ignore_runtime(&p),
            "medium"
        );
        assert_eq!(
            NORMAL.ig_medium_soft().unwrap()(&p),
            n_ignore_runtime_post(&p),
            "medium_soft"
        );
        assert_eq!(
            SATISFIED.ig_medium().unwrap()(&p),
            s_ignore_runtime(&p),
            "s-medium"
        );
        assert_eq!(
            SATISFIED.ig_medium_soft().unwrap()(&p),
            s_ignore_satisfied_buildtime_slot_op(&p),
            "s-medium_soft"
        );
    }
    assert_eq!(
        (NORMAL.medium, NORMAL.medium_soft, NORMAL.medium_post),
        (3, 2, 2)
    );
    assert_eq!(
        (
            SATISFIED.medium,
            SATISFIED.medium_soft,
            SATISFIED.medium_post
        ),
        (7, 6, 3)
    );
    assert!(NORMAL.ig(0).is_none(), "the NORMAL ladder starts at NONE");
}

// -----------------------------------------------------------------
// #146 follow-up: the survivors of the first after-run
// -----------------------------------------------------------------

#[test]
fn split_disjunctive_keeps_a_group_when_a_second_group_marker_is_pending() {
    // A defensive-shape pin: `||` immediately followed by another
    // group inside an open group must not restart the group
    // bookkeeping (a structured reduce flattens nested `||`, so this
    // pins the guard against a future walker change).
    assert_eq!(
        split_disjunctive(&tokens(&["||", "(", "||", "(", "dev-libs/a", ")", ")"])),
        (vec![], vec![("dev-libs/a".to_string(), 0, 0)])
    );
}
