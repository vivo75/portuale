//! Seed the walk queue with the top-level and auto-replace atoms.

use crate::resolve::pass::*;

/// Seed the walk queue with the top-level and auto-replace atoms.
pub(crate) fn seed_queue(ctx: &ResolveCtx<'_>, state: &mut PassState, bp: &BacktrackParams) {
    for a in ctx.atoms.iter().rev() {
        // A top-level atom has no parent to flip a flag on, so no unevaluated
        // form (Portage never suggests a parent-flag fix for one either).
        // Reversed declaration order: Portage feeds same-level atoms through a
        // stack, so the last declared resolves first; selection reuses an
        // already-graphed satisfier, making order visible.
        state.queue.push_back(QueueItem {
            atom: a.clone(),
            depth: 0,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }
    // The slot-operator auto-replace set rides as root nodes after the user args
    // (which is why the rebuilt consumer's deps re-walk at all). `owner: None`
    // mirrors a set arg having no parent; `depth: 1` keeps them out of `--deep`
    // accounting and the `(Argument)` puller line.
    // Portage: depgraph.py, _gen_reinstall_sets; _eliminate_rebuilds rule 5
    for (cat, pkg) in &bp.slot_operator_replace_installed {
        state.queue.push_back(QueueItem {
            atom: format!("{cat}/{pkg}"),
            depth: 1,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }
    // The probe's upgraded child rides the same auto set, seeded at the exact
    // candidate the probe chose (Portage's `abi_reinstalls`).
    for (cat, pkg, ver) in &bp.slot_operator_forced_upgrade {
        state.queue.push_back(QueueItem {
            atom: format!("={cat}/{pkg}-{ver}"),
            depth: 1,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }
}
