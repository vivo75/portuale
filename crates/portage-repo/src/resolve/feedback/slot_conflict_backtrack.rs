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
            excluded: ctx.excluded,
            params: grown,
            slot_pullers: &pass.slot_pullers,
            sc: &pass.slot_conflicts[0],
            config,
            gate: ctx.gate(),
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
            fold_rebuild_scan(pass, ctx, grown, config);
            return Some(PassDecision::Feedback(BacktrackFeedback::SlotConflict {
                base: Box::new(std::mem::take(grown)),
                choices,
            }));
        }
    }
    None
}

/// Folds the slot-operator rebuild scan and the update-force scan into `grown`:
/// Portage's probes of one pass all land in one `config` change.
// Portage: depgraph.py, _slot_operator_trigger_reinstalls
pub(crate) fn fold_rebuild_scan(
    pass: &PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
) {
    let nvc = pass.suppressed_nvc
        || pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));
    if nvc
        || ctx.backtrack_max == 0
        || ctx.ignore_built_slot_operator_deps
        || !ctx.rebuild_if_new_slot
    {
        return;
    }
    let top_atoms: HashSet<String> = ctx.top_level.iter().map(|s| (*s).to_string()).collect();
    let scan_scope = ctx.scan_reachable(&pass.entries);
    let (mut scheduled, _abi, _masked) = slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
        root: ctx.root,
        entries: &pass.entries,
        reachable: &ctx.slot_op_reachable,
        scope: Some(&scan_scope),
        walked_installed: &installed_walked_cps(&pass.installed_walked),
        already: &grown.slot_operator_replace_installed,
        undone: &grown.slot_operator_undone,
        with_bdeps: ctx.with_bdeps,
        update: ctx.update,
        top_level_cps: &ctx.top_level_cps,
        top_level_atoms: &top_atoms,
        excluded: ctx.excluded,
        config,
        gate: ctx.gate(),
    });
    let (force_consumers, forced) =
        slot_operator_update_force_scan(SlotOperatorUpdateForceScanArgs {
            root: ctx.root,
            config,
            entries: &pass.entries,
            reachable: &ctx.slot_op_reachable,
            already: &scheduled,
            undone: &grown.slot_operator_undone,
            with_bdeps: ctx.with_bdeps,
            update: ctx.update,
            top_level_cps: &ctx.top_level_cps,
            excluded: ctx.excluded,
            gate: ctx.gate(),
        });
    scheduled.extend(force_consumers);
    grown.slot_operator_forced_upgrade.extend(forced);
    let before = grown.slot_operator_replace_installed.clone();
    drop_masked_installed(ctx.root, grown, &before, &mut scheduled);
    grown.slot_operator_replace_installed = scheduled;
}

/// Whether an installed instance of `cp` was masked by this search.
fn installed_instance_masked(root: &Path, grown: &BacktrackParams, cp: &(String, String)) -> bool {
    let masked = grown.masked_negatives_for(cp);
    all_installed_packages(root).iter().any(|p| {
        p.category == cp.0
            && p.package == cp.1
            && installed_version_masked(&masked, &p.category, &p.package, &p.version)
    })
}

/// Drops the newly joined cps whose installed instance this search masked:
/// it is no longer in the graph, so it cannot join the replace set (Portage
/// schedules the live child only).
pub(crate) fn drop_masked_installed(
    root: &Path,
    grown: &BacktrackParams,
    before: &BTreeSet<(String, String)>,
    scheduled: &mut BTreeSet<(String, String)>,
) {
    scheduled.retain(|cp| before.contains(cp) || !installed_instance_masked(root, grown, cp));
}

/// Drops the rebuild display pairs naming a cp that is masked-installed and
/// not in the replace set: Portage marks `r` only for replace-set members.
pub(crate) fn drop_masked_pairs(
    root: &Path,
    params: &BacktrackParams,
    pairs: &mut Vec<(String, String)>,
) {
    let bt = std::env::var("PORTUALE_BT_TRACE").is_ok_and(|value| value == "1");
    let off = |cpv: &str| {
        let head = cpv.split_once(':').map_or(cpv, |(h, _)| h);
        crate::split_cpv(head).is_some_and(|(c, p, _)| {
            let cp = (c, p);
            !params.slot_operator_replace_installed.contains(&cp)
                && installed_instance_masked(root, params, &cp)
        })
    };
    pairs.retain(|(provider, consumer)| {
        let provider_off = off(provider);
        let consumer_off = off(consumer);
        if bt && (provider_off || consumer_off) {
            // `PORTUALE_BT_TRACE=1` only: which side killed the display pair.
            eprintln!(
                "BT-DROP pair {provider} -> {consumer}: provider_off={provider_off} consumer_off={consumer_off}"
            );
        }
        !provider_off && !consumer_off
    });
}
