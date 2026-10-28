//! A dependency with no matching package: mask its parent and re-run.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// A dependency with no matching package: mask its parent and re-run.
pub(crate) fn step_missing_dep(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
    config: &Config,
) -> Option<PassDecision> {
    // Portage's walk stops at the first unsatisfied dependency in
    // `_minimize_children` order (package-less atoms first, in textual
    // order), so a USE-only miss no autounmask change can fix, in the same
    // dep string ahead of the trigger's atom, means the walk never reached
    // the trigger's dep. Drop the trigger then; with nothing else filed the
    // pass ends (no restart). The walk here visits reverse-textual order, so
    // the trigger may already be armed when the earlier miss is recorded.
    // Portage: depgraph.py, _minimize_children; _add_dep.
    if trigger_preempted_by_use_miss(pass) {
        pass.missing_dep_trigger.take();
    }
    // Portage: backtracking.py, _feedback_missing_dep. Mask the parent (`!=cpv` in
    // `runtime_pkg_mask`, latched so the same parent is never re-masked) and re-run;
    // with the `||` closure honouring slot constraints, a branch whose `a` has an
    // unsatisfiable subtree now yields to `b`. A latched trigger, or a top-level /
    // USE-only mismatch with no trigger, falls through (dead end unless other
    // feedback fired first).
    if let Some(((pc, pp), neg, dep_atom)) = pass.missing_dep_trigger.take() {
        // A built slot-operator atom probes for a resolving replacement first; only
        // a miss falls through to the mask below. Skipped under `--usepkgonly`.
        if ctx.backtrack_max > 0
            && !ctx.usepkgonly
            && parse_dep_atom(&dep_atom, "missing_dep").is_some_and(|a| is_built_slot_op(&a))
        {
            let (fresh, binary_masks) =
                slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
                    root: ctx.root,
                    entries: &pass.entries,
                    slot_want: &pass.slot_want,
                    pullers: &pass.slot_pullers,
                    masked: &grown.runtime_pkg_mask,
                    with_bdeps: ctx.with_bdeps,
                    excluded: ctx.excluded,
                    config,
                    gate: ctx.gate(),
                });
            // Portage stops the pass at the first unsatisfied dep its probe
            // cannot heal (`return 0` after recording the missing dependency),
            // so parents behind a trigger that is no hit are never probed.
            // Portage: depgraph.py, _add_pkg_dep_string (unsatisfied dep arm)
            let trigger_hit = unsat_trigger_hit(&fresh, &binary_masks, (&pc, &pp));
            if trigger_hit
                && file_unsat_probe_results(
                    &mut *grown,
                    fresh,
                    binary_masks,
                    ctx.usepkgonly,
                    ctx.root,
                )
            {
                return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(std::mem::take(grown)),
                }));
            }
        }
        // The latch moves into the node with the mask (see `Backtracker::feedback`);
        // the trigger here only packages.
        return Some(PassDecision::Feedback(BacktrackFeedback::MissingDep {
            base: Box::new(std::mem::take(grown)),
            owner: (pc.clone(), pp.clone()),
            entry: MaskEntry {
                neg,
                reason: MaskReason::MissingDependency {
                    parent: (pc, pp),
                    atom: dep_atom,
                },
            },
        }));
    }
    None
}

/// Whether a USE-only miss Portage meets first preempts the trigger: the
/// miss is unfixable (no autounmask USE change for it), in the trigger
/// owner's dep string, and textually ahead of the trigger's atom. Package-less
/// atoms go first in Portage's order, so a fixable (satisfiable-group) miss
/// never preempts. Cross-parent order is still the walk's own, so only the
/// same owner retracts.
fn trigger_preempted_by_use_miss(pass: &PassResult) -> bool {
    let Some(((pc, pp), _, dep_atom)) = pass.missing_dep_trigger.as_ref() else {
        return false;
    };
    // The expanding entry: the one whose dep string names the trigger's atom.
    let Some(owner) = pass
        .entries
        .iter()
        .find(|e| e.category == *pc && e.package == *pp && edge_index(&e.deps, dep_atom).is_some())
    else {
        return false;
    };
    let trigger_idx = edge_index(&owner.deps, dep_atom).expect("matched above");
    pass.use_unsat_deps.iter().any(|rep| {
        edge_index(&owner.deps, &rep.atom).is_some_and(|rep_idx| rep_idx < trigger_idx)
            && pass.entries.iter().any(|e| {
                e.category == rep.category
                    && e.package == rep.package
                    && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)
                    && e.use_suggestion.is_none()
                    && e.required_by.iter().any(|(c, p)| c == pc && p == pp)
            })
    })
}

/// The position of `atom` in the dep string: the raw token or its evaluated
/// form, whichever names the edge.
fn edge_index(deps: &[crate::DepEdge], atom: &str) -> Option<usize> {
    deps.iter()
        .position(|d| d.atom == atom || d.evaluated == atom)
}

/// Whether the probe heals the trigger's own parent, seeded or masked.
fn unsat_trigger_hit(
    fresh: &BTreeSet<(String, String)>,
    binary_masks: &BTreeSet<(String, String, String)>,
    owner: (&String, &String),
) -> bool {
    fresh.iter().any(|(c, p)| (c, p) == owner)
        || binary_masks.iter().any(|(c, p, _)| (c, p) == owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_that_misses_the_trigger_owner_is_no_hit_whoever_else_it_heals() {
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        let fresh = BTreeSet::from([s("kde-apps", "kmail")]);
        let masks = BTreeSet::from([(
            "kde-plasma".to_string(),
            "kwin".to_string(),
            "6.8.0".to_string(),
        )]);
        let (kc, kp) = s("kde-plasma", "libplasma");
        assert!(!unsat_trigger_hit(&fresh, &masks, (&kc, &kp)));
        let (mc, mp) = s("kde-apps", "kmail");
        assert!(unsat_trigger_hit(&fresh, &masks, (&mc, &mp)));
        let (wc, wp) = s("kde-plasma", "kwin");
        assert!(unsat_trigger_hit(&fresh, &masks, (&wc, &wp)));
    }
}
