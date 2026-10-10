//! Dead-end trigger: an installed parent's slot-operator dep no candidate satisfies.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// Dead-end trigger: an installed parent's slot-operator dep no candidate satisfies.
pub(crate) fn step_slot_operator_probe(
    has_nvc: bool,
    ctx: &ResolveCtx<'_>,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _slot_operator_unsatisfied_probe. A pass whose only
    // way forward is an installed parent's unsatisfiable built `:=` dep restarts
    // with that parent seeded into the replace set instead of aborting.
    // Backtrack-gated; the set only ever grows, so the search terminates.
    if has_nvc && ctx.backtrack_max > 0 {
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
        // A non-installed built (binary) parent is masked, not seeded; filed
        // through the shared helper with the missing-dep trigger arm.
        if file_unsat_probe_results(&mut *grown, fresh, binary_masks, ctx.usepkgonly) {
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
