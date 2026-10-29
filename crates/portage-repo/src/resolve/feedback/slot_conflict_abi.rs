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
    // Runs inside conflict processing, before the masking backtrack: a built parent
    // whose soname/`:=` conflict atom the update probe can resolve is rebuilt
    // instead of reported. Its restart carries no masks. Gated on a live search,
    // visible built atoms, and no unsatisfiable dependency outstanding.
    if !pass.slot_conflicts.is_empty()
        && ctx.backtrack_max > 0
        && !ctx.ignore_built_slot_operator_deps
        && !pass.suppressed_nvc
        && !pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    {
        // Same inputs the rebuild scan builds for its own refusal map: every
        // cp this pass walked and the merge-bound set.
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
        let fresh = slot_conflict_abi_probe_full(SlotConflictAbiProbeArgs {
            root: ctx.root,
            entries: &pass.entries,
            conflicts: &pass.slot_conflicts,
            already: &grown.slot_operator_replace_installed,
            undone: &grown.slot_operator_undone,
            with_bdeps: ctx.with_bdeps,
            walked: &walked,
            excluded: ctx.excluded,
            probe_parents: &probe_parents,
            config,
            gate: ctx.gate(),
        });
        // A child is only ever filed together with its parent, so the parent
        // set is the whole guard.
        if !fresh.parents.is_empty() {
            // Portage's `_slot_operator_update_backtrack` files both sides of
            // the built `:=` dep: `_replace_installed_atom(dep.parent)` and,
            // with `new_child_slot is None`, `_replace_installed_atom(child)`
            // when the child is installed (`depgraph.py:2454-2460`).
            // `probe_replaced` is the display mirror that becomes
            // `__auto_slot_operator_replace_installed__` (the `r` tag), so it
            // grows with the same set.
            let filed: BTreeSet<(String, String)> =
                fresh.parents.union(&fresh.children).cloned().collect();
            grown
                .slot_operator_replace_installed
                .extend(filed.iter().cloned());
            grown.probe_replaced.extend(filed);
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
