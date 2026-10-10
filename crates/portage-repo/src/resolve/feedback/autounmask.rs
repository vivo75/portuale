//! Autounmask breakage, or a grown --autounmask-use config to re-run with.

use crate::resolve::feedback::*;

/// Autounmask breakage, or a grown --autounmask-use config to re-run with.
pub(crate) fn step_autounmask(
    grown: &mut BacktrackParams,
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
) -> Option<PassDecision> {
    // Real `_autounmask_breakage` (depgraph.py:12262-12280): an
    // accumulated autounmask USE change made another use-dep
    // unsatisfiable and the two can't be reconciled (a flag wanted
    // both ways). Drop every autounmask change, turn suggestion fully
    // off, and re-resolve one final clean pass -- exactly real's
    // `myparams["autounmask"] = False` retry. The `autounmask_disabled`
    // latch keeps it to a single extra pass.
    if grown.autounmask_use_broke && !grown.autounmask_disabled {
        grown.autounmask_disabled = true;
        grown.autounmask_suggest_keywords = false;
        grown.autounmask_suggest_use = false;
        grown.autounmask_suggest_license = false;
        grown.autounmask_suggest_masks = false;
        grown.autounmask_use_config.clear();
        grown.autounmask_use_change_records.clear();
        grown.backtrack_config = None;
        return Some(PassDecision::Feedback(BacktrackFeedback::Config {
            params: Box::new(std::mem::take(grown)),
        }));
    }

    // Backtracking slice: this pass folded a new `--autounmask-use`
    // flip into `autounmask_use_config` (real `_feedback_config`
    // growing `_needed_use_config_changes`). Re-run the whole walk
    // with the grown config so the flipped package is re-resolved
    // with the flag on and its `flag?`-gated deps appear. Converges
    // because `autounmask_grew` is only set on a *newly*-added
    // `(cp, flag)` entry. Config feedback is budget-free (real's
    // `_feedback_config` doesn't count toward `--backtrack=N`).
    if pass.autounmask_grew {
        let mut c = ctx.config.clone();
        c.autounmask_use = autounmask_use_tier(&grown.autounmask_use_config);
        grown.backtrack_config = Some(c);
        return Some(PassDecision::Feedback(BacktrackFeedback::Config {
            params: Box::new(std::mem::take(grown)),
        }));
    }
    None
}
