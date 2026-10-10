//! The backtracking loop: one pass per node, settle or feed back.

use crate::*;

pub(crate) fn backtracking_resolve(req: &ResolveRequest) -> Result<GraphResult, Error> {
    // Portage `_backtrack_depgraph`: pop a node, run one pass, settle or
    // feed back; `--backtrack=0` explores no feedback (the root pass settles).
    let ctx = ResolveCtx::new(req)?;
    let initial = BacktrackParams::initial(req);
    let mut bt = Backtracker::new(
        std::cmp::max(1, ctx.backtrack_max.div_ceil(2)),
        initial.clone(),
    );
    let mut first_pass = true;
    // One per loop iteration beyond the first; the final best-run re-pass
    // shares the value.
    let mut passes: u64 = 0;
    let mut restarts: u64 = 0;
    // The `--backtrack=0` reverse-dependency feed: a satisfiable
    // installed-consumer pin enforces in-pass even with backtracking off.
    // A feed pass is not a retry (`restarts` stays 0).
    let mut feed_params: Option<BacktrackParams> = None;
    // USE changes `replay_walk` found needed on earlier passes.
    let mut replay_needed: HashMap<(String, String), HashMap<String, bool>> = HashMap::new();
    loop {
        let from_feed = feed_params.is_some();
        let Some(params) = feed_params.take().or_else(|| bt.get()) else {
            break;
        };
        // A retry whose walk fails is a failed iteration, not a result; only
        // the root pass's own failure propagates. A feed pass is just dropped.
        let mut pass = match run_pass(&ctx, &params, first_pass) {
            Ok(pass) => pass,
            Err(_) if from_feed => continue,
            Err(_) if !first_pass => {
                bt.adopt_current(params.clone());
                // A failed iteration counts too; the cap is checked first (#297).
                if passes >= u64::from(ctx.backtrack_max) {
                    break;
                }
                if !bt.unexplored.is_empty() {
                    passes += 1;
                    restarts = passes - 1;
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        if !from_feed {
            passes += 1;
            restarts = passes - 1;
        }
        let was_first = first_pass;
        first_pass = false;
        // The per-pass config view: the accumulator's autounmask-use changes
        // layered on as the top USE tier; cloned so the calls below can
        // borrow alongside it.
        let config_owned: Option<portage_profile::Config> = params.backtrack_config.clone();
        let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
        // The clean pass after an autounmask breakage is reported as it
        // stands, cut to what the fail-fast walk reached.
        if params.autounmask_disabled {
            return run_breakage_clean_pass(&ctx, &initial, restarts);
        }
        // The walk over this pass with the USE changes it needs; a flip
        // contradicting one already needed fails the pass into one clean pass.
        let mut needed_init = params.autounmask_use_config.clone();
        for (cp, flags) in &replay_needed {
            let m = needed_init.entry(cp.clone()).or_default();
            for (f, on) in flags {
                m.entry(f.clone()).or_insert(*on);
            }
        }
        // With `--autounmask-backtrack` off only the first pass carrying an
        // autounmask change is replayed.
        let replay = (params.autounmask_suggest_use
            && (ctx.autounmask_backtrack_enabled || passes == 1)
            && (pass_has_use_changes(&params, &pass)
                || !pass.slot_conflicts.is_empty()
                || !replay_needed.is_empty()))
        .then(|| replay_walk(&ctx, &pass.entries, true, &needed_init));
        // The changes this walk needed carry into every later try.
        if ctx.autounmask_backtrack_enabled
            && let Some(r) = replay.as_ref()
        {
            for (cp, flags) in &r.needed {
                let m = replay_needed.entry(cp.clone()).or_default();
                for (f, on) in flags {
                    m.entry(f.clone()).or_insert(*on);
                }
            }
        }
        if replay
            .as_ref()
            .is_some_and(|r| r.failure.as_ref().is_some_and(|f| f.contradiction))
        {
            return run_breakage_clean_pass(&ctx, &initial, restarts);
        }
        let decision = collect_feedback(&ctx, &params, &mut pass, config);
        // With `--autounmask-backtrack` off, a pass whose graph carries an
        // autounmask change ends the loop: the pass is the report.
        let decision = match decision {
            PassDecision::Feedback(BacktrackFeedback::SlotConflict { base, .. })
                if ctx.backtrack_max > 0
                    && !ctx.autounmask_backtrack_enabled
                    && let Some(r) = replay.as_ref()
                    && r.failure.is_none()
                    && r.needed != needed_init =>
            {
                apply_replay_flips(
                    &ctx,
                    &mut pass.entries,
                    r,
                    &params.autounmask_use_config,
                    &mut pass.autounmask_use_changes,
                );
                let entries = pass.entries.clone();
                let slot_conflicts = std::mem::take(&mut pass.slot_conflicts);
                let mut result = assemble_result(&ctx, &base, pass, config, restarts);
                result.slot_conflicts = slot_conflicts;
                result.settled_with_backtracking = true;
                replay_slot_conflicts(&ctx, &entries, r, &mut result.slot_conflicts);
                return Ok(result);
            }
            d => d,
        };
        match decision {
            PassDecision::Settle { params } => {
                let settled = params.clone();
                let result = assemble_result(&ctx, &params, pass, config, restarts);
                // A retry pass that dropped a set member and still reports a
                // slot conflict is a failed iteration; a drop on a
                // conflict-free pass settles (#276, #286).
                if !result.missing_args.is_empty()
                    && !result.slot_conflicts.is_empty()
                    && !was_first
                    && !from_feed
                {
                    bt.adopt_current(*settled);
                    continue;
                }
                return Ok(result);
            }
            PassDecision::DeadEnd { params } => {
                // The node is consumed but keeps the pass's merged accumulators
                // for the best-run re-pass below.
                bt.adopt_current(*params);
                // Same cap as the failed-iteration arm above (#297).
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
            }
            PassDecision::Feedback(kind) => {
                if ctx.backtrack_max == 0 {
                    // No search: report the root pass with its merged working
                    // copy -- except the feed above, which enforces satisfiable
                    // reverse-dep pins in-pass.
                    match kind {
                        BacktrackFeedback::RevDep { params: grown } if !ctx.nodeps => {
                            feed_params = Some(*grown);
                            continue;
                        }
                        kind => {
                            let grown: BacktrackParams = match kind {
                                BacktrackFeedback::Config { params }
                                | BacktrackFeedback::RevDep { params }
                                | BacktrackFeedback::Circular { params } => *params,
                                BacktrackFeedback::SlotConflict { base, .. } => *base,
                                BacktrackFeedback::MissingDep { base, .. } => *base,
                            };
                            return Ok(assemble_result(&ctx, &grown, pass, config, restarts));
                        }
                    }
                }
                // Break when the restart budget is spent; fall through to the
                // best-run re-pass after the loop.
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
                bt.feedback(kind);
            }
        }
    }
    // Exhausted without settling: report the deepest terminal node, never a
    // masked-by-backtracking graph unless nothing better exists.
    let best = bt.get_best_run();
    // Re-run the best params with backtracking off once any restart happened.
    let req0;
    let ctx0;
    let rctx: &ResolveCtx = if restarts > 0 && ctx.backtrack_max > 0 {
        req0 = ResolveRequest {
            backtrack_max: 0,
            ..req.clone()
        };
        ctx0 = ResolveCtx::new(&req0)?;
        &ctx0
    } else {
        &ctx
    };
    let pass = run_pass(rctx, &best, false)?;
    // With the search's accumulated changes, a dependency wanting a flag the
    // other way from a needed change means one clean pass instead.
    let mut best_needed = best.autounmask_use_config.clone();
    for (cp, flags) in &replay_needed {
        let m = best_needed.entry(cp.clone()).or_default();
        for (f, on) in flags {
            m.entry(f.clone()).or_insert(*on);
        }
    }
    if !best.autounmask_disabled
        && best.autounmask_suggest_use
        && (pass_has_use_changes(&best, &pass)
            || !pass.slot_conflicts.is_empty()
            || !replay_needed.is_empty())
        && replay_walk(&ctx, &pass.entries, true, &best_needed)
            .failure
            .is_some_and(|f| f.contradiction)
    {
        return run_breakage_clean_pass(&ctx, &initial, restarts);
    }
    let config_owned: Option<portage_profile::Config> = best.backtrack_config.clone();
    let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
    let prune_unresolved = restarts > 0
        && ctx.backtrack_max > 0
        && !best.prune_rebuilds
        && !best.slot_operator_replace_installed.is_empty()
        && pass.slot_conflicts.is_empty()
        && {
            let (skipped, missing, full) = backtrack_missed_updates(BacktrackMissedUpdatesArgs {
                repos: &ctx.repos,
                root: ctx.root,
                config,
                entries: &pass.entries,
                params: &best,
                missing_args: &[],
            });
            !skipped.is_empty()
                || !missing.is_empty()
                || !full.is_empty()
                || !pass.skipped_updates.is_empty()
        };
    let mut result = assemble_result(rctx, &best, pass, config, restarts);
    result.prune_unresolved = prune_unresolved;
    result.backtrack_max = ctx.backtrack_max;
    Ok(result)
}
