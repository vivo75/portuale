//! The pass result from the finished walk state.

use crate::resolve::pass::*;

/// The pass result from the finished walk state.
///
/// REQUIRED_USE violations fail here, after the whole walk, so every
/// reachable candidate still resolves on its own terms.
pub(crate) fn assemble_pass_result(
    ctx: &ResolveCtx<'_>,
    state: PassState,
    orphan_blockers: Vec<OrphanBlocker>,
) -> Result<PassResult, Error> {
    if !state.required_use_violations.is_empty() {
        // Each block is self-delimiting (leading + trailing newline).
        return Err(Error::Detail(state.required_use_violations.join("")));
    }

    let pass = PassResult {
        entries: state.entries,
        suppressed_nvc: state.suppressed_nvc,
        slot_conflicts: state.slot_conflicts,
        skipped_updates: state.skipped_updates,
        orphan_blockers,
        slot_want: state.slot_want,
        slot_pullers: state.slot_pullers,
        masked_deps: state.masked_deps,
        use_unsat_deps: state.use_unsat_deps,
        plain_miss_deps: state.plain_miss_deps,
        nvc_dep_atoms: state.nvc_dep_atoms,
        missing_dep_trigger: state.missing_dep_trigger,
        missing_args: state.missing_args,
        autounmask_grew: state.autounmask_grew,
        parent_flip_rescued: state.parent_flip_rescued,
        edge_kind_map: state.edge_kind_map,
        changed_deps_report_entries: state.changed_deps_report_entries,
        // The `WARNING: … package.provided:` block lists requested atoms in requested
        // order (not walk order); atoms absent from argv sort last, stably.
        pprovided_atoms: {
            let mut v = state.pprovided_atoms;
            let pos = |a: &String| ctx.atoms.iter().position(|x| x == a).unwrap_or(usize::MAX);
            v.sort_by_key(pos);
            v
        },
        autounmask_keyword_changes: state.autounmask_keyword_changes,
        autounmask_use_changes: state.autounmask_use_changes,
        autounmask_license_changes: state.autounmask_license_changes,
        autounmask_mask_changes: state.autounmask_mask_changes,
        use_overlay: state.use_overlay,
        use_change_overlay: state.use_change_overlay,
        use_broke: state.use_broke,
        abi_rebuilds: None,
        parent_atoms: state.parent_atoms,
    };
    Ok(pass)
}
