use super::*;

#[test]
fn disabled_path_matches_direct_scans() {
    // The PORTAGE_SERIALIZE_FRONTIER_DISABLE fallback routes every
    // query to the plain scans.
    let mut rng = Rng(0xabcdef);
    let n = 8;
    let mut edges = Vec::new();
    for _ in 0..14 {
        edges.push((rng.below(n), rng.below(n), prio(rng.next() & 127)));
    }
    let edges: Vec<_> = edges.into_iter().filter(|(p, c, _)| p != c).collect();
    let g = test_graph(n, &edges);
    for ig in all_filters() {
        assert_eq!(leaves_via(None, &g, ig), g.leaf_nodes(ig));
        for node in 0..n {
            assert_eq!(is_leaf_via(None, &g, node, ig), g.is_leaf(node, ig));
        }
    }
}

#[test]
fn frontier_levels_cover_every_ladder_rung() {
    let g = test_graph(2, &[(0, 1, prio(0))]);
    let fr = SerializeFrontier::build(&g);
    // Level 0 is the None filter, then one level per rung of both ladders.
    assert!(fr.levels[0].is_none(), "level 0 is the None filter");
    assert_eq!(fr.levels.len(), 11);
    assert_eq!(fr.level_of(None), Some(0));
    for ig in all_filters() {
        assert!(fr.level_of(ig).is_some(), "no level for {ig:?}");
    }
    for (l, f) in fr.levels.iter().enumerate() {
        if l == 0 {
            continue;
        }
        assert!(f.is_some(), "level {l} must hold a filter");
    }
}

#[test]
fn frontier_matches_direct_scans_through_removals() {
    // Diamond with mixed priorities, plus an optional edge and a
    // multi-priority edge.
    let g = test_graph(
        4,
        &[
            (0, 1, prio(2)),  // runtime
            (0, 2, prio(32)), // optional
            (1, 3, prio(4)),  // runtime_post
            (2, 3, prio(1)),  // buildtime
            (0, 3, prio(2)),  // second priority on a direct edge
        ],
    );
    let mut g = g;
    // Fold a second priority into the (0,3) edge, like add_edge does.
    g.add_edge(0, 3, prio(16));
    let mut fr = SerializeFrontier::build(&g);
    assert_equivalent(&g, &mut fr);
    // Drain in an order that frees parents mid-sequence, mirroring the
    // selection loop.
    for node in [3, 1, 2, 0] {
        g.alive[node] = false;
        fr.remove(node, &g.children[node].clone(), &g.parents[node].clone());
        g.order.retain(|&i| g.alive[i]);
        assert_equivalent(&g, &mut fr);
    }
    assert!(g.order.is_empty());
}

#[test]
fn frontier_matches_direct_scans_on_random_graphs() {
    // 50 deterministic graphs drained in random order, comparing after
    // every mutation.
    let mut rng = Rng(0x12345678);
    for _ in 0..50 {
        let n = 1 + rng.below(12);
        let mut edges = Vec::new();
        let nedges = rng.below(n * 2 + 1);
        for _ in 0..nedges {
            let (mut p, mut c) = (rng.below(n), rng.below(n));
            if p == c {
                continue;
            }
            if p > c {
                std::mem::swap(&mut p, &mut c);
            }
            let mut prios = vec![prio(rng.next() & 127)];
            if rng.below(4) == 0 {
                prios.push(prio(rng.next() & 127));
            }
            for pr in prios {
                edges.push((p, c, pr));
            }
        }
        let mut g = test_graph(n, &edges);
        let mut fr = SerializeFrontier::build(&g);
        assert_equivalent(&g, &mut fr);
        let mut perm: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = rng.below(i + 1);
            perm.swap(i, j);
        }
        for node in perm {
            g.alive[node] = false;
            fr.remove(node, &g.children[node].clone(), &g.parents[node].clone());
            g.order.retain(|&i| g.alive[i]);
            assert_equivalent(&g, &mut fr);
        }
    }
}

#[test]
fn frontier_add_edge_only_grows_survival() {
    // Parity with Portage's add_edge contract: adding a priority only
    // makes an edge survive more levels, never fewer.
    let mut g = test_graph(2, &[(0, 1, prio(32))]);
    let mut fr = SerializeFrontier::build(&g);
    let before: Vec<u32> = fr.surv[0].to_vec();
    // Sync a grown priority list (a second priority on the edge that
    // survives strictly more levels).
    g.add_edge(0, 1, prio(2));
    fr.add_edge(
        1,
        Some(0),
        &g.children[0].iter().find(|(c, _)| *c == 1).unwrap().1,
    );
    for (l, (&b, &a)) in before.iter().zip(fr.surv[0].iter()).enumerate() {
        assert!(a >= b, "counts must not shrink at level {l}");
    }
    assert_equivalent(&g, &mut fr);
    // Re-adding the identical mask is a no-op.
    let snapshot: Vec<u32> = fr.surv[0].to_vec();
    fr.add_edge(
        1,
        Some(0),
        &g.children[0].iter().find(|(c, _)| *c == 1).unwrap().1,
    );
    assert_eq!(fr.surv[0], snapshot);
    // A brand-new node gets an appended index and starts isolated.
    fr.add_edge(2, None, &[]);
    assert!(fr.surv.len() > 2);
}

// -----------------------------------------------------------------
// The scheduler loop
// -----------------------------------------------------------------

#[test]
fn digraph_child_nodes_and_has_parents_pin_the_alive_filter() {
    let mut g = test_graph(3, &[(0, 1, prio(2)), (0, 2, prio(32))]);
    assert_eq!(g.child_nodes(0, None), vec![1, 2]);
    assert!(g.has_parents(1));
    assert!(!g.has_parents(0));
    // The optional edge drops under `n_ignore_optional`.
    assert_eq!(g.child_nodes(0, Some(Ignore::NOptional)), vec![1]);
    // A dead child disappears.
    g.alive[1] = false;
    assert_eq!(g.child_nodes(0, None), vec![2]);
    // A dead parent does not count.
    g.alive[0] = false;
    assert!(!g.has_parents(2));
}

#[test]
fn digraph_add_edge_merges_priorities_without_duplicates() {
    let mut g = test_graph(2, &[]);
    g.add_edge(0, 1, prio(2));
    g.add_edge(0, 1, prio(2));
    assert_eq!(g.children[0].len(), 1);
    assert_eq!(g.children[0][0].1, vec![prio(2)]);
    g.add_edge(0, 1, prio(1));
    assert_eq!(g.children[0][0].1, vec![prio(2), prio(1)]);
    assert_eq!(g.parents[1], vec![0]);
}
