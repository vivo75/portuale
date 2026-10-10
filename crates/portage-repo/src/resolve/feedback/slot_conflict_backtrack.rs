//! Portage's slot-conflict backtrack: mask the first conflict's ranked choice.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// Portage's slot-conflict backtrack: mask the first conflict's ranked choice.
pub(crate) fn step_slot_conflict_backtrack(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // One node per ranked choice, first conflicting slot only. Puller-masking now
    // happens only through a later pass's own missing-dep feedback, never pre-masked.
    if !pass.slot_conflicts.is_empty() {
        let choices = slot_conflict_mask_choices(SlotConflictMaskChoicesArgs {
            repos: &ctx.repos,
            excluded: ctx.excluded,
            params: grown,
            slot_pullers: &pass.slot_pullers,
            sc: &pass.slot_conflicts[0],
            config,
        });
        // Novelty is compared on the full entry (neg + reason): at least one group
        // member new to the search state. A repeat with recomputed parents is a
        // distinct state and explores -- the mask budget still caps it.
        let added = choices.iter().any(|c| {
            c.similar
                .iter()
                .map(|s| (&s.target, &s.parents))
                .chain(std::iter::once((&c.target, &c.parents)))
                .any(|((cat, pkg, ver), parents)| {
                    let entry = MaskEntry {
                        neg: format!("!={cat}/{pkg}-{ver}"),
                        reason: MaskReason::SlotConflict {
                            parents: parents.clone(),
                        },
                    };
                    !grown
                        .runtime_pkg_mask
                        .get(&(cat.clone(), pkg.clone()))
                        .is_some_and(|bucket| bucket.contains(&entry))
                })
        });
        if added {
            // Portage runs the slot-operator trigger reinstalls in the same pass, so
            // the replace set rides the conflict's node instead of costing a restart.
            let nvc = pass.suppressed_nvc
                || pass
                    .entries
                    .iter()
                    .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));
            if !nvc
                && ctx.backtrack_max > 0
                && !ctx.ignore_built_slot_operator_deps
                && ctx.rebuild_if_new_slot
            {
                let top_atoms: HashSet<String> =
                    ctx.top_level.iter().map(|s| (*s).to_string()).collect();
                let (mut scheduled, _abi, _masked) =
                    slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
                        root: ctx.root,
                        repos: &ctx.repos,
                        entries: &pass.entries,
                        reachable: &ctx.slot_op_reachable,
                        already: &grown.slot_operator_replace_installed,
                        undone: &grown.slot_operator_undone,
                        with_bdeps: ctx.with_bdeps,
                        update: ctx.update,
                        top_level_cps: &ctx.top_level_cps,
                        top_level_atoms: &top_atoms,
                        excluded: ctx.excluded,
                        config,
                    });
                let (force_consumers, forced) =
                    slot_operator_update_force_scan(SlotOperatorUpdateForceScanArgs {
                        root: ctx.root,
                        repos: &ctx.repos,
                        config,
                        entries: &pass.entries,
                        reachable: &ctx.slot_op_reachable,
                        already: &scheduled,
                        undone: &grown.slot_operator_undone,
                        with_bdeps: ctx.with_bdeps,
                        update: ctx.update,
                        top_level_cps: &ctx.top_level_cps,
                        excluded: ctx.excluded,
                    });
                scheduled.extend(force_consumers);
                grown.slot_operator_forced_upgrade.extend(forced);
                grown.slot_operator_replace_installed = scheduled;
            }
            return Some(PassDecision::Feedback(BacktrackFeedback::SlotConflict {
                base: Box::new(std::mem::take(grown)),
                choices,
            }));
        }
    }
    None
}
