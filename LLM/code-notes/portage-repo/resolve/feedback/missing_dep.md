# crates/portage-repo/src/resolve/feedback/missing_dep.rs
Missing-dependency feedback: mask the parent of an unsatisfiable dep and re-run.

## fn step_missing_dep
- Portage `backtracking.py::_feedback_missing_dep`: this pass hit a dependency with no matching package; mask its parent (`!=cpv` in `runtime_pkg_mask`, latched in `missing_dep_masked` so the same parent is never re-masked) and re-run. Combined with the `||` closure honouring `slot_constraints`, a `|| ( a b )` whose `a` has an unsatisfiable subtree now yields to `b`. A trigger the latch already covers, or a top-level/USE-only mismatch with no trigger at all, falls through below (dead end unless other feedback fired first).
- #257 + #276: Portage tries the whole `_slot_operator_unsatisfied_probe` before the missing-dependency mask for a built slot-operator atom (`_emerge/depgraph.py:3447-3455` returns ahead of the `runtime_pkg_mask` fallback at `:3456-3470`): a hit seeds the replace set (an installed parent with a resolving replacement) or masks the binary (a non-installed built parent, `slot_operator_mask_built`), and the restart takes the replacement; only a miss falls through to the mask below. Filed through the shared helper (one expansion point, no new restart bookkeeping). Under `--usepkgonly` Portage's `_iter_similar_available` yields binaries only, so no ebuild replacement exists and the probe is skipped (the missing-dependency path below stands).
- C2: the latch moves into the node with the mask (see `Backtracker::feedback`); the trigger here only packages.

## gate threading (#571)
- The probe args site uses `ctx.gate()`.

## fn trigger_preempted_by_use_miss (#594)
- Portage's walk stops at the first unsatisfied dependency in `_minimize_children` order: package-less atoms first, in textual order (`depgraph.py:4766-4788`), then the satisfiable ones. A USE-only miss no autounmask change can fix (the `without_use` reselect at `:3494-3498` finds a package, so arm 9 never files) sits in the package-less group; a fixable USE miss sits in the satisfiable group, after every package-less atom, so it never precedes a filed miss.
- The trigger is armed during a reverse-textual walk (see `enqueue_flat_deps`, plains pushed reversed for the stack discipline), so the blocking USE miss may be recorded after the trigger. The retract therefore runs post-walk here: same owner, the miss's atom textually ahead of the trigger's in the owner's dep-string order (the entry `deps` order, a proxy for Portage's per-key order), and the miss's entry carrying no USE suggestion (unfixable). Cross-parent order is still the walk's own FIFO (Portage pops LIFO, `depgraph.py:3276`), so only the same owner retracts; the cross-parent shapes stay out under #609.
