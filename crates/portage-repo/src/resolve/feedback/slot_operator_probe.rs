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
    // Backlog #215 (v2 `#24f`): real `_slot_operator_unsatisfied_probe`
    // (`_emerge/depgraph.py:2817`, fired in-walk at `:3447-3458`) as a
    // dead-end-pass trigger -- see [`slot_operator_unsatisfied_probe`].
    // A pass whose only way forward is an installed parent's
    // unsatisfiable built `:=` dep restarts with that parent seeded
    // into `slot_operator_replace_installed` (real's
    // `backtrack_infos["config"]` + `_need_restart`, the same
    // `BacktrackFeedback::Config` the #211 scan returns) instead of
    // aborting. Backtrack-gated like real's `_allow_backtracking`
    // (`:3447`): under `--backtrack=0` the abort stands. The seed only
    // ever grows the set, so the search terminates; a pass with no
    // replacement stays a dead end below.
    if has_nvc && ctx.backtrack_max > 0 {
        let (fresh, binary_masks) = slot_operator_unsatisfied_probe_full(
            ctx.root,
            &ctx.repos,
            &pass.entries,
            &pass.slot_want,
            &pass.slot_pullers,
            &grown.runtime_pkg_mask,
            ctx.with_bdeps,
            ctx.excluded,
            config,
        );
        // #257: a non-installed built (binary) parent is masked like
        // #212's stale binaries (`[binary]`-marked `SlotOperatorBuilt`
        // negative, `runtime_pkg_mask` bucket), real `:2896-2903`.
        // Filed through the shared helper with the trigger arm above.
        if file_unsat_probe_results(&mut *grown, fresh, binary_masks, ctx.usepkgonly) {
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
