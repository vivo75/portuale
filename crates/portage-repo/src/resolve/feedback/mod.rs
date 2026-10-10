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

/// Phase C1 (023): why one `cat/pkg` version is masked on retry,
/// mirroring real `backtracking.py`'s `runtime_pkg_mask[pkg][reason]`
/// dict shape (one entry per masked version, reason-annotated). C1 only
/// splits the storage -- readers still see the union (see
/// `BacktrackParams::union_constraints`); C2's `Backtracker` consumes
/// the reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskReason {
    /// Real `mask_info["slot conflict"]`: the masked version collided in
    /// its slot. `parents` are the pulling `((cat, pkg, ver), atom)`
    /// triples whose atom does NOT match the masked version (real's
    /// `conflict_atoms`, used by the bug-375573 discard check, which
    /// tests versioned parent packages -- hence the stored `ver`).
    /// Parent downgrades are `MissingDependency` masks on a later node,
    /// never empty-parent slot masks (C3 removed puller pre-masking).
    SlotConflict {
        parents: Vec<((String, String, String), String)>,
    },
    /// Real `mask_info["missing dependency"]`: the masked parent's
    /// subtree contained an unsatisfiable dep; `atom` is that dep's text
    /// (real also records the root; portuale resolves a single root).
    MissingDependency {
        parent: (String, String),
        atom: String,
    },
    /// Real `mask_info["slot_operator_mask_built"]` (backlog #212,
    /// v2 `#24c`): a non-installed binary whose built slot-operator dep
    /// cannot be satisfied the way it was built, recorded by
    /// `_slot_change_backtrack` (`depgraph.py:2383`),
    /// `_slot_operator_update_backtrack` (`:2427-2431`) and
    /// `_slot_operator_unsatisfied_backtrack` (`:2901-2903`) and
    /// enforced through the backtrack-config restart (`:5703-5712`,
    /// `resolver/backtracking.py::_feedback_config:237-242`, which
    /// files the mask under *both* `slot_operator_mask_built` *and*
    /// `runtime_pkg_mask`). Real keys by package object, so the mask
    /// hides only the binary instance -- the same-version ebuild stays
    /// visible. Portuale's string readers match binary and ebuild at
    /// one `cpv:slot::repo` key, so the rendered `neg` carries a
    /// `[binary]` marker (`!=cat/pkg-ver[binary]`): every
    /// string-constraint reader skips marked entries (see
    /// [`is_binary_only_mask`] -- `match_from_list` ignores USE-deps,
    /// so the marker would otherwise match the ebuild too), and only
    /// the binary-candidate pool filter honours them (via
    /// [`slot_operator_binary_masked_version`]). Only the slot-change
    /// arm is ported
    /// (see [`slot_operator_slot_change_probe`]); the update-probe and
    /// unsatisfied-probe arms have no portuale-visible diverging shape
    /// (both steer a parent the graph already replaces or fails on, so
    /// same-version binary-vs-ebuild steering is vacuous there).
    SlotOperatorBuilt,
}

/// One `!=cat/pkg-ver` negative with its reason. `neg` renders exactly
/// as the string used to live in `slot_constraints`, so every reader
/// that matches on it behaves identically. The masked version is
/// recoverable without parsing: strip the `!=cat/pkg-` prefix formed
/// from the bucket key (C2 does this when it needs per-cpv granularity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaskEntry {
    pub(crate) neg: String,
    pub(crate) reason: MaskReason,
}

/// File one `_slot_operator_unsatisfied_probe` result into the working
/// backtrack accumulators: replace-set seeds grow
/// `slot_operator_replace_installed` (real
/// `backtrack_infos["config"]["slot_operator_replace_installed"]`),
/// binary parents become `[binary]`-marked `SlotOperatorBuilt`
/// negatives (real `:2896-2903`). Returns true when anything grew --
/// the caller restarts with the same budget-free Config feedback the
/// #211 scan returns. Single filing point for both probe sites (the
/// missing-dep trigger arm, which real runs first per failing edge, and
/// the no-trigger dead-end fallback); the replace set therefore
/// expands in exactly one place here, and neither arm keeps restart
/// bookkeeping of its own. Under `--usepkgonly` the binary result is
/// discarded (real's `_iter_similar_available` yields binaries only,
/// so no ebuild replacement exists -- see the trigger site).
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

/// One pass's outcome in real `_backtrack_depgraph` terms -- see the
/// type docs above. The overlay merge runs first (steps read the merged
/// accumulators, as they always did); each arm then packages its effect
/// without touching the search state.
pub(crate) fn collect_feedback(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    pass: &mut PassResult,
    config: &portage_profile::Config,
) -> PassDecision {
    // C2: the A3 overlay union lands in a working copy, not the live
    // search state -- the driver applies the packaged effect through
    // `Backtracker::feedback`.
    // A3: fold this pass's overlay into the cross-pass accumulators
    // before deciding. The old code wrote `bp` directly during the walk,
    // so at this point `bp` already held everything; the union below
    // reproduces that state exactly (idempotent per-flag insert with the
    // same `bucket.get(f) != Some(want)` rule, `(atom, token)`-deduped
    // record append, `|=` latch for the contradiction). Placed before the
    // chain rather than in step 5: an earlier step (solvable growth, mask
    // trial, breakage) may fire on the same pass that folded flips, and
    // the retry must see them -- as it always did.
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

    // #57 S1: the *dropped* half of the reverse-dependency scan (#54,
    // documented at its enforcement site further down) is not feedback
    // at all -- it is the input `assemble_result` renders the residual
    // installed-instance block from. A pass that ends in slot-conflict
    // mask feedback returns long before the scan's own site, so those
    // pins never reached `grown` and a later `get_best_run` re-pass
    // reported the collision without its installed consumers (the
    // keeper-reachable triangle lost keeper's `=dev-libs/paired-1.0`
    // parent line the moment the walk started recording that collision
    // itself). Collected here for exactly that case -- a pass that found
    // a slot conflict -- so the ordinary settle path pays for one scan,
    // as it always did, not two.
    if !pass.slot_conflicts.is_empty() {
        let (_, dropped) = reverse_dependency_constraints(
            &ctx.repos,
            ctx.root,
            &pass.entries,
            ctx.with_bdeps,
            ctx.excluded,
            &pass.slot_want,
            &ctx.slot_op_reachable,
            ctx.dynamic_deps,
            ctx.ignore_built_slot_operator_deps,
            &ctx.top_level_cps,
            &ctx.world_reachable,
            ctx.config,
            ctx.complete,
        );
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

/// Phase A2 (023): the settle tail of `backtracking_resolve` --
/// rebuild passes, trace dump, merge-order sort, autounmask
/// coalesce, residual conflicts, masked chains, `abort_outcome`,
/// and the `GraphResult`. Residual conflicts (`slot_conflicts`
/// extension) live here, not in `PassResult`: no `continue`
/// follows them, so they cannot influence a retry.
/// Backlog #205: USE changes start their chain at the forcing
/// parent, not the first requirer. Real `_get_dep_chain(pkg,
/// unsatisfied_dependency=True)` (`depgraph.py:6257`) picks the
/// parent whose atom the package still does not satisfy
/// (`best_match_to_list`); when every parent is satisfied
/// post-change -- the ordinary flip shape, the change fixed them
/// all -- it walks the digraph parents preferring the last merge
/// parent (bug #354747), i.e. the deepest forcing edge, then the
/// argument. The flip site records its forcing owner in
/// `AutounmaskChange::trigger`; the fill starts there when it is
/// still a requirer, else the legacy first-requirer start stands
/// (stale triggers after a masking retry, parent-flip rescues).
/// (Keyword/license/mask changes keep the legacy start: real
/// renders those without the unsatisfied filter -- see
/// [`fill_non_use_change_chains`].) The ascent above that start is
/// unchanged (single-branch, first-requirer hops).
/// Backlog #248: no requirer at all means the change sits on a start
/// node (an argument package) -- real `_get_dep_chain` never prints
/// the start node, so the fill passes `None` and
/// [`autounmask_dep_chain`] renders just the `(argument)` line. (The
/// old `.or(Some(own))` fallback started the walk at the change's own
/// package, printing its `# required by <cpv>::<repo>` row first.)
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

/// Backlog #248: keyword/license/mask twin of
/// [`fill_use_change_chains`]. Re-derive from the change's own atom:
/// its left-hand `=<cpv>` / `>=<cpv>` names the package the change is
/// on, whose requirer chain is what real `_get_dep_chain_as_comment`
/// prints (real renders those without the unsatisfied filter, so no
/// `trigger` narrowing here). Same no-start-node rule: a change on an
/// argument package (no requirers) passes `None`, yielding just the
/// `(argument)` line.
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
