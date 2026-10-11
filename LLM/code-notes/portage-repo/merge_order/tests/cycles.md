# crates/portage-repo/src/merge_order/tests/cycles.rs
Cycle enumeration, smallest-cycle search, and cycle-harvest pins.

## fn elementary_cycles_counts_rotations_like_portage
- The square shape (one ring per node, four records) is what trips Portage's `> 3` large-cycle advisory; the self-loop singleton mirrors Portage's `shortest_path(x, x) == [x]`.

## fn reduced_merge_order_drains_a_set_in_leaf_order
- The all-in-drain shape mirrors Portage's stuck remainder.

## fn find_smallest_cycle_pins_the_ladder_and_smallest_pick
- The smaller closure wins even though the bigger ring sorts first; the ignore assertion pins the lowest relaxed rung that produces a leaf.

## fn find_smallest_cycle_keeps_the_first_of_two_equal_rings
- The first of the equal-size rings is kept; `<=` would replace it with the later one.
