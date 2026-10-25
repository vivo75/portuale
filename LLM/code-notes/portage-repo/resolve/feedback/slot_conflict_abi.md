# crates/portage-repo/src/resolve/feedback/slot_conflict_abi.rs
ABI backtrack arm: rebuild a built parent whose soname/`:=` conflict atom the update probe resolves.

## fn step_slot_conflict_abi
- Backlog #214 (v2 `#24e`): Portage `_slot_conflict_backtrack_abi` (`lib/_emerge/depgraph.py:2282-2315`) runs inside `_process_slot_conflict`, BEFORE the masking backtrack below -- a built parent whose soname/`:=` conflict atom the update probe can resolve is rebuilt instead of reported. Same order here: the probe below fires before the solvability/mask arms, and its restart carries no masks (Portage's `remaining` skip).
- Gated on a live search like the scan's own call site (`backtrack_max > 0` is Portage's `_allow_backtracking`; `ignore_built_slot_operator_deps` drops the built atoms the probe reads; no unsatisfiable dependency outstanding, like the `has_nvc` dead-end arm below -- Portage never reaches conflict processing with a failed `_create_graph`).
- Deliberately NOT gated on `rebuild_if_new_slot` (Portage's conflict path never is -- the scan's call-site gate is the pre-existing narrowing for its own arms, not this one).
- Same inputs the scan builds for its own refusal map: every cp this pass walked, the merge-bound set, and the R2 parents.

## gate threading (#571)
- `SlotConflictAbiProbeArgs` uses `ctx.gate()`.
