use super::*;

#[test]
fn elementary_cycles_counts_rotations_like_portage() {
    // Square 0->1->2->3->0: one ring per node (rotations), four
    // records -- the shape that trips real's `> 3` large-cycle
    // advisory.
    let square = test_graph(
        4,
        &[
            (0, 1, prio(1)),
            (1, 2, prio(1)),
            (2, 3, prio(1)),
            (3, 0, prio(1)),
        ],
    );
    let cycles = elementary_cycles(&square, None);
    assert_eq!(cycles.len(), 4);
    for c in &cycles {
        assert_eq!(c.len(), 4);
    }
    // Triangle: three rotations.
    let tri = test_graph(3, &[(0, 1, prio(1)), (1, 2, prio(1)), (2, 0, prio(1))]);
    assert_eq!(elementary_cycles(&tri, None).len(), 3);
    // Acyclic diamond: none.
    let dag = test_graph(
        4,
        &[
            (0, 1, prio(1)),
            (0, 2, prio(1)),
            (1, 3, prio(1)),
            (2, 3, prio(1)),
        ],
    );
    assert!(elementary_cycles(&dag, None).is_empty());
    // Self-loop: the singleton record, like real's
    // shortest_path(x, x) == [x].
    let slf = test_graph(1, &[(0, 0, prio(1))]);
    assert_eq!(elementary_cycles(&slf, None), vec![vec![0]]);
}

#[test]
fn elementary_cycles_honors_the_survival_filter() {
    // Same square, but the 1->2 edge is optional-only: under the
    // satisfied medium_soft rung it drops out, breaking the ring.
    let g = test_graph(
        4,
        &[
            (0, 1, prio(1)),
            (1, 2, prio(32)),
            (2, 3, prio(1)),
            (3, 0, prio(1)),
        ],
    );
    assert_eq!(elementary_cycles(&g, None).len(), 4);
    assert!(
        elementary_cycles(&g, satisfied_medium_soft_rung()).is_empty(),
        "a filter-dropped edge breaks every ring"
    );
}

#[test]
fn elementary_cycles_keeps_every_tied_shortest_path() {
    // 0->1, 0->2, 1->3, 2->3, 3->0: from 0 both children yield
    // length-3 paths ([1,3,0] and [2,3,0]) -- both recorded. The
    // other nodes contribute one each ([3,0,1], [3,0,2], and [0,1,3]
    // from 3, whose BFS reaches 3 via 1 first in sorted order).
    let g = test_graph(
        4,
        &[
            (0, 1, prio(1)),
            (0, 2, prio(1)),
            (1, 3, prio(1)),
            (2, 3, prio(1)),
            (3, 0, prio(1)),
        ],
    );
    let cycles = elementary_cycles(&g, None);
    let from_zero: Vec<_> = cycles.iter().filter(|c| c.last() == Some(&0)).collect();
    assert_eq!(from_zero.len(), 2, "both tied paths recorded: {cycles:?}");
    assert_eq!(cycles.len(), 5);
    assert_eq!(cycle_sets(&cycles).len(), 2);
}

#[test]
fn reduced_merge_order_drains_a_set_in_leaf_order() {
    // Square with a downstream outsider (4, depending on 0), all in
    // the drain set like real's stuck remainder: leaf-drain order,
    // outsider with the rest.
    let g = test_graph(
        5,
        &[
            (0, 1, prio(1)),
            (1, 2, prio(1)),
            (2, 3, prio(1)),
            (3, 0, prio(1)),
            (4, 0, prio(1)),
        ],
    );
    let drain: HashSet<usize> = [0, 1, 2, 3, 4].into_iter().collect();
    let order = reduced_merge_order(&g, &drain, &[]);
    assert_eq!(order.len(), 5);
    assert_eq!(
        order.iter().copied().collect::<HashSet<_>>(),
        drain,
        "the whole drain set, outsider included"
    );
    // Members-only drain: exactly the members.
    let members: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
    let order = reduced_merge_order(&g, &members, &[]);
    assert_eq!(order.len(), 4);
    assert_eq!(order.iter().copied().collect::<HashSet<_>>(), members);
    // Chain: tail-first.
    let chain = test_graph(4, &[(0, 1, prio(1)), (1, 2, prio(1)), (0, 3, prio(1))]);
    let members: HashSet<usize> = [0, 1, 2, 3].into_iter().collect();
    assert_eq!(reduced_merge_order(&chain, &members, &[]), vec![2, 3, 1, 0]);
}

#[test]
fn find_smallest_cycle_pins_the_ladder_and_smallest_pick() {
    // A three-ring (names "aa*") and a two-ring ("zz*"): the smaller
    // closure wins even though the bigger ring sorts first.
    let g = test_graph(
        5,
        &[
            (0, 1, prio(2)),
            (1, 2, prio(2)),
            (2, 0, prio(2)),
            (3, 4, prio(2)),
            (4, 3, prio(2)),
        ],
    );
    let entries = vec![
        new_entry("dev-libs", "aa1", "1.0", Vec::new()),
        new_entry("dev-libs", "aa2", "1.0", Vec::new()),
        new_entry("dev-libs", "aa3", "1.0", Vec::new()),
        new_entry("dev-libs", "zz1", "1.0", Vec::new()),
        new_entry("dev-libs", "zz2", "1.0", Vec::new()),
    ];
    let mut frontier = SerializeFrontier::build(&g);
    let (sub, ig) = find_smallest_cycle(
        &g,
        Some(&mut frontier),
        &entries,
        &NORMAL,
        &[],
        true,
        &HashSet::new(),
    )
    .expect("a mergeable cycle exists");
    assert_eq!(sub, HashSet::from([3usize, 4]));
    assert!(ig.is_some());
    assert_eq!(
        ig.unwrap()(&prio(4)),
        n_ignore_runtime_post(&prio(4)),
        "the lowest relaxed rung that produces a leaf"
    );
}

#[test]
fn harvest_cycle_pins_leaf_order_and_installed_preference() {
    // 0 -> 1 runtime; 1 has no children, so it is the first leaf and
    // 0 follows.
    let g = test_graph(2, &[(0, 1, prio(2))]);
    let sub: HashSet<usize> = [0, 1].into_iter().collect();
    assert_eq!(harvest_cycle(&g, &sub), vec![1, 0]);
    // An installed leaf is preferred over a merge-bound one.
    let mut g = test_graph(2, &[]);
    g.installed[0] = true;
    let sub: HashSet<usize> = [0, 1].into_iter().collect();
    assert_eq!(
        harvest_cycle(&g, &sub),
        vec![0, 1],
        "the installed leaf is picked first"
    );
}

#[test]
fn cycle_report_reports_the_ring_and_its_requirer_cone() {
    let mut b = new_entry(
        "dev-libs",
        "b",
        "1.0",
        vec![runtime_edge("dev-libs/a", "dev-libs", "a")],
    );
    b.required_by = vec![("dev-libs".to_string(), "owner".to_string())];
    let entries = vec![
        new_entry(
            "dev-libs",
            "a",
            "1.0",
            vec![runtime_edge("dev-libs/b", "dev-libs", "b")],
        ),
        b,
        new_entry("dev-libs", "owner", "1.0", Vec::new()),
    ];
    let root = Path::new("/nonexistent-root-for-unit-test");
    let (cycles, display) = cycle_report(
        &entries,
        &["dev-libs/owner".to_string()],
        root,
        &HashMap::new(),
    );
    assert!(
        !cycles.is_empty(),
        "the runtime ring is recorded: {cycles:?}"
    );
    assert_eq!(
        display.len(),
        3,
        "the owner's cone joins the drain: {display:?}"
    );
    assert_eq!(
        display.iter().copied().collect::<HashSet<_>>(),
        HashSet::from([0, 1, 2])
    );
}

#[test]
fn elementary_cycles_records_only_the_shortest_path_per_node() {
    // Node 0's first child leads to a length-3 path back to 0, its
    // second child to a length-2 one: only the shortest is recorded.
    let g = test_graph(
        4,
        &[
            (0, 1, prio(2)),
            (1, 2, prio(2)),
            (2, 0, prio(2)),
            (0, 3, prio(2)),
            (3, 0, prio(2)),
        ],
    );
    let cycles = elementary_cycles(&g, None);
    let to_zero: Vec<Vec<usize>> = cycles
        .iter()
        .filter(|c| c.last() == Some(&0))
        .cloned()
        .collect();
    assert_eq!(
        to_zero,
        vec![vec![3usize, 0]],
        "the longer tied candidate is dropped once a shorter one appears"
    );
}

#[test]
fn find_smallest_cycle_keeps_the_first_of_two_equal_rings() {
    // A 3-ring ("aa*") then two 2-rings ("mm*", "zz*"): the first
    // smallest closure wins; `<=` would replace it with the later
    // equal-size one.
    let g = test_graph(
        7,
        &[
            (0, 1, prio(2)),
            (1, 2, prio(2)),
            (2, 0, prio(2)),
            (3, 4, prio(2)),
            (4, 3, prio(2)),
            (5, 6, prio(2)),
            (6, 5, prio(2)),
        ],
    );
    let entries = vec![
        new_entry("dev-libs", "aa1", "1.0", Vec::new()),
        new_entry("dev-libs", "aa2", "1.0", Vec::new()),
        new_entry("dev-libs", "aa3", "1.0", Vec::new()),
        new_entry("dev-libs", "mm1", "1.0", Vec::new()),
        new_entry("dev-libs", "mm2", "1.0", Vec::new()),
        new_entry("dev-libs", "zz1", "1.0", Vec::new()),
        new_entry("dev-libs", "zz2", "1.0", Vec::new()),
    ];
    let mut frontier = SerializeFrontier::build(&g);
    let (sub, _) = find_smallest_cycle(
        &g,
        Some(&mut frontier),
        &entries,
        &NORMAL,
        &[],
        true,
        &HashSet::new(),
    )
    .expect("a mergeable cycle exists");
    assert_eq!(
        sub,
        HashSet::from([3usize, 4]),
        "the first of the equal-size rings is kept"
    );
}
