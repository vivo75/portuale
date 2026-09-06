use super::*;

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
    // Portage's own deps-tuple key order.
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
fn ignore_predicates_pin_every_ladder_rung() {
    // The expected value of every `ignore_priority` predicate for every
    // combination of `DepPriority` flags.
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
