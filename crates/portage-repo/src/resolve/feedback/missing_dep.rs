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
            && portage_dep::parse_atom(&dep_atom).is_some_and(|a| is_built_slot_op(&a))
        {
            let (fresh, binary_masks) =
                slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
                    root: ctx.root,
                    repos: &ctx.repos,
                    entries: &pass.entries,
                    slot_want: &pass.slot_want,
                    pullers: &pass.slot_pullers,
                    masked: &grown.runtime_pkg_mask,
                    with_bdeps: ctx.with_bdeps,
                    excluded: ctx.excluded,
                    config,
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
