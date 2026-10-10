# crates/portage-repo/src/resolve/backtracker.rs
The backtracking search tree.

## struct BacktrackNode / struct Backtracker
- Phase C2 (023). `depth`/`mask_steps` live inside `BacktrackParams` (C1) rather than alongside the node.

## fn check_runtime_pkg_mask
- Portage `Backtracker._check_runtime_pkg_mask` (bug 375573); Portage's 692746 arm (masking an all-matched package to avoid a missed update). Portage `backtracking.py:150-154` skips `slot_operator_mask_built` (and missing-dependency) masks the same way. Carried into C3; C3's real-shaped parent sets are the place to tighten the granularity.

## fn params_equal
- `backtrack_config` is rebuilt deterministically from `autounmask_use_config` whenever it grows.

## impl Backtracker::new
- Portage `_add(self._root)`.

## impl Backtracker::add_node
- Portage `Backtracker._add`: bug-375573 discard.

## impl Backtracker::feedback SlotConflict arm
- Backlog #57 S1: exactly as `PassDecision::DeadEnd` does -- without keeping the accumulators, the dropped reverse-dep pins and autounmask records would be thrown away and the re-pass would report less than the pass that found the conflict did. C3 (Portage's `_feedback_slot_conflict` loop); C4 similar group. Portage assigns `runtime_pkg_mask[pkg]["slot conflict"]`, never accumulates.

## impl Backtracker::feedback mask_order pushes
- Backlog #129 (S1): first-seen order for the missed-update notice (see `mask_order`).

## impl Backtracker::feedback MissingDep arm
- Portage's `dep.parent not in _runtime_pkg_mask` is the mask itself.
