//! Collect the pass blockers and flag tree-solved replacements.

use crate::resolve::pass::*;
use portage_profile::Config;

/// Collect the pass blockers and flag tree-solved replacements.
pub(crate) fn collect_pass_blockers(
    ctx: &ResolveCtx<'_>,
    config: &Config,
    union_constraints: &HashMap<(String, String), Vec<String>>,
    state: &mut PassState,
    bp: &BacktrackParams,
) -> Vec<OrphanBlocker> {
    if !ctx.nodeps {
        collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
            repos: &ctx.repos,
            root: ctx.root,
            config,
            dynamic_deps: ctx.dynamic_deps,
            ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
            disj_constraints: union_constraints,
            root_deps_running_root: ctx.root_deps_running_root,
            entries: &state.entries,
            pending_blockers: &mut state.pending_blockers,
            installed_meta_memo: &mut state.installed_meta_memo,
        });
    }

    let conflicts = resolve_blockers_with_seeds(ResolveBlockersWithSeedsArgs {
        root: ctx.root,
        pending: &state.pending_blockers,
        entries: &state.entries,
        blocker_retry_closure: &ctx.blocker_retry_closure,
        buildpkgonly: ctx.buildpkgonly,
        set_seed_atoms: &ctx.config.blocker_retry_seed_atoms,
    });
    // Unresolved rows whose owner has no display entry ride out on the result.
    let mut orphan_blockers: Vec<OrphanBlocker> = Vec::new();
    file_blocker_conflicts(
        &mut state.entries,
        ctx.root,
        conflicts,
        &mut orphan_blockers,
    );

    // Tree-mode solved verdicts: only Replacement rows can a scheduled uninstall
    // solve, and the simulation is gated on at least one existing, so the common
    // no-blocker resolve pays nothing. Flags ride into `GraphResult` for `--tree`.
    let needs_tree_sim = state.entries.iter().any(|e| {
        e.blockers.iter().any(|b| {
            !b.unsolvable && matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. }))
        })
    });
    if needs_tree_sim {
        let solved =
            ctx.tree_sim_memo
                .get_or_compute(&state.entries, &bp.circular_dependency, || {
                    merge_order::tree_solved_replacements(merge_order::TreeSolvedReplacementsArgs {
                        entries: &state.entries,
                        top_level_atoms: ctx.atoms,
                        config: ctx.config,
                        root: ctx.root,
                        implicit_system_deps: ctx.implicit_system_deps,
                        repos: &ctx.repos,
                        dynamic_deps: ctx.dynamic_deps,
                        circular: &bp.circular_dependency,
                    })
                });
        for (owner, blocker) in solved {
            if let Some(b) = state
                .entries
                .get_mut(owner)
                .and_then(|e| e.blockers.get_mut(blocker))
            {
                b.tree_scheduled_uninstall = true;
            }
        }
    }
    orphan_blockers
}
