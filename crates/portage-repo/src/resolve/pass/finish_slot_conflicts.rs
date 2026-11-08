//! Merge, solve and synthesize the pass slot conflicts.

use crate::resolve::pass::*;
use portage_profile::Config;

/// Merge, solve and synthesize the pass slot conflicts.
pub(crate) fn finish_slot_conflicts(
    state: &mut PassState,
    ctx: &ResolveCtx<'_>,
    config: &Config,
    bp: &BacktrackParams,
) {
    record_merge_bound_virtual_leaks(state, ctx, config);
    state.slot_conflicts = merge_same_slot_conflicts(MergeSameSlotConflictsArgs {
        conflicts: &state.slot_conflicts,
        pullers: &state.slot_pullers,
        config,
        root: ctx.root,
        // A lone record misses parents walked later; at `--backtrack=0` re-file
        // them from the live pullers, like the merged ones (Portage reads the
        // final parent atoms).
        refile_singletons: ctx.backtrack_max == 0,
        gate: ctx.gate(),
    });
    solve_pass_slot_conflicts(&mut *state, ctx, bp, config);

    // Rows for the shadowed second instances of surviving conflicts (after the
    // solve, before blocker collection, so the new nodes join it).
    synthesize_surviving_conflict_entries(SynthesizeSurvivingConflictEntriesArgs {
        entries: &mut state.entries,
        conflicts: &state.slot_conflicts,
        required_by_map: &state.required_by_map,
        root: ctx.root,
        config,
        gate: ctx.gate(),
    });
}

/// Quirk Q19: a merge-bound virtual's leaked atoms are parent atoms of what they
/// match, like its own choice.
// Portage: depgraph.py, _select_atoms_highest_available (chosen_atom_ids)
fn record_merge_bound_virtual_leaks(state: &mut PassState, ctx: &ResolveCtx<'_>, config: &Config) {
    if state.slot_conflicts.is_empty() {
        return;
    }
    let leaks = merge_bound_virtual_leaks(MergeBoundVirtualLeaksArgs {
        entries: &state.entries,
        root: ctx.root,
        config,
        gate: ctx.gate(),
        update: ctx.update,
        root_deps_running_root: ctx.root_deps_running_root,
        deep: ctx.deep,
        excluded: ctx.excluded,
        arg_atoms: ctx.atoms,
        set_args: &ctx.set_args,
        complete: ctx.complete,
    });
    let mut leaks: Vec<_> = leaks.into_iter().collect();
    leaks.sort_by(|a, b| a.0.cmp(&b.0));
    for ((vc, vp, vv), atoms) in leaks {
        let mut atoms: Vec<String> = atoms.into_iter().collect();
        atoms.sort();
        for atom in atoms {
            let Some(dep) = parse_dep_atom(&atom, "virtual_leak") else {
                continue;
            };
            let row = (vc.clone(), vp.clone(), vv.clone(), atom.clone());
            let pullers = state
                .slot_pullers
                .entry((dep.category.clone(), dep.package.clone()))
                .or_default();
            if !pullers.contains(&row) {
                pullers.push(row);
            }
        }
    }
}
