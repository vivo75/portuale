//! Installed reverse dependencies an upgrade breaks: pin or drop them.

use crate::resolve::feedback::*;

/// The installed consumers a pass's complete graph can contain: the required
/// sets' closure that does not run through a package the pass replaces.
/// Only graph nodes pull their deps in, and a replaced installed package is
/// not one, so its installed deps are consumers only if something else
/// reaches them.
// Portage: depgraph.py, _complete_graph
pub(crate) fn live_reachable_consumers(
    ctx: &ResolveCtx<'_>,
    entries: &[GraphEntry],
) -> HashSet<(String, String)> {
    if ctx.slot_op_reachable.is_empty() {
        return HashSet::new();
    }
    let replaced: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    ctx.installed_graph
        .reachable_without(&ctx.slot_op_seeds, &[], &replaced)
}

/// The installed packages the request's own atoms reach, not running through a
/// replaced package: the ones Portage's main walk gives digraph edges.
// Portage: depgraph.py, _slot_operator_check_reverse_dependencies (digraph.has_edge)
pub(crate) fn request_walk_consumers(
    ctx: &ResolveCtx<'_>,
    entries: &[GraphEntry],
) -> HashSet<(String, String)> {
    let replaced: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    ctx.installed_graph
        .reachable_without(ctx.atoms, &[], &replaced)
}

/// Installed reverse dependencies an upgrade breaks: pin or drop them.
pub(crate) fn step_reverse_dep_pins(
    ctx: &ResolveCtx<'_>,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
) -> ControlFlow<PassDecision, Vec<RevDepPin>> {
    // Portage: depgraph.py, _complete_graph reaching _slot_operator_check_reverse_dependencies.
    // An upgrade this pass picked breaks a dep an *installed* package records against
    // it. Portuale finds those atoms with a direct vdb reverse scan and feeds them back
    // here as positive enforcement atoms (they stay in the positives bucket, not masks);
    // the re-resolve then picks the highest candidate satisfying them -- normally the
    // installed version -- and drops whatever the rejected version had dragged in.
    // Latched, so a pass finding nothing new falls through and the loop terminates.
    //
    // A pin jointly unsatisfiable with this pass's hard requirements is dropped
    // instead of enforced: Portage merges the hard-required version anyway and reports
    // the broken consumer against the installed instance. Dropped pins accumulate for
    // that residual report; they never enter `slot_constraints`.
    let mut added = false;
    let live_reachable = live_reachable_consumers(ctx, &pass.entries);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &ctx.repos,
        root: ctx.root,
        entries: &pass.entries,
        with_bdeps: ctx.with_bdeps,
        excluded: ctx.excluded,
        hard_want: &pass.slot_want,
        reachable: &live_reachable,
        dynamic_deps: ctx.dynamic_deps,
        ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
        top_level_cps: &ctx.top_level_cps,
        world_reachable: &ctx.world_reachable,
        config: ctx.config,
        complete: ctx.complete,
    });
    // Rule 4 of `_eliminate_rebuilds` reads every parent atom of the rebuilt pkg, and
    // the complete-graph nomerge consumers are part of `_parent_atoms`; these pins
    // carry their atoms. Cloned before the loops consume the vectors.
    let reverse_pins: Vec<RevDepPin> = enforced.iter().chain(dropped.iter()).cloned().collect();
    for pin in enforced {
        // Every enforced pin is retained (deduped) so the settle path can report the
        // upgrades they withhold -- no conflict ever forms on that path, so without
        // this the withhold would stay silent.
        if !grown.reverse_dep_pins.contains(&pin) {
            grown.reverse_dep_pins.push(pin.clone());
        }
        if grown
            .reverse_dep_masked
            .insert((pin.cp.clone(), pin.atom.clone()))
        {
            // Portage's "Record missed updates" tail: a version the re-resolve drops
            // for the enforced pin is recorded as a slot-conflict-style mask on the
            // chosen version (parents = the enforcing consumer), so the existing
            // derivation renders the WARNING row. The positive pin still drives
            // selection; the negative excludes nothing the pin does not already
            // exclude. Gated on a live search like every mask; budget-free like the
            // RevDep feedback it rides.
            if ctx.backtrack_max > 0 {
                for e in &pass.entries {
                    if e.category != pin.cp.0 || e.package != pin.cp.1 {
                        continue;
                    }
                    let (ver, slot, sub, repo) = match &e.outcome {
                        PretendOutcome::New { version }
                        | PretendOutcome::Reinstall { version, .. } => (
                            version.as_str(),
                            e.slot.as_deref().unwrap_or("0"),
                            e.sub_slot.as_deref().unwrap_or("0"),
                            e.repo_name.as_deref().unwrap_or(""),
                        ),
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } => (
                            to.as_str(),
                            e.slot.as_deref().unwrap_or("0"),
                            e.sub_slot.as_deref().unwrap_or("0"),
                            e.repo_name.as_deref().unwrap_or(""),
                        ),
                        _ => continue,
                    };
                    let candidate = format!("{}/{}-{ver}:{slot}/{sub}::{repo}", pin.cp.0, pin.cp.1);
                    if portage_dep::match_from_list(&pin.atom, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty())
                    {
                        continue;
                    }
                    let neg = format!("!={}/{}-{ver}", pin.cp.0, pin.cp.1);
                    let bucket = grown.runtime_pkg_mask.entry(pin.cp.clone()).or_default();
                    if !bucket.iter().any(|m| m.neg == neg) {
                        bucket.push(MaskEntry {
                            neg: neg.clone(),
                            reason: MaskReason::SlotConflict {
                                parents: vec![(pin.consumer.clone(), pin.atom.clone())],
                            },
                        });
                        grown.mask_order.push((pin.cp.clone(), neg));
                    }
                }
            }
            // Enforced pins are positive enforcement atoms, not masks -- exactly like
            // the solvable-conflict wants above.
            grown
                .slot_constraints
                .entry(pin.cp)
                .or_default()
                .push(pin.atom);
            added = true;
        }
    }
    for pin in dropped {
        if !grown.dropped_pins.contains(&pin) {
            grown.dropped_pins.push(pin);
        }
    }
    if added {
        // A distinct variant so the driver can feed these pins back in-pass under
        // `--backtrack=0`, where every other feedback kind still settles.
        return ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::RevDep {
            params: Box::new(std::mem::take(grown)),
        }));
    }
    ControlFlow::Continue(reverse_pins)
}
