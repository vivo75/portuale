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
