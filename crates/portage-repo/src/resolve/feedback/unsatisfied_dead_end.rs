//! No feedback fired: a pass still carrying an unsatisfied dep is a dead end.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// No feedback fired: a pass still carrying an unsatisfied dep is a dead end.
pub(crate) fn step_unsatisfied_dead_end(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
    reverse_pins: &[RevDepPin],
) -> ControlFlow<PassDecision, bool> {
    // No feedback fired: a dead end, not a report -- settling here would abort on the
    // masked graph instead of the best run. A conflict-only fall-through still settles.
    let has_nvc = pass.suppressed_nvc
        || pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));

    // Portage: depgraph.py, _slot_operator_trigger_reinstalls, on the settled graph. A
    // scanned consumer becomes a *walked node* next pass, so its deps, `:=` binding and
    // merge-order edges are real. Last in the chain; budget-free and ever-growing
    // (bounded by installed cp count), so the search terminates.
    //
    // `--backtrack=0` turns the trigger off (`allow_backtracking = max_retries > 0`):
    // with no search there is no restart to apply the set on. `--nodeps` matches.
    //
    // `abi_rebuilds` is always recorded (empty when off) so `assemble_result` knows the
    // scan ran; only the `get_best_run` re-pass (never here) leaves it `None`.
    if has_nvc
        || ctx.backtrack_max == 0
        || ctx.ignore_built_slot_operator_deps
        || !ctx.rebuild_if_new_slot
    {
        pass.abi_rebuilds = Some(Vec::new());
    } else {
        // Raw request atoms behind `top_level_cps` (explicit slot/version atoms pin nothing).
        let top_atoms: HashSet<String> = ctx.top_level.iter().map(|s| (*s).to_string()).collect();
        let (scheduled, abi_rebuilds, masked_binaries) =
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
        pass.abi_rebuilds = Some(abi_rebuilds);
        // The update probe's forcing half, for a child the walk left installed.
        let mut scheduled = scheduled;
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
        let forced_new = !forced.is_subset(&grown.slot_operator_forced_upgrade);
        grown.slot_operator_forced_upgrade.extend(forced);
        // Stale-binary triples as `slot_operator_mask_built` negatives. `neg` carries a
        // `[binary]` marker: Portage keys by package object (binary only), portuale's
        // string readers match both. Repeats replace nothing and take no `mask_order`
        // entry; mask-only growth still restarts (Portage sets `_need_restart` always).
        let mut masks_new = false;
        for (cat, pkg, ver) in &masked_binaries {
            let neg = format!("!={cat}/{pkg}-{ver}[binary]");
            let bucket = grown
                .runtime_pkg_mask
                .entry((cat.clone(), pkg.clone()))
                .or_default();
            if !bucket.iter().any(|m| m.neg == neg) {
                bucket.push(MaskEntry {
                    neg,
                    reason: MaskReason::SlotOperatorBuilt,
                });
                masks_new = true;
            }
        }
        if scheduled != grown.slot_operator_replace_installed || masks_new || forced_new {
            grown.slot_operator_replace_installed = scheduled;
            return ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }

        // Portage: depgraph.py, _resolve (prune_rebuilds): non-empty replace set plus
        // missed updates records `prune_rebuilds` and restarts once; the re-walk drops
        // the unnecessary rebuilds and re-schedules genuine ones above, terminating.
        //
        // The conflict-handler removals ride `pass.skipped_updates` (filed during this
        // pass's walk), so a non-empty row set here IS the conflict-source read; the
        // trigger is a bare OR over both halves. Gated on no live slot conflict: a
        // conflict-owning pass belongs to the conflict machinery above.
        if !grown.prune_rebuilds
            && !grown.slot_operator_replace_installed.is_empty()
            && pass.slot_conflicts.is_empty()
        {
            let (skipped, missing, full) = backtrack_missed_updates(BacktrackMissedUpdatesArgs {
                repos: &ctx.repos,
                root: ctx.root,
                config,
                entries: &pass.entries,
                params: grown,
                missing_args: &[],
            });
            if !skipped.is_empty()
                || !missing.is_empty()
                || !full.is_empty()
                || !pass.skipped_updates.is_empty()
            {
                apply_prune_rebuilds(&mut *grown);
                return ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(std::mem::take(grown)),
                }));
            }
        }

        // Portage: depgraph.py, _resolve (_eliminate_rebuilds), after the trigger settled:
        // each walked rebuild whose tree deps equal its installed deps leaves the set
        // (latched, so the scan can't re-add it) and walks as `AlreadyInstalled` next
        // pass. Budget-free. Skipped under `--emptytree` or a live slot conflict.
        if pass.slot_conflicts.is_empty() {
            let demoted = slot_operator_eliminate_rebuilds(SlotOperatorEliminateRebuildsArgs {
                root: ctx.root,
                repos: &ctx.repos,
                entries: &pass.entries,
                replace: &grown.slot_operator_replace_installed,
                slot_want: &pass.slot_want,
                reverse_pins,
                selective: ctx.selective,
                changed_slot: ctx.changed_slot,
                top_level_cps: &ctx.top_level_cps,
                empty: ctx.empty,
            });
            if !demoted.is_empty() {
                for cp in &demoted {
                    grown.slot_operator_replace_installed.remove(cp);
                    grown.slot_operator_undone.insert(cp.clone());
                }
                return ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(std::mem::take(grown)),
                }));
            }
        }
    }
    ControlFlow::Continue(has_nvc)
}
