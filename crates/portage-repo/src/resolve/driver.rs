//! The backtracking loop: one pass per node, settle or feed back.

use crate::*;
use portage_profile::Config;

pub(crate) fn backtracking_resolve(req: &ResolveRequest) -> Result<GraphResult, Error> {
    // Portage `_backtrack_depgraph`: pop a node, run one pass, settle or
    // feed back; `--backtrack=0` explores no feedback (the root pass settles).
    // A resolve never writes the VDB: validate VDB memos once, not per read.
    let _vdb_frozen = portage_vdb::freeze();
    let ctx = ResolveCtx::new(req)?;
    let initial = BacktrackParams::initial(req);
    let mut bt = Backtracker::new(
        std::cmp::max(1, ctx.backtrack_max.div_ceil(2)),
        initial.clone(),
    );
    let mut first_pass = true;
    // One per loop pass; `restarts` is Portage `_backtrack_depgraph`'s
    // `backtracked`: the passes that did not end the loop. The final
    // best-run re-pass after the loop shares the value.
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
        // Every loop pass is a try; a feed pass is not a retry.
        if !from_feed {
            passes += 1;
        }
        // A retry whose walk fails is a failed iteration, not a result; only
        // the root pass's own failure propagates. A feed pass is just dropped.
        let mut pass = match run_pass(&ctx, &params, first_pass) {
            Ok(pass) => pass,
            Err(_) if from_feed => continue,
            Err(_) if !first_pass => {
                bt.adopt_current(params.clone());
                // A failed iteration counts too, but only while nodes remain;
                // the cap is checked first.
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
                if !bt.unexplored.is_empty() {
                    restarts += 1;
                }
                continue;
            }
            Err(e) => return Err(e),
        };
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
        // The slot-conflict handler stops the search when a USE change would
        // solve this pass's conflicts. The root pass is then the report; a
        // later pass's stop is followed by a final pass from the best run
        // that no longer shows the suggestion.
        // Portage: depgraph.py, need_config_change (handler arm), _backtrack_depgraph
        if slot_conflict_handler_stops(&ctx, &pass, config) {
            if restarts > 0 {
                break;
            }
            let grown: BacktrackParams = match decision {
                PassDecision::Settle { params } | PassDecision::DeadEnd { params } => *params,
                PassDecision::Feedback(
                    BacktrackFeedback::Config { params }
                    | BacktrackFeedback::RevDep { params }
                    | BacktrackFeedback::Circular { params },
                ) => *params,
                PassDecision::Feedback(
                    BacktrackFeedback::SlotConflict { base, .. }
                    | BacktrackFeedback::MissingDep { base, .. },
                ) => *base,
            };
            let mut result = assemble_result(&ctx, &grown, pass, config, restarts);
            result.settled_with_backtracking = ctx.backtrack_max > 0;
            return Ok(result);
        }
        // With `--autounmask-backtrack` off, a pass whose graph carries an
        // autounmask change ends the loop: the pass is the report. The change
        // must reach the graph before the pass's stop: the replay records
        // only what its fail-fast walk reached, so a change it found came
        // before a missing-dependency failure. A fixable USE miss alone never
        // breaks: it sits after every package-less atom, so the replay meets
        // the missing dep first with nothing new recorded.
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
            PassDecision::Feedback(BacktrackFeedback::MissingDep { base, .. })
                if ctx.backtrack_max > 0
                    && !ctx.autounmask_backtrack_enabled
                    && let Some(r) = replay.as_ref()
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
                // conflict-free pass settles.
                if !result.missing_args.is_empty()
                    && !result.slot_conflicts.is_empty()
                    && !was_first
                    && !from_feed
                {
                    bt.adopt_current(*settled);
                    if !bt.unexplored.is_empty() {
                        restarts += 1;
                    }
                    continue;
                }
                return Ok(result);
            }
            PassDecision::DeadEnd { params } => {
                // The node is consumed but keeps the pass's merged accumulators
                // for the best-run re-pass below.
                bt.adopt_current(*params);
                // Same cap as the failed-iteration arm above; a dead end with
                // nodes left counts like any other failed pass.
                if restarts >= u64::from(ctx.backtrack_max) {
                    break;
                }
                if !bt.unexplored.is_empty() {
                    restarts += 1;
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
                // A restart counts even when every child it adds is rejected.
                // A grown autounmask config Portage would not restart for is
                // not a Portage pass and is not counted.
                let counts = grow_restart_counts(GrowRestartArgs {
                    ctx: &ctx,
                    params: &params,
                    pass: &pass,
                    kind: &kind,
                    replay: replay.as_ref(),
                });
                bt.feedback(kind);
                if counts {
                    restarts += 1;
                }
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
    let config_owned: Option<portage_profile::Config> = best.backtrack_config.clone();
    let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
    // The terminal node's accumulators predate the search: re-derive the
    // pass-local ones (dropped/enforced pins) through the settle path, so
    // the fallback report matches what the best params settle on their own
    // pass. Any other decision re-runs untouched, exactly as before.
    // Portage: _backtrack_depgraph falls back to the best run's own report.
    let mut probe = pass;
    let settled: Option<Box<BacktrackParams>> =
        match collect_feedback(rctx, &best, &mut probe, config) {
            PassDecision::Settle { params } => Some(params),
            _ => None,
        };
    // The settled params are owned; the untouched path re-runs and clones.
    let (pass, report): (PassResult, BacktrackParams) = match settled {
        Some(params) => (probe, *params),
        None => {
            // The terminal node's accumulators predate the search, and the
            // fired-but-unstuck masks live on discarded nodes: re-derive the
            // pin reports for the fallback, so an unhealable conflict still
            // aborts loudly instead of assembling silent rc 0. Selection is
            // already done, so only the report accumulators merge (never
            // `slot_constraints` or new masks).
            // Portage: _backtrack_depgraph falls back to the best run's own report.
            let naked = run_pass(rctx, &best, false)?;
            let mut report = best.clone();
            let live_reachable = live_reachable_consumers(rctx, &naked.entries);
            let (enforced, dropped) =
                reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
                    root: rctx.root,
                    entries: &naked.entries,
                    with_bdeps: rctx.with_bdeps,
                    excluded: rctx.excluded,
                    hard_want: &naked.slot_want,
                    reachable: &live_reachable,
                    dynamic_deps: rctx.dynamic_deps,
                    ignore_built_slot_operator_deps: rctx.ignore_built_slot_operator_deps,
                    top_level_cps: &rctx.top_level_cps,
                    world_reachable: &rctx.world_reachable,
                    config,
                    complete: rctx.complete,
                    portage_complete: rctx.portage_complete(&naked.entries),
                    installed_walked: &naked.installed_walked,
                    update: rctx.update,
                    selective: rctx.selective && !rctx.empty,
                    backtrack_allowed: rctx.backtrack_max > 0,
                    arg_atoms: rctx.atoms,
                    root_deps_running_root: rctx.root_deps_running_root,
                    gate: rctx.gate(),
                });
            for pin in enforced {
                if !report.reverse_dep_pins.contains(&pin) {
                    report.reverse_dep_pins.push(pin);
                }
            }
            for pin in dropped {
                if !report.dropped_pins.contains(&pin) {
                    report.dropped_pins.push(pin);
                }
            }
            (naked, report)
        }
    };
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
    let prune_unresolved = prune_unresolved_after_backtrack(restarts, &ctx, &report, &pass, config);
    let mut result = assemble_result(rctx, &report, pass, config, restarts);
    result.prune_unresolved = prune_unresolved;
    result.backtrack_max = ctx.backtrack_max;
    Ok(result)
}

/// Arguments of [`grow_restart_counts`].
struct GrowRestartArgs<'a> {
    ctx: &'a ResolveCtx<'a>,
    params: &'a BacktrackParams,
    pass: &'a PassResult,
    kind: &'a BacktrackFeedback,
    replay: Option<&'a merge_order::WalkReplay>,
}

/// Whether this feedback restarts the search the way Portage would: every
/// feedback counts except a grown autounmask config Portage would not restart
/// for (a flip its fail-fast walk never reaches, or one that changes nothing
/// Portage checks).
// Portage: depgraph.py, _backtrack_depgraph; _pkg_use_enabled's
// want_restart_for_use_change
fn grow_restart_counts(args: GrowRestartArgs<'_>) -> bool {
    let GrowRestartArgs {
        ctx,
        params,
        pass,
        kind,
        replay,
    } = args;
    let BacktrackFeedback::Config { params: grown } = kind else {
        return true;
    };
    // Only the grow arm can be a pass Portage would not take: the breakage
    // arm disables the suggestions, and a filed probe restart counts.
    if !pass.autounmask_grew
        || (grown.autounmask_disabled && !params.autounmask_disabled)
        || grown.slot_operator_replace_installed != params.slot_operator_replace_installed
        || grown.probe_replaced != params.probe_replaced
    {
        return true;
    }
    let Some(replay) = replay else { return true };
    // Portage restarts for a USE change only when its walk completes: a
    // failed walk files no restart (it is a failed pass, not a restart).
    if replay.failure.is_some() {
        return false;
    }
    let changes = &grown.autounmask_use_config;
    pass.use_overlay.iter().any(|(cp, flags)| {
        flags.iter().any(|(flag, want)| {
            // Reached: the fail-fast replay folded the same flip before its
            // failure; otherwise Portage's walk never got there.
            if replay.needed.get(cp).and_then(|m| m.get(flag)) != Some(want) {
                return false;
            }
            flip_wants_restart(FlipRestartArgs {
                ctx,
                pass,
                cp,
                changes,
            })
        })
    })
}

/// Arguments of [`flip_wants_restart`].
struct FlipRestartArgs<'a> {
    ctx: &'a ResolveCtx<'a>,
    pass: &'a PassResult,
    cp: &'a (String, String),
    changes: &'a HashMap<(String, String), HashMap<String, bool>>,
}

/// Portage's `want_restart_for_use_change` on one flipped package: its
/// flattened dependencies or license change under the new USE, or the change
/// breaks a USE dep of one of its existing parents. Anything unprovable
/// restarts, like before.
// Portage: depgraph.py, _pkg_use_enabled
fn flip_wants_restart(args: FlipRestartArgs<'_>) -> bool {
    let FlipRestartArgs {
        ctx,
        pass,
        cp,
        changes,
    } = args;
    if flattened_deps_change(ctx, pass, cp, changes) {
        return true;
    }
    parent_use_dep_breaks(pass, cp, changes)
}

/// Whether any merge-bound version of `cp` flattens its dependencies or
/// license differently under the flipped USE.
fn flattened_deps_change(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    cp: &(String, String),
    changes: &HashMap<(String, String), HashMap<String, bool>>,
) -> bool {
    let empty: HashMap<String, bool> = HashMap::new();
    let changed = changes.get(cp).unwrap_or(&empty);
    let mut versions: Vec<(String, Option<String>)> = Vec::new();
    for e in &pass.entries {
        if (e.category.clone(), e.package.clone()) != *cp {
            continue;
        }
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                Some(version.clone())
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some(to.clone())
            }
            _ => None,
        };
        if let Some(version) = version
            && !versions.iter().any(|(v, _)| v == &version)
        {
            versions.push((version, e.repo_name.clone()));
        }
    }
    if versions.is_empty() {
        return true;
    }
    versions.iter().any(|(version, repo)| {
        let Ok(cands) = ctx.repo_candidates(&cp.0, &cp.1) else {
            return true;
        };
        let Some(cand) = cands
            .iter()
            .find(|c| &c.version == version && repo.as_deref().is_none_or(|r| r == c.repo_name))
        else {
            return true;
        };
        let old_use = effective_use_flags(EffectiveUseFlagsArgs {
            config: ctx.config,
            iuse: &cand.iuse,
            keywords: &cand.keywords,
            candidate_str: &cand.match_str(&cp.0, &cp.1),
            build_id: cand.build_id.as_deref(),
            category: &cp.0,
            package: &cp.1,
        });
        let mut new_use: HashSet<String> = old_use.as_ref().clone();
        for (flag, want) in changed {
            if *want {
                new_use.insert(flag.clone());
            } else {
                new_use.remove(flag);
            }
        }
        let pf = format!("{}-{version}", cp.1);
        let Ok(metadata) = repo_aux_metadata(&cand.repo_location, &cp.0, &pf) else {
            return true;
        };
        [
            "BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND", "LICENSE",
        ]
        .iter()
        .any(|key| {
            let text = metadata.get(*key).map(String::as_str).unwrap_or("");
            let tokens: Vec<String> = text.split_whitespace().map(str::to_string).collect();
            let old_flat = portage_use_reduce::use_reduce_flat(
                &tokens,
                &old_use,
                portage_use_reduce::MatchMode::Normal,
            );
            let new_flat = portage_use_reduce::use_reduce_flat(
                &tokens,
                &new_use,
                portage_use_reduce::MatchMode::Normal,
            );
            match (old_flat, new_flat) {
                (Ok(old_flat), Ok(new_flat)) => {
                    let mut old_sorted = old_flat;
                    let mut new_sorted = new_flat;
                    old_sorted.sort();
                    new_sorted.sort();
                    old_sorted != new_sorted
                }
                _ => true,
            }
        })
    })
}

/// Whether the flipped USE breaks a USE dep of an existing parent: for every
/// edge landing on `cp`, a mentioned flag whose required state mismatches the
/// change restarts. Unparseable and conditional atoms restart, like before.
// Portage: depgraph.py, _pkg_use_enabled
fn parent_use_dep_breaks(
    pass: &PassResult,
    cp: &(String, String),
    changes: &HashMap<(String, String), HashMap<String, bool>>,
) -> bool {
    let empty: HashMap<String, bool> = HashMap::new();
    let changed = changes.get(cp).unwrap_or(&empty);
    if changed.is_empty() {
        return false;
    }
    pass.entries.iter().any(|e| {
        e.deps.iter().any(|d| {
            if d.category != cp.0 || d.package != cp.1 {
                return false;
            }
            let Some(atom) = parse_dep_atom(&d.atom, "grow restart count") else {
                return true;
            };
            let Some(use_deps) = atom.use_deps.as_deref() else {
                return false;
            };
            let mut enabled: HashSet<String> = HashSet::new();
            let mut disabled: HashSet<String> = HashSet::new();
            for dep in use_deps {
                match dep.op {
                    portage_dep::UseDepOp::Enabled => {
                        enabled.insert(dep.flag.clone());
                    }
                    portage_dep::UseDepOp::Disabled => {
                        disabled.insert(dep.flag.clone());
                    }
                    _ => return true,
                }
            }
            changed.iter().any(|(flag, want)| {
                let want_enabled = enabled.contains(flag);
                (want_enabled || disabled.contains(flag)) && want_enabled != *want
            })
        })
    })
}

/// Whether unresolved dependencies are pruned from the final result after a backtrack.
pub(crate) fn prune_unresolved_after_backtrack(
    restarts: u64,
    ctx: &ResolveCtx<'_>,
    best: &BacktrackParams,
    pass: &PassResult,
    config: &Config,
) -> bool {
    restarts > 0
        && ctx.backtrack_max > 0
        && !best.prune_rebuilds
        && !best.slot_operator_replace_installed.is_empty()
        && pass.slot_conflicts.is_empty()
        && {
            let (skipped, missing, full) = backtrack_missed_updates(BacktrackMissedUpdatesArgs {
                root: ctx.root,
                config,
                entries: &pass.entries,
                params: best,
                missing_args: &[],
                gate: ctx.gate(),
            });
            !skipped.is_empty()
                || !missing.is_empty()
                || !full.is_empty()
                || !pass.skipped_updates.is_empty()
        }
}

/// Whether the slot-conflict handler finds a USE change for the conflicts of
/// `pass`, which ends the backtracking search. The handler only runs when the
/// pass may not accept its blocker conflicts.
// Portage: depgraph.py, need_config_change; _accept_blocker_conflicts
fn slot_conflict_handler_stops(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    config: &portage_profile::Config,
) -> bool {
    if pass.slot_conflicts.is_empty()
        || (ctx.backtrack_max == 0 && (ctx.buildpkgonly || ctx.nodeps))
    {
        return false;
    }
    !solve_slot_conflict_records(SolveRecordsArgs {
        conflicts: &pass.slot_conflicts,
        config,
        root: ctx.root,
        gate: ctx.gate(),
    })
    .changes
    .is_empty()
}
