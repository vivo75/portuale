# crates/portage-repo/src/resolve/feedback/missing_dep.rs
Missing-dependency feedback: mask the parent of an unsatisfiable dep and re-run.

## fn step_missing_dep
- Portage `backtracking.py::_feedback_missing_dep`: this pass hit a dependency with no matching package; mask its parent (`!=cpv` in `runtime_pkg_mask`, latched in `missing_dep_masked` so the same parent is never re-masked) and re-run. Combined with the `||` closure honouring `slot_constraints`, a `|| ( a b )` whose `a` has an unsatisfiable subtree now yields to `b`. A trigger the latch already covers, or a top-level/USE-only mismatch with no trigger at all, falls through below (dead end unless other feedback fired first).
- #257 + #276: Portage tries the whole `_slot_operator_unsatisfied_probe` before the missing-dependency mask for a built slot-operator atom (`_emerge/depgraph.py:3447-3455` returns ahead of the `runtime_pkg_mask` fallback at `:3456-3470`): a hit seeds the replace set (an installed parent with a resolving replacement) or masks the binary (a non-installed built parent, `slot_operator_mask_built`), and the restart takes the replacement; only a miss falls through to the mask below. Filed through the shared helper (one expansion point, no new restart bookkeeping). Under `--usepkgonly` Portage's `_iter_similar_available` yields binaries only, so no ebuild replacement exists and the probe is skipped (the missing-dependency path below stands).
- C2: the latch moves into the node with the mask (see `Backtracker::feedback`); the trigger here only packages.

## gate threading (#571)
- The probe args site uses `ctx.gate()`.
