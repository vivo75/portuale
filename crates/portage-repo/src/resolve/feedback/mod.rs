//! Turning a pass's problems into feedback for the next pass.

use crate::*;
use std::ops::ControlFlow;

mod autounmask;
mod circular_dead_end;
mod missing_dep;
mod no_leaf_restart;
mod reverse_dep_pins;
mod slot_conflict_abi;
mod slot_conflict_backtrack;
mod slot_operator_probe;
mod solvable_slot_conflicts;
mod unsatisfied_dead_end;
pub(crate) use autounmask::*;
pub(crate) use circular_dead_end::*;
pub(crate) use missing_dep::*;
pub(crate) use no_leaf_restart::*;
pub(crate) use reverse_dep_pins::*;
pub(crate) use slot_conflict_abi::*;
pub(crate) use slot_conflict_backtrack::*;
pub(crate) use slot_operator_probe::*;
pub(crate) use solvable_slot_conflicts::*;
pub(crate) use unsatisfied_dead_end::*;

/// Why one `cat/pkg` version is masked on retry. Readers see the union across
/// reasons; the backtracker consumes them. Portage: backtracking.py, `runtime_pkg_mask`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskReason {
    /// The masked version collided in its slot. `parents` are the pulling
    /// `((cat, pkg, ver), atom)` triples whose atom does NOT match it.
    SlotConflict {
        parents: Vec<((String, String, String), String)>,
    },
    /// The masked parent's subtree contained an unsatisfiable dep; `atom` is
    /// that dep's text. Portage also records the root; portuale resolves one root.
    MissingDependency {
        parent: (String, String),
        atom: String,
    },
    /// A non-installed binary whose built slot-operator dep cannot be satisfied the
    /// way it was built. The rendered `neg` carries a `[binary]` marker, so only the
    /// binary-candidate pool filter honours it; every string-constraint reader skips
    /// marked entries. Only the slot-change arm is ported; the update and
    /// unsatisfied probe arms steer a parent the graph already replaces or fails on.
    SlotOperatorBuilt,
}

/// One `!=cat/pkg-ver` negative with its reason. `neg` renders exactly as the string
/// used to live in `slot_constraints`, so every reader matching on it behaves
/// identically. The masked version is recoverable without parsing: strip the
/// `!=cat/pkg-` prefix formed from the bucket key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaskEntry {
    pub(crate) neg: String,
    pub(crate) reason: MaskReason,
}

/// File one `_slot_operator_unsatisfied_probe` result into the working backtrack
/// accumulators: replace-set seeds grow `slot_operator_replace_installed`, binary
/// parents become `[binary]`-marked `SlotOperatorBuilt` negatives. Returns true when
/// anything grew -- the caller restarts with budget-free Config feedback. Single
/// filing point for both probe sites (the missing-dep trigger arm and the no-trigger
/// dead-end fallback). Under `--usepkgonly` the binary result is discarded.
pub(crate) fn file_unsat_probe_results(
    grown: &mut BacktrackParams,
    fresh: BTreeSet<(String, String)>,
    binary_masks: BTreeSet<(String, String, String)>,
    usepkgonly: bool,
) -> bool {
    let mut masks_new = false;
    // `--usepkgonly`: no ebuild replacement (see the trigger site).
    let binary_masks = if usepkgonly {
        BTreeSet::new()
    } else {
        binary_masks
    };
    for (cat, pkg, ver) in &binary_masks {
        let neg = format!("!={cat}/{pkg}-{ver}[binary]");
        let bucket = grown
            .runtime_pkg_mask
            .entry((cat.clone(), pkg.clone()))
            .or_default();
        if !bucket.iter().any(|m| m.neg == neg) {
            bucket.push(MaskEntry {
                neg,
                reason: MaskReason::SlotOperatorBuilt,
            });
            masks_new = true;
        }
    }
    if masks_new
        || fresh
            .iter()
            .any(|cp| !grown.slot_operator_replace_installed.contains(cp))
    {
        grown.slot_operator_replace_installed.extend(fresh);
        return true;
    }
    false
}

/// One pass's outcome in `_backtrack_depgraph` terms. The overlay merge runs first
/// (steps read the merged accumulators, as they always did); each arm then packages
/// its effect without touching the search state.
pub(crate) fn collect_feedback(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    pass: &mut PassResult,
    config: &portage_profile::Config,
) -> PassDecision {
    // The A3 overlay union lands in a working copy, not the live search state --
    // the driver applies the packaged effect through `Backtracker::feedback`.
    // Placed before the chain: an earlier step may fire on the same pass that folded
    // flips, and the retry must see them -- as it always did.
    let mut grown = params.clone();
    for (cp, flags) in &pass.use_overlay {
        let bucket = grown.autounmask_use_config.entry(cp.clone()).or_default();
        for (f, want) in flags {
            if bucket.get(f) != Some(want) {
                bucket.insert(f.clone(), *want);
            }
        }
    }
    for rec in &pass.use_change_overlay {
        if !grown
            .autounmask_use_change_records
            .iter()
            .any(|c| c.atom == rec.atom && c.token == rec.token)
        {
            grown.autounmask_use_change_records.push(rec.clone());
        }
    }
    grown.autounmask_use_broke |= pass.use_broke;

    // A pass that ends in slot-conflict mask feedback returns long before the
    // reverse-dependency scan's own site, so those pins never reached `grown` and a
    // later re-pass would report the collision without its installed consumers.
    // Collected here for exactly that case, so the ordinary settle path pays for one
    // scan, as it always did, not two.
    if !pass.slot_conflicts.is_empty() {
        let (_, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
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
        for pin in dropped {
            if !grown.dropped_pins.contains(&pin) {
                grown.dropped_pins.push(pin);
            }
        }
    }

    if let Some(decision) = step_circular_dead_end(pass, ctx, &mut grown) {
        return decision;
    }

    if let Some(decision) = step_slot_conflict_abi(pass, ctx, &mut grown, config) {
        return decision;
    }

    if let Some(decision) = step_solvable_slot_conflicts(pass, ctx, config, &mut grown) {
        return decision;
    }

    if let Some(decision) = step_slot_conflict_backtrack(pass, ctx, &mut grown, config) {
        return decision;
    }

    if let Some(decision) = step_autounmask(&mut grown, pass, ctx) {
        return decision;
    }

    if let Some(decision) = step_missing_dep(pass, ctx, &mut grown, config) {
        return decision;
    }

    let reverse_pins = match step_reverse_dep_pins(ctx, pass, &mut grown) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(decision) => return decision,
    };

    let has_nvc = match step_unsatisfied_dead_end(pass, ctx, &mut grown, config, &reverse_pins) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(decision) => return decision,
    };

    if let Some(decision) = step_no_leaf_restart(ctx, has_nvc, pass, &mut grown) {
        return decision;
    }

    if let Some(decision) = step_slot_operator_probe(has_nvc, ctx, pass, &mut grown, config) {
        return decision;
    }

    if has_nvc {
        PassDecision::DeadEnd {
            params: Box::new(grown),
        }
    } else {
        PassDecision::Settle {
            params: Box::new(grown),
        }
    }
}

/// The settle tail of `backtracking_resolve`. USE changes start their chain at the
/// forcing parent, not the first requirer: the flip site records its forcing owner in
/// `AutounmaskChange::trigger`; the fill starts there when it is still a requirer,
/// else the legacy first-requirer start stands. The ascent above that start is
/// unchanged (single-branch, first-requirer hops). Keyword/license/mask changes keep
/// the legacy start (see `fill_non_use_change_chains`).
pub(crate) fn fill_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| {
                    change
                        .trigger
                        .clone()
                        .filter(|t| e.required_by.contains(t))
                        .or_else(|| e.required_by.first().cloned())
                });
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}

/// Keyword/license/mask twin of `fill_use_change_chains`. Re-derive from the change's
/// own atom: its left-hand `=<cpv>` / `>=<cpv>` names the package the change is on.
/// Same no-start-node rule: a change on an argument package passes `None`, yielding
/// just the `(argument)` line.
pub(crate) fn fill_non_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| e.required_by.first().cloned());
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}
