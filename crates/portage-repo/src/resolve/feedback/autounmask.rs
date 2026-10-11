//! Autounmask breakage, or a grown --autounmask-use config to re-run with.

use crate::resolve::feedback::*;

/// Autounmask breakage, or a grown --autounmask-use config to re-run with.
pub(crate) fn step_autounmask(
    grown: &mut BacktrackParams,
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _autounmask_breakage. An accumulated autounmask
    // USE change broke another use-dep irreconcilably: drop every change and
    // re-resolve one final clean pass. Latched to a single extra pass.
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

    // A newly-folded --autounmask-use flip: re-run the walk with the grown
    // config. Budget-free; converges because the flag is set on new entries only.
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
