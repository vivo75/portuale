//! Installed reverse dependencies an upgrade breaks: pin or drop them.

use crate::resolve::feedback::*;

/// Installed reverse dependencies an upgrade breaks: pin or drop them.
pub(crate) fn step_reverse_dep_pins(
    ctx: &ResolveCtx<'_>,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
) -> ControlFlow<PassDecision, Vec<RevDepPin>> {
    // Real `_complete_graph`'s installed-universe walk reaching
    // `_slot_operator_check_reverse_dependencies`: an upgrade this
    // pass picked breaks a dependency an *installed* package -- one
    // nowhere near the requested atom's own tree -- already records
    // against it. Real sees such an atom because complete mode
    // (auto-enabled by any installed-package version/USE change)
    // deep-walks `@world`/`@selected`/`@system` and pulls every
    // *reachable* installed package into the graph as a nomerge node
    // (#54: not the whole installed universe -- see
    // `reverse_dependency_constraints`'s doc comment), contributing its
    // recorded atoms to `_parent_atoms`;
    // portuale finds them with a direct vdb reverse scan instead and
    // feeds them back here as positive enforcement atoms (C1: they stay
    // in the positives bucket -- they are not masks), which is
    // precisely what parent atoms are. The re-resolve then picks the
    // highest candidate satisfying them -- normally the installed
    // version, so the entry settles as `AlreadyInstalled` -- and
    // drops whatever the rejected version had dragged into the
    // graph. `reverse_dep_masked` latches every constraint already
    // added, so a pass that finds nothing new falls through and the
    // loop terminates.
    //
    // A pin jointly unsatisfiable with this pass's hard requirements
    // (an explicit versioned request, or a dependency only a newer
    // version satisfies) is dropped instead of enforced: real merges
    // the hard-required version anyway and reports the broken
    // consumer against the installed instance. Dropped pins
    // accumulate in `dropped_pins` for that residual report (see
    // `build_residual_slot_conflicts`); they never enter
    // `slot_constraints`, so the hard-required version is never
    // masked into invisibility.
    let mut added = false;
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
        repos: &ctx.repos,
        root: ctx.root,
        entries: &pass.entries,
        with_bdeps: ctx.with_bdeps,
        excluded: ctx.excluded,
        hard_want: &pass.slot_want,
        reachable: &ctx.slot_op_reachable,
        dynamic_deps: ctx.dynamic_deps,
        ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
        top_level_cps: &ctx.top_level_cps,
        world_reachable: &ctx.world_reachable,
        config: ctx.config,
        complete: ctx.complete,
    });
    // #24 S4: rule 4 of `_eliminate_rebuilds` checks every parent atom of
    // the rebuilt pkg, and real's complete-graph nomerge consumers are
    // part of `_parent_atoms`; these are the pins carrying their atoms.
    // Cloned before the loops consume the vectors.
    let reverse_pins: Vec<RevDepPin> = enforced.iter().chain(dropped.iter()).cloned().collect();
    for pin in enforced {
        // Backlog #90 (S2) + #92: retain every enforced pin (deduped
        // like `dropped_pins`) so `assemble_result` can report the
        // upgrades they withhold -- no conflict ever forms on that
        // path, so without this the withhold would stay silent.
        if !grown.reverse_dep_pins.contains(&pin) {
            grown.reverse_dep_pins.push(pin.clone());
        }
        if grown
            .reverse_dep_masked
            .insert((pin.cp.clone(), pin.atom.clone()))
        {
            // Backlog #205: real's "Record missed updates" tail
            // (`lib/_emerge/depgraph.py:2085-2106`) -- a version the
            // re-resolve drops for the enforced pin is reported
            // through `_show_missed_update_slot_conflicts`. Record the
            // rejection as a slot-conflict-style mask on the chosen
            // version (parents = the enforcing consumer), so the
            // existing `backtrack_missed_updates` derivation renders
            // the `WARNING` row. The positive pin above still drives
            // selection; the negative excludes nothing the pin does
            // not already exclude. Gated on a live search like every
            // mask (real `_allow_backtracking`); budget-free like the
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
            // C1: enforced pins are positive enforcement atoms (the
            // consumer's recorded atom must keep matching), not
            // masks -- they stay in the positives bucket, exactly
            // like the solvable-conflict wants above.
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
        // Backlog #209: a distinct variant (see its doc comment) so the
        // driver can feed these pins back in-pass under `--backtrack=0`,
        // where every other feedback kind still settles.
        return ControlFlow::Break(PassDecision::Feedback(BacktrackFeedback::RevDep {
            params: Box::new(std::mem::take(grown)),
        }));
    }
    ControlFlow::Continue(reverse_pins)
}
