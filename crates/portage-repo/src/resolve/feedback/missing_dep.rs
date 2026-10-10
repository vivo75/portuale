//! A dependency with no matching package: mask its parent and re-run.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// A dependency with no matching package: mask its parent and re-run.
pub(crate) fn step_missing_dep(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // Real `backtracking.py::_feedback_missing_dep`: this pass hit a
    // dependency with no matching package; mask its parent (`!=cpv` in
    // `runtime_pkg_mask`, latched in `missing_dep_masked` so the
    // same parent is never re-masked) and re-run. Combined with the
    // `||` closure honouring `slot_constraints`, a `|| ( a b )` whose
    // `a` has an unsatisfiable subtree now yields to `b`. A trigger the
    // latch already covers, or a top-level/USE-only mismatch with no
    // trigger at all, falls through below (dead end unless other
    // feedback fired first).
    if let Some(((pc, pp), neg, dep_atom)) = pass.missing_dep_trigger.take() {
        // #257 + #276: real tries the whole
        // `_slot_operator_unsatisfied_probe` before the
        // missing-dependency mask for a built slot-operator atom
        // (`_emerge/depgraph.py:3447-3455` returns ahead of the
        // `runtime_pkg_mask` fallback at `:3456-3470`): a hit seeds the
        // replace set (an installed parent with a resolving
        // replacement) or masks the binary (a non-installed built
        // parent, `slot_operator_mask_built`), and the restart takes
        // the replacement; only a miss falls through to the mask
        // below. Filed through the shared helper (one expansion point,
        // no new restart bookkeeping). Under `--usepkgonly` real's
        // `_iter_similar_available` yields binaries only, so no ebuild
        // replacement exists and the probe is skipped (the
        // missing-dependency path below stands).
        if ctx.backtrack_max > 0
            && !ctx.usepkgonly
            && portage_dep::parse_atom(&dep_atom).is_some_and(|a| is_built_slot_op(&a))
        {
            let (fresh, binary_masks) =
                slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
                    root: ctx.root,
                    repos: &ctx.repos,
                    entries: &pass.entries,
                    slot_want: &pass.slot_want,
                    pullers: &pass.slot_pullers,
                    masked: &grown.runtime_pkg_mask,
                    with_bdeps: ctx.with_bdeps,
                    excluded: ctx.excluded,
                    config,
                });
            if file_unsat_probe_results(&mut *grown, fresh, binary_masks, ctx.usepkgonly) {
                return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(std::mem::take(grown)),
                }));
            }
        }
        // C2: the latch moves into the node with the mask (see
        // `Backtracker::feedback`); the trigger here only packages.
        return Some(PassDecision::Feedback(BacktrackFeedback::MissingDep {
            base: Box::new(std::mem::take(grown)),
            owner: (pc.clone(), pp.clone()),
            entry: MaskEntry {
                neg,
                reason: MaskReason::MissingDependency {
                    parent: (pc, pp),
                    atom: dep_atom,
                },
            },
        }));
    }
    None
}
