# crates/portuale/src/emerge_build/tests/merge_wait.rs
Merge-wait scope and cycle-edge pins.

## fn none_scope_selects_nothing
- Oracle: upstream `test_merge_wait_scope.py` (`testMergeWaitScope`).

## fn blocking_deps_ignores_post_edges_of_cyclic_owners
- Oracle: commit `3d677b7b2` (bug 452172).
