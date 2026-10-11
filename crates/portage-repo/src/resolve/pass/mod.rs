//! One resolver pass over the graph.

use crate::*;
use std::ops::ControlFlow;

mod assemble_pass;
mod candidate_pool;
mod changed_deps_report;
mod collect_pass_blockers;
mod enqueue_deps;
mod enqueue_graphed;
mod entry_fetch;
mod entry_use_display;
mod entry_use_flags;
mod existing_slot;
mod finish_required_by;
mod finish_slot_conflicts;
mod graph_entry;
mod graph_resolved;
mod initial_outcome;
mod outcome_fallbacks;
mod parent_record;
mod parent_use_flip;
mod pop_queue_item;
mod prepare_atom;
mod resolve_outcome;
mod resolved_version;
mod seed_queue;
mod slot_conflicts;
mod step_queue_item;
mod top_level_unsatisfied;
mod unresolved_dep;
pub(crate) use assemble_pass::*;
pub(crate) use candidate_pool::*;
pub(crate) use changed_deps_report::*;
pub(crate) use collect_pass_blockers::*;
pub(crate) use enqueue_deps::*;
pub(crate) use enqueue_graphed::*;
pub(crate) use entry_fetch::*;
pub(crate) use entry_use_display::*;
pub(crate) use entry_use_flags::*;
pub(crate) use existing_slot::*;
pub(crate) use finish_required_by::*;
pub(crate) use finish_slot_conflicts::*;
pub(crate) use graph_entry::*;
pub(crate) use graph_resolved::*;
pub(crate) use initial_outcome::*;
pub(crate) use outcome_fallbacks::*;
pub(crate) use parent_record::*;
pub(crate) use parent_use_flip::*;
pub(crate) use pop_queue_item::*;
pub(crate) use prepare_atom::*;
pub(crate) use resolve_outcome::*;
pub(crate) use resolved_version::*;
pub(crate) use seed_queue::*;
pub(crate) use slot_conflicts::*;
pub(crate) use step_queue_item::*;
pub(crate) use top_level_unsatisfied::*;
pub(crate) use unresolved_dep::*;

/// One full BFS walk over the current `bp` (Phase A3). Takes `&bp`, never
/// `&mut bp`: flips record into the pass-local overlay, which `collect_feedback`
/// and the driver fold into `bp` before deciding. The `REQUIRED_USE` early-`Err`
/// is the only failure mode.
pub(crate) fn run_pass(
    ctx: &ResolveCtx,
    bp: &BacktrackParams,
    first_pass: bool,
) -> Result<PassResult, Error> {
    // The effective config for this pass (cloned out of `bp`, so only `&bp`
    // is borrowed here).
    let config_owned: Option<portage_profile::Config> = bp.backtrack_config.clone();
    let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
    let mut state = PassState::default();
    // Pass-constant: `bp` is never mutated mid-walk (only the overlay).
    let union_constraints = bp.union_constraints();
    if resolver_debug() && first_pass {
        for a in ctx.atoms {
            resolver_trace::tr!("\n      Arg: {a}\n     Atom: {a}\n");
        }
    }
    seed_queue(ctx, &mut state, bp);
    state.seeds_left = state.queue.len();

    let pprovided_refs: Vec<&str> = config.package_provided.iter().map(String::as_str).collect();

    // Portage pops its `_dep_stack` LIFO, so the last argument's whole subtree
    // walks before the previous argument's deps are looked at; a FIFO queue
    // would interleave the subtrees. Children each seed enqueues drain as one
    // block, in seed order. Portage: depgraph.py, _create_graph
    while let ControlFlow::Continue(QueueItem {
        atom: current_atom,
        depth,
        owner,
        owner_targets_running_root: item_owner_targets_running_root,
        unevaluated: unevaluated_atom,
        targets_running_root: item_targets_running_root,
        buildtime_hard,
        from_disjunction,
    }) = pop_queue_item(&mut state)
    {
        if step_queue_item(StepQueueItemArgs {
            current_atom,
            owner: &owner,
            unevaluated_atom: &unevaluated_atom,
            depth,
            item_owner_targets_running_root,
            item_targets_running_root,
            buildtime_hard,
            from_disjunction,
            first_pass,
            ctx,
            config,
            bp,
            union_constraints: &union_constraints,
            pprovided_refs: &pprovided_refs,
            state: &mut state,
        })?
        .is_break()
        {
            continue;
        }
    }

    // `.get()`, not `.remove()`: several entries can share one `(category,
    // package)` (one per slot), and every one needs the same `required_by`.
    // A `.remove()` left all but the first with an empty list, which `--tree`
    // dropped to its flush-left net. Unkeyed entries keep their built
    // `required_by` (a `--root-deps` build entry names its requester itself).
    finish_required_by(&mut state, ctx);

    // The direct solve runs on the walked graph before the blocker phases
    // (Portage solves first, then validates). One joint record per slot, so
    // the solver keeps the jointly-satisfying version; removals ride
    // `skipped_updates` to the renderer.
    finish_slot_conflicts(&mut state, ctx, config, bp);

    // The all-installed-packages blocker collection runs after the graph is
    // complete and before classification; `--nodeps` is its only early exit.
    // Portage: depgraph.py.
    let orphan_blockers = collect_pass_blockers(ctx, config, &union_constraints, &mut state, bp);

    assemble_pass_result(ctx, state, orphan_blockers)
}
