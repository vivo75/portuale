# crates/portage-repo/src/resolve/feedback/slot_conflict_abi.rs
ABI backtrack arm: rebuild a built parent whose soname/`:=` conflict atom the update probe resolves.

## fn step_slot_conflict_abi
- Backlog #214 (v2 `#24e`): Portage `_slot_conflict_backtrack_abi` (`lib/_emerge/depgraph.py:2282-2315`) runs inside `_process_slot_conflict`, BEFORE the masking backtrack below -- a built parent whose soname/`:=` conflict atom the update probe can resolve is rebuilt instead of reported. Same order here: the probe below fires before the solvability/mask arms, and its restart carries no masks (Portage's `remaining` skip).
- Gated on a live search like the scan's own call site (`backtrack_max > 0` is Portage's `_allow_backtracking`; `ignore_built_slot_operator_deps` drops the built atoms the probe reads; no unsatisfiable dependency outstanding, like the `has_nvc` dead-end arm below -- Portage never reaches conflict processing with a failed `_create_graph`).
- Deliberately NOT gated on `rebuild_if_new_slot` (Portage's conflict path never is -- the scan's call-site gate is the pre-existing narrowing for its own arms, not this one).
- Same inputs the scan builds for its own refusal map: every cp this pass walked, the merge-bound set, and the R2 parents.
- Backlog numbers removed from the comment (original text): Portage's `_slot_operator_update_backtrack` files both sides of the built `:=` dep: `_replace_installed_atom(dep.parent)` and, with `new_child_slot is None`, `_replace_installed_atom(child)` when the child is installed (`depgraph.py:2454-2460`). `probe_replaced` is the display mirror that becomes `__auto_slot_operator_replace_installed__` (the `r` tag), so it grows with the whole filing -- including a subsumed child, whose upgrade row still needs the tag. The seed set takes parents and kept children only: a subsumed child would ghost a reinstall node beside its upgrade (backlog #642).

## gate threading (#571)
- `SlotConflictAbiProbeArgs` uses `ctx.gate()`.
