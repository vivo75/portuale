use super::*;

#[test]
fn elementary_cycles_counts_rotations_like_portage() {
    // Square 0->1->2->3->0: one ring per node (rotations), four
    // records -- the shape that trips Portage's `> 3` large-cycle advisory.
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
    // Self-loop: the singleton record, like Portage's
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
    // the drain set like Portage's stuck remainder: leaf-drain order,
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
