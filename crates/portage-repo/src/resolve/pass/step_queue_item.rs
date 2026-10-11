//! Process one queued atom through resolve, graph and enqueue.

use crate::resolve::pass::*;
use portage_profile::Config;

/// The values [`step_queue_item`] reads from its caller.
pub(crate) struct StepQueueItemArgs<'a> {
    pub(crate) current_atom: String,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) depth: u32,
    pub(crate) item_owner_targets_running_root: bool,
    pub(crate) item_targets_running_root: bool,
    pub(crate) buildtime_hard: bool,
    pub(crate) from_disjunction: bool,
    pub(crate) first_pass: bool,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) pprovided_refs: &'a [&'a str],
    pub(crate) state: &'a mut PassState,
}

/// Process one queued atom through resolve, graph and enqueue.
///
/// One `'queue` loop iteration: prepare and dedup, resolve the outcome,
/// record the parent row, graph the resolved version and enqueue its
/// dependencies. `Break` drops the node.
pub(crate) fn step_queue_item(args: StepQueueItemArgs<'_>) -> Result<ControlFlow<()>, Error> {
    let StepQueueItemArgs {
        current_atom,
        owner,
        unevaluated_atom,
        depth,
        item_owner_targets_running_root,
        item_targets_running_root,
        buildtime_hard,
        from_disjunction,
        first_pass,
        ctx,
        config,
        bp,
        union_constraints,
        pprovided_refs,
        state,
    } = args;
    let ControlFlow::Continue(PrepareQueuedAtomOut {
        key,
        atom_root,
        atom,
    }) = prepare_queued_atom(PrepareQueuedAtomArgs {
        current_atom: &current_atom,
        owner,
        pprovided_refs,
        depth,
        state: &mut *state,
        item_targets_running_root,
        ctx,
        buildtime_hard,
        item_owner_targets_running_root,
        config,
        unevaluated_atom,
        first_pass,
    })
    else {
        return Ok(ControlFlow::Break(()));
    };

    // After a solvable slot conflict every parent atom targeting this `cat/pkg`
    // enforces together, so one version satisfies all and the conflict clears.
    let ControlFlow::Continue(outcome) = resolve_initial_outcome(ResolveInitialOutcomeArgs {
        union_constraints,
        key: &key,
        ctx,
        config,
        depth,
        bp,
        owner,
        from_disjunction,
        state: &mut *state,
        current_atom: &current_atom,
        buildtime_hard,
        item_targets_running_root,
        atom_root,
    })?
    else {
        return Ok(ControlFlow::Break(()));
    };
    let mut outcome = apply_outcome_fallbacks(ApplyOutcomeFallbacksArgs {
        outcome,
        current_atom: &current_atom,
        atom: &atom,
        key: &key,
        ctx,
        config,
        bp,
        union_constraints,
        state: &mut *state,
        atom_root,
        item_targets_running_root,
        owner,
        depth,
        buildtime_hard,
        from_disjunction,
    })?;

    // Portage `_add_pkg`'s per-instance parent-atom record, taken after the
    // outcome flips and before the dedup below, so the child is what this
    // atom resolved to. A shadowed `AlreadyInstalled` keeps its own row.
    // Portage: depgraph.py, _add_parent_atom.
    record_parent_atom(RecordParentAtomArgs {
        outcome: &outcome,
        key: &key,
        owner,
        current_atom: &current_atom,
        unevaluated_atom,
        state: &mut *state,
    });

    // A dependency whose use-dep was conditional on the parent's USE and no
    // candidate satisfies the evaluated form: flip the parent's flag instead
    // (recorded, or folded for a backtrack re-walk). A probe re-resolution
    // gates both arms, so only a helping flip records. See `parent_use_flip`.
    // Portage: depgraph.py (_apply_parent_use_changes, _success_without_autounmask).
    let ControlFlow::Continue((current_atom, atom)) = parent_use_flip(ParentUseFlipArgs {
        outcome: &outcome,
        bp,
        depth,
        current_atom,
        atom,
        owner,
        unevaluated_atom,
        ctx,
        state: &mut *state,
        config,
        key: &key,
    })?
    else {
        return Ok(ControlFlow::Break(()));
    };

    // `--changed-deps-report` stays silent with `--changed-deps` (Portage
    // discards its own dict unread), so skip computing `deps_changed` at all
    // in that case. Only installed-naming outcomes qualify; a `Reinstall`
    // here is always for another trigger, so it still fires independently.
    record_changed_deps_report(ctx, &outcome, &key, state);

    if step_top_level_unsatisfied(StepTopLevelUnsatisfiedArgs {
        ctx,
        current_atom: &current_atom,
        outcome: &mut outcome,
        state: &mut *state,
        union_constraints,
        key: &key,
        config,
        unevaluated_atom,
        bp,
        atom: &atom,
    })?
    .is_break()
    {
        return Ok(ControlFlow::Break(()));
    }

    let ControlFlow::Continue(SelectResolvedVersionOut {
        version,
        outcome,
        key,
    }) = select_resolved_version(SelectResolvedVersionArgs {
        current_atom: &current_atom,
        bp,
        key,
        outcome,
        ctx,
        state: &mut *state,
        config,
        owner,
        atom_root,
        item_targets_running_root,
        unevaluated_atom,
        depth,
        union_constraints,
        atom: &atom,
    })
    else {
        return Ok(ControlFlow::Break(()));
    };

    let ControlFlow::Continue(repo_candidates) = candidate_pool(CandidatePoolArgs {
        ctx,
        key: &key,
        config,
        current_atom: &current_atom,
        atom: &atom,
        bp,
    }) else {
        return Ok(ControlFlow::Break(()));
    };
    let ControlFlow::Continue(graphed) = graph_resolved_entry(GraphResolvedEntryArgs {
        repo_candidates: &repo_candidates,
        version: &version,
        key: &key,
        item_targets_running_root,
        state: &mut *state,
        ctx,
        current_atom: &current_atom,
        atom: &atom,
        config,
        bp,
        owner,
        union_constraints,
        depth,
        outcome,
    }) else {
        return Ok(ControlFlow::Break(()));
    };
    if enqueue_graphed_entry(EnqueueGraphedEntryArgs {
        ctx,
        config,
        bp,
        state: &mut *state,
        key: &key,
        version: &version,
        current_atom: &current_atom,
        atom: &atom,
        unevaluated_atom,
        owner,
        depth,
        union_constraints,
        repo_candidates: &repo_candidates,
        graphed,
    })?
    .is_break()
    {
        return Ok(ControlFlow::Break(()));
    }
    Ok(ControlFlow::Continue(()))
}
