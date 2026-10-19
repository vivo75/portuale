//! Restart: an installed parent's build-time dep merged this run.

use crate::resolve::feedback::*;

/// Restart: an installed parent's build-time dep merged this run.
pub(crate) fn step_rebuild_if_restart(
    has_nvc: bool,
    ctx: &ResolveCtx<'_>,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _rebuild_config.trigger_rebuilds + the
    // `_need_restart` outer loop. A pass that merged a build-time dep of a
    // reached installed parent restarts with that parent seeded as a forced
    // reinstall instead of settling, so the parent's own deps re-resolve.
    // Backtrack-gated like the slot-operator trigger (`_allow_backtracking`
    // is `max_retries > 0`); the seed set only ever grows, and seeded
    // parents never re-fire, so the search terminates. Fires only on an
    // otherwise-clean pass: conflicts and misses take their own feedback
    // arms first, exactly as Portage returns before `trigger_rebuilds`.
    if !has_nvc
        && pass.slot_conflicts.is_empty()
        && ctx.backtrack_max > 0
        && !ctx.nodeps
        && (ctx.rebuild_if_unbuilt || ctx.rebuild_if_new_rev || ctx.rebuild_if_new_ver)
    {
        let fresh = rebuild_if_restart_parents(RebuildIfRestartParentsArgs {
            root: ctx.root,
            entries: &pass.entries,
            already: &grown.rebuild_if_replace_installed,
            unbuilt: ctx.rebuild_if_unbuilt,
            new_rev: ctx.rebuild_if_new_rev,
            new_ver: ctx.rebuild_if_new_ver,
            rebuild_exclude: ctx.rebuild_exclude,
            rebuild_ignore: ctx.rebuild_ignore,
        });
        if fresh
            .iter()
            .any(|cp| !grown.rebuild_if_replace_installed.contains(cp))
        {
            grown.rebuild_if_replace_installed.extend(fresh);
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
