//! Portage's slot-conflict ABI backtrack for built parents' `:=`/soname atoms.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// Portage's slot-conflict ABI backtrack for built parents' `:=`/soname atoms.
pub(crate) fn step_slot_conflict_abi(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // Backlog #214 (v2 `#24e`): real `_slot_conflict_backtrack_abi`
    // (`lib/_emerge/depgraph.py:2282-2315`) runs inside
    // `_process_slot_conflict`, BEFORE the masking backtrack below --
    // a built parent whose soname/`:=` conflict atom the update probe
    // can resolve is rebuilt instead of reported. Same order here: the
    // probe below fires before the solvability/mask arms, and its
    // restart carries no masks (real's `remaining` skip). Gated on a
    // live search like the scan's own call site (`backtrack_max > 0`
    // is real's `_allow_backtracking`; `ignore_built_slot_operator_deps`
    // drops the built atoms the probe reads; no unsatisfiable
    // dependency outstanding, like the `has_nvc` dead-end arm below --
    // real never reaches conflict processing with a failed
    // `_create_graph`).
    // Deliberately NOT gated on `rebuild_if_new_slot` (real's conflict
    // path never is -- the scan's call-site gate is the pre-existing
    // narrowing for its own arms, not this one).
    if !pass.slot_conflicts.is_empty()
        && ctx.backtrack_max > 0
        && !ctx.ignore_built_slot_operator_deps
        && !pass.suppressed_nvc
        && !pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    {
        // Same inputs the scan builds for its own refusal map: every
        // cp this pass walked, the merge-bound set, and the R2 parents.
        let walked: HashSet<(String, String)> = pass
            .entries
            .iter()
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let mut in_graph: HashSet<(String, String)> = HashSet::new();
        for e in &pass.entries {
            if !matches!(
                e.outcome,
                PretendOutcome::AlreadyInstalled { .. }
                    | PretendOutcome::NoVisibleCandidate
                    | PretendOutcome::Uninstall { .. }
            ) {
                in_graph.insert((e.category.clone(), e.package.clone()));
            }
        }
        let probe_parents = collect_probe_parents(CollectProbeParentsArgs {
            root: ctx.root,
            entries: &pass.entries,
            reachable: &ctx.slot_op_reachable,
            walked: &walked,
            in_graph: &in_graph,
            already: &grown.slot_operator_replace_installed,
            with_bdeps: ctx.with_bdeps,
            excluded: ctx.excluded,
        });
        let fresh = slot_conflict_abi_probe(SlotConflictAbiProbeArgs {
            root: ctx.root,
            repos: &ctx.repos,
            entries: &pass.entries,
            conflicts: &pass.slot_conflicts,
            already: &grown.slot_operator_replace_installed,
            undone: &grown.slot_operator_undone,
            with_bdeps: ctx.with_bdeps,
            walked: &walked,
            excluded: ctx.excluded,
            probe_parents: &probe_parents,
            config,
        });
        if !fresh.is_empty() {
            grown.slot_operator_replace_installed.extend(fresh);
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
