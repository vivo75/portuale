//! The backtracking loop.

use crate::*;

pub(crate) fn backtracking_resolve(req: &ResolveRequest) -> Result<GraphResult, Error> {
    // Phase C2 (023): real `_backtrack_depgraph` -- pop a node, run one
    // pass, settle or feed the outcome back, and when the search is
    // exhausted re-run with the deepest terminal (config-only) params.
    // `--backtrack=0` explores no feedback at all (real's
    // `_allow_backtracking` gate): the root pass settles as-is.
    let ctx = ResolveCtx::new(req)?;
    let initial = BacktrackParams::initial(req);
    let mut bt = Backtracker::new(
        std::cmp::max(1, ctx.backtrack_max.div_ceil(2)),
        initial.clone(),
    );
    let mut first_pass = true;
    // #59 S2: real `_backtrack_depgraph`'s `backtracked` counter -- one
    // per loop iteration beyond the first (`passes - 1`). The final
    // best-run re-pass after the loop shares the value, exactly as
    // real's counter is frozen before its own best-run re-pass.
    let mut passes: u64 = 0;
    let mut restarts: u64 = 0;
    // Backlog #209: the `--backtrack=0` reverse-dependency feed loop.
    // Real `_resolve_conflicts` (`_emerge/depgraph.py:9444`) runs
    // `_complete_graph()` (`:8562`) with no `_allow_backtracking` gate
    // (that flag only gates auto-enabling `complete` mode on slot
    // conflicts at `:9444-9450`, and the slot-op trigger at `:2131`),
    // so a satisfiable installed-consumer pin enforces in-pass even
    // with backtracking off. Portuale's scan runs post-pass, so its
    // `RevDep` working copy feeds one more pass here instead of
    // settling -- a feed loop, not a backtrack retry (`restarts` stays
    // 0, real `backtrack: 0/0`). Bounded: `reverse_dep_masked` latches
    // every enforced `(cp, atom)`, so a pass that adds nothing new
    // falls through to `Settle`. `--nodeps` is excluded: real pops
    // `recurse` there (`create_depgraph_params.py:181-183`), so its
    // complete graph never runs.
    let mut feed_params: Option<BacktrackParams> = None;
    // Backlog #244: the USE changes `replay_walk` found needed on earlier
    // passes (see the `_feedback_config` note in the loop).
    let mut replay_needed: HashMap<(String, String), HashMap<String, bool>> = HashMap::new();
    loop {
        let from_feed = feed_params.is_some();
        let Some(params) = feed_params.take().or_else(|| bt.get()) else {
            break;
        };
        // Real `_backtrack_depgraph`: a retry whose walk fails (a masked
        // top-level select, backlog #198's single-version peel) is a
        // failed iteration, not a result -- the search continues and an
        // exhausted search falls back to the best run. Only the root
        // pass's own failure propagates (nothing to fall back to). A
        // feed pass has no node to adopt into, so it is just dropped.
        let mut pass = match run_pass(&ctx, &params, first_pass) {
            Ok(pass) => pass,
            Err(_) if from_feed => continue,
            Err(_) if !first_pass => {
                bt.adopt_current(params.clone());
                // Real counts a failed iteration without feedback too
                // (`elif backtracker: backtracked += 1`) and checks the
                // cap first (`backtracked >= max_retries`) (#297).
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
        // The per-pass config view: the accumulator's autounmask-use
        // changes layered on as the top USE tier (see `Config::
        // autounmask_use`). Cloned (not borrowed) out of the node params
        // so the decision/assembly calls below can borrow alongside it;
        // the clone only allocates on autounmask-growth passes.
        let config_owned: Option<portage_profile::Config> = params.backtrack_config.clone();
        let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
        // Backlog #244: the clean pass after an autounmask breakage is
        // real's last pass (`allow_backtracking=False`) -- reported as
        // it stands, cut to what the fail-fast walk reached.
        if params.autounmask_disabled {
            return run_breakage_clean_pass(&ctx, &initial, restarts);
        }
        // Backlog #244: real's own walk over this pass, with the USE
        // changes it needs (`_needed_use_config_changes`). A flip that
        // contradicts one already needed fails the pass right there
        // (`depgraph.py:7714`) -- no slot-conflict or config feedback is
        // ever computed for it (`_resolve` returns before
        // `_process_slot_conflicts`), and the failure is an autounmask
        // breakage (`autounmask_breakage_detected`, `:11779`): one clean
        // pass follows (`:12262-12280`).
        let mut needed_init = params.autounmask_use_config.clone();
        for (cp, flags) in &replay_needed {
            let m = needed_init.entry(cp.clone()).or_default();
            for (f, on) in flags {
                m.entry(f.clone()).or_insert(*on);
            }
        }
        // With `--autounmask-backtrack` off real stops after the first
        // pass that carries an autounmask change (`need_config_change`),
        // so only that pass is replayed; later passes portuale runs for
        // other feedback have no real counterpart to mirror.
        let replay = (params.autounmask_suggest_use
            && (ctx.autounmask_backtrack_enabled || passes == 1)
            && (pass_has_use_changes(&params, &pass)
                || !pass.slot_conflicts.is_empty()
                || !replay_needed.is_empty()))
        .then(|| replay_walk(&ctx, &pass.entries, true, &needed_init));
        // Real `_feedback_config` (`--autounmask-backtrack=y`): the changes
        // this walk needed carry into every later try, including the ones
        // portuale's own pass never recorded (its walk skips the flip
        // when the requester first lands on another instance of the
        // slot).
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
        // Backlog #244: real `need_config_change` (`depgraph.py:11708`):
        // with `--autounmask-backtrack` off, a pass whose graph carries
        // an autounmask change ends the loop before any slot-conflict
        // backtracking (`_autounmask_backtrack_disabled`) -- the pass is
        // the report, conflict block, change block and "terminated
        // early" notice together.
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
                // Backlog #286: a retry pass that dropped a set member
                // and still reports a slot conflict is a failed
                // iteration, like the abort the drop replaced -- the
                // search goes on to masks that settle cleanly (`btnr`:
                // masking the world-selected `btrb` ends in the very
                // conflict the bound mask was meant to avoid). A drop
                // on an otherwise conflict-free pass settles (#276).
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
                // Real's abandoned `_create_graph` attempt: the node is
                // consumed, but it keeps the pass's merged accumulators
                // so a best-run re-pass reports the same USE changes and
                // dropped pins the abandoned pass saw. The search itself
                // continues elsewhere; the report comes from the best
                // run below.
                bt.adopt_current(*params);
                // Same cap as the failed-iteration arm above (#297).
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
            }
            PassDecision::Feedback(kind) => {
                if ctx.backtrack_max == 0 {
                    // No search: report the root pass with its merged
                    // working copy, exactly like the old single pass --
                    // except the #209 feed above, which enforces
                    // satisfiable reverse-dep pins in-pass like real's
                    // ungated `_complete_graph`.
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
                // Real `_backtrack_depgraph` breaks when `backtracked >=
                // max_retries` before feeding back; `restarts` is the
                // number of restarts already done (passes-1); breaking
                // falls through to the best-run re-pass after the loop.
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
                bt.feedback(kind);
            }
        }
    }
    // Exhausted without settling: the best run (deepest terminal node)
    // is reported, never a masked-by-backtracking graph unless nothing
    // better exists (real `get_best_run`).
    let best = bt.get_best_run();
    // Real re-runs the best params with `allow_backtracking=False` once
    // any restart happened (`backtracked`): every `--backtrack=0` gate
    // applies to that last pass (#295).
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
    // Backlog #244: real's post-loop `autounmask_breakage_detected()`
    // (`depgraph.py:12262`): with the changes the search accumulated, a
    // dependency of the best run wants a flag the other way from a
    // needed change -- one clean pass instead.
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
            let (skipped, missing, full) =
                backtrack_missed_updates(&ctx.repos, ctx.root, config, &pass.entries, &best, &[]);
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
