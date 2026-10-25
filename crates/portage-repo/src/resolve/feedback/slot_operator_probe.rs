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
        if file_unsat_probe_results(&mut *grown, fresh, binary_masks, ctx.usepkgonly, ctx.root) {
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}

/// Pre-chain trigger: a merge-bound binary parent whose built `:S=` dep names an
/// installed child with a tree update in the bound slot. Runs before the conflict
/// steps so the `slot_operator_mask_built` config change is filed first, as
/// Portage does, and the restart recomputes the conflict under it.
pub(crate) fn step_built_parent_update_probe(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    grown: &mut BacktrackParams,
    config: &Config,
) -> BuiltParentProbe {
    // Portage: depgraph.py, _slot_operator_trigger_reinstalls. Backtrack-gated;
    // the mask set only grows, so the restart terminates.
    if ctx.backtrack_max == 0 || ctx.usepkgonly {
        return BuiltParentProbe::Quiet;
    }
    // Portage runs this probe after `_complete_graph`, so the installed consumers
    // are known; a plain pass stands in with the blocker-retry closure.
    let live = if ctx.slot_op_reachable.is_empty() {
        ctx.blocker_retry_closure.clone()
    } else {
        live_reachable_consumers(ctx, &pass.entries)
    };
    let walked = request_walk_consumers(ctx, &pass.entries);
    let (fresh, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: ctx.root,
        repos: &ctx.repos,
        config,
        local_binpkg: &ctx.local_binpkg,
        entries: &pass.entries,
        with_bdeps: ctx.with_bdeps,
        excluded: ctx.excluded,
        masked: &grown.runtime_pkg_mask,
        installed: &ctx.installed_graph,
        live: &live,
        walked: &walked,
    });
    let probed = fresh.clone();
    if !file_unsat_probe_results(&mut *grown, fresh, masks, ctx.usepkgonly, ctx.root) {
        return BuiltParentProbe::Quiet;
    }
    // Portage: depgraph.py, _slot_operator_update_backtrack (the replace set that
    // becomes `__auto_slot_operator_replace_installed__`, rendered as `r`).
    grown.probe_replaced.extend(probed);
    fold_rebuild_scan(pass, ctx, grown, config);
    // Portage derives the conflict nodes from this very pass on top of the
    // config change, so a pass with conflicts hands its filed change to the
    // conflict steps instead of restarting on it alone.
    if pass.slot_conflicts.is_empty() {
        BuiltParentProbe::Restart(PassDecision::Feedback(BacktrackFeedback::Config {
            params: Box::new(std::mem::take(grown)),
        }))
    } else {
        BuiltParentProbe::Filed
    }
}

/// Clean-pass trigger: an installed parent whose built `:S/SS=` dep a tree
/// replacement re-binds to a new child sub-slot. Runs last, so it never
/// preempts a mask, heal, or disclosure: that is exactly the pass-0 gap
/// (Portage's end-of-pass trigger on a clean pass); conflicted passes seed
/// through `fold_rebuild_scan` instead.
pub(crate) fn step_installed_parent_update_probe(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _slot_operator_trigger_reinstalls (the installed-parent
    // update-probe half). Backtrack- and update-gated; the seed set only ever
    // grows, and seeded parents never re-fire, so the search terminates.
    if ctx.backtrack_max == 0 || !ctx.update {
        return None;
    }
    // Portage runs this probe after `_complete_graph`, so the installed consumers
    // are known; a plain pass stands in with the blocker-retry closure.
    let live = if ctx.slot_op_reachable.is_empty() {
        ctx.blocker_retry_closure.clone()
    } else {
        live_reachable_consumers(ctx, &pass.entries)
    };
    let walked = request_walk_consumers(ctx, &pass.entries);
    let top_atoms: HashSet<String> = ctx.top_level.iter().map(|s| (*s).to_string()).collect();
    let fresh = installed_parent_update_probe(InstalledParentUpdateProbeArgs {
        root: ctx.root,
        repos: &ctx.repos,
        config,
        entries: &pass.entries,
        with_bdeps: ctx.with_bdeps,
        excluded: ctx.excluded,
        masked: &grown.runtime_pkg_mask,
        installed: &ctx.installed_graph,
        live: &live,
        walked: &walked,
        top_level_atoms: &top_atoms,
        update: ctx.update,
    });
    let probed = fresh.clone();
    // No binary masks here: both filings name installed packages (a stale
    // binary parent is the built-parent probe's case, filed just above).
    if !file_unsat_probe_results(
        &mut *grown,
        fresh,
        BTreeSet::new(),
        ctx.usepkgonly,
        ctx.root,
    ) {
        return None;
    }
    // Portage: depgraph.py, _slot_operator_update_backtrack (the replace set that
    // becomes `__auto_slot_operator_replace_installed__`, rendered as `r`).
    grown.probe_replaced.extend(probed);
    fold_rebuild_scan(pass, ctx, grown, config);
    Some(PassDecision::Feedback(BacktrackFeedback::Config {
        params: Box::new(std::mem::take(grown)),
    }))
}

/// What [`step_built_parent_update_probe`] found.
pub(crate) enum BuiltParentProbe {
    /// Nothing new was filed.
    Quiet,
    /// A change was filed into the working copy; the conflict steps build on it.
    Filed,
    /// A change was filed and the pass restarts on it.
    Restart(PassDecision),
}
