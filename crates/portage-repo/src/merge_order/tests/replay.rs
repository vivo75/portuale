use super::*;

#[test]
fn replay_create_graph_fails_at_the_first_contradiction_in_lifo_order() {
    let root = Path::new("/nonexistent-portuale-244");
    let atoms =
        |order: &[&str]| -> Vec<String> { order.iter().map(|p| format!("t/{p}")).collect() };
    // argv `c b a`: A's deps walk first, `-foo` becomes a needed
    // change on d-0, B's `d[foo]` then contradicts it.
    let order = ["c", "b", "a"];
    let entries = aub0_entries(&order);
    let use_of = aub0_use(&entries);
    let r = replay_create_graph(ReplayCreateGraphArgs {
        entries: &entries,
        top_level_atoms: &atoms(&order),
        root,
        allow_flips: true,
        needed_init: &HashMap::new(),
        use_of: &use_of,
    });
    assert_eq!(
        r.failure,
        Some(ReplayFailure {
            parent: 1,
            edge: 0,
            contradiction: true
        })
    );
    assert_eq!(r.admitted, vec![0, 1, 2, 3], "c, b, a, then d-0 by A");
    assert_eq!(r.admitted_by[3], Some((2, 0)));

    // argv `a c b`: B takes d-0 at its default, C adds d-1, and A's
    // `-foo` is the first needed change -- accepted, no failure.
    let order = ["a", "c", "b"];
    let entries = aub0_entries(&order);
    let use_of = aub0_use(&entries);
    let r = replay_create_graph(ReplayCreateGraphArgs {
        entries: &entries,
        top_level_atoms: &atoms(&order),
        root,
        allow_flips: true,
        needed_init: &HashMap::new(),
        use_of: &use_of,
    });
    assert_eq!(r.failure, None);
    assert_eq!(r.admitted, vec![0, 1, 2, 3, 4]);
    assert_eq!(
        r.edges,
        vec![(2, 0, 3), (1, 0, 4), (0, 0, 3)],
        "walk order B, C, A; both A and B land on d-0"
    );
    let d = ("t".to_string(), "d".to_string());
    assert_eq!(r.needed[&d].get("foo"), Some(&false));
    assert_eq!(r.flip_origin[&d], (0, 0));

    // The next try carries `-foo` in: B's `d[foo]` now contradicts.
    let r2 = replay_create_graph(ReplayCreateGraphArgs {
        entries: &entries,
        top_level_atoms: &atoms(&order),
        root,
        allow_flips: true,
        needed_init: &r.needed,
        use_of: &use_of,
    });
    assert!(r2.failure.is_some_and(|f| f.contradiction && f.parent == 2));

    // Real's clean pass (autounmask off): A's `d[-foo]` fails after
    // B and C admitted both instances.
    let clean = replay_create_graph(ReplayCreateGraphArgs {
        entries: &entries,
        top_level_atoms: &atoms(&order),
        root,
        allow_flips: false,
        needed_init: &HashMap::new(),
        use_of: &use_of,
    });
    assert_eq!(
        clean.failure,
        Some(ReplayFailure {
            parent: 0,
            edge: 0,
            contradiction: false
        })
    );
    assert_eq!(clean.admitted, vec![0, 1, 2, 3, 4]);
}
