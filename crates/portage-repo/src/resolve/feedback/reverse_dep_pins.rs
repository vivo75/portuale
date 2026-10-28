//! Installed reverse dependencies an upgrade breaks: pin or drop them.

use crate::resolve::feedback::*;
use crate::resolve::{DepTree, reduced_dep_tree};

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

/// The installed-consumer pins one pass's reverse-dependency scan found.
pub(crate) struct ReverseScan {
    /// Pins the merge still satisfies: enforced on the next pass.
    pub(crate) enforced: Vec<RevDepPin>,
    /// Pins jointly unsatisfiable with the pass's hard requirements.
    pub(crate) dropped: Vec<RevDepPin>,
}

/// The pass's one reverse-dependency scan.
// Portage: depgraph.py, _complete_graph reaching _slot_operator_check_reverse_dependencies.
pub(crate) fn scan_reverse_dependencies(ctx: &ResolveCtx<'_>, pass: &PassResult) -> ReverseScan {
    let live_reachable = live_reachable_consumers(ctx, &pass.entries);
    let (enforced, dropped) = reverse_dependency_constraints(ReverseDependencyConstraintsArgs {
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
        portage_complete: ctx.portage_complete(&pass.entries),
        installed_walked: &pass.installed_walked,
        update: ctx.update,
        selective: ctx.selective && !ctx.empty,
        backtrack_allowed: ctx.backtrack_max > 0,
        arg_atoms: ctx.atoms,
        root_deps_running_root: ctx.root_deps_running_root,
        gate: ctx.gate(),
    });
    ReverseScan { enforced, dropped }
}

/// Whether the pass met a masked, USE-unsatisfied or absent dependency.
/// Portage's walk stops there and processes no slot conflict (depgraph.py
/// `_create_graph`, `_resolve`); the `_complete_graph` re-walk does not stop.
pub(crate) fn pass_has_unsatisfied_dep(pass: &PassResult) -> bool {
    !pass.masked_deps.is_empty()
        || !pass.use_unsat_deps.is_empty()
        || !pass.plain_miss_deps.is_empty()
        || pass.missing_dep_trigger.is_some()
}

/// Whether this pass may feed a consumer-pin conflict back. Portage returns
/// before it processes slot conflicts when a dependency is missing, masked,
/// USE-unsatisfied or plainly absent (depgraph.py `_resolve`), and a fresh
/// autounmask change restarts the run first. A newly enforced pin is solved
/// before any backtracking over what is left.
fn consumer_pin_conflict_may_feed_back(
    pass: &PassResult,
    grown: &BacktrackParams,
    scan: &ReverseScan,
) -> bool {
    scan.enforced.iter().all(|p| {
        grown
            .reverse_dep_masked
            .contains(&(p.cp.clone(), p.atom.clone()))
    }) && pass.autounmask_use_changes.is_empty()
        && pass.use_overlay.is_empty()
        && !pass_has_unsatisfied_dep(pass)
        && !pass.autounmask_grew
        && !(grown.autounmask_use_broke && !grown.autounmask_disabled)
}

/// Files the slot conflict a dropped installed-consumer pin leaves behind:
/// the merge version and the installed instance in one slot, the consumer's
/// atoms as parents of the installed one. Added after the walker's own
/// conflicts, so the backtrack step, which feeds back the first conflict
/// only, still takes theirs first. Returns whether a record was filed.
/// Nothing is filed without a live search.
// Portage: depgraph.py, _process_slot_conflicts
pub(crate) fn file_consumer_pin_conflicts(
    ctx: &ResolveCtx<'_>,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
    scan: &ReverseScan,
) -> bool {
    if ctx.backtrack_max == 0 || !consumer_pin_conflict_may_feed_back(pass, grown, scan) {
        return false;
    }
    for pin in &scan.dropped {
        if !grown.dropped_pins.contains(pin) {
            grown.dropped_pins.push(pin.clone());
        }
    }
    let records = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        config: ctx.config,
        root: ctx.root,
        entries: &pass.entries,
        slot_pullers: &pass.slot_pullers,
        dropped: &grown.dropped_pins,
        gate: ctx.gate(),
    });
    let mut filed = false;
    for record in records {
        filed |= file_after_walker_conflicts(&mut pass.slot_conflicts, record);
    }
    filed
}

/// Appends `record` after the existing conflicts. A conflict of the same
/// slot and instances is kept in place and takes the record's installed
/// parents. Returns whether the list changed.
fn file_after_walker_conflicts(conflicts: &mut Vec<SlotConflict>, record: SlotConflict) -> bool {
    let same = |c: &SlotConflict| {
        c.category == record.category
            && c.package == record.package
            && c.slot == record.slot
            && c.resolved_version == record.resolved_version
            && c.instances.get(1).map(|i| &i.version) == record.instances.get(1).map(|i| &i.version)
    };
    let Some(existing) = conflicts.iter_mut().find(|c| same(c)) else {
        conflicts.push(record);
        return true;
    };
    let mut changed = false;
    for added in record.instances.iter().filter(|i| i.installed) {
        if let Some(inst) = existing
            .instances
            .iter_mut()
            .find(|i| i.installed && i.version == added.version)
        {
            for parent in &added.parents {
                if !inst.parents.contains(parent) {
                    inst.parents.push(parent.clone());
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Every `AnyOf` group anywhere in the tree (through conjunctions).
fn collect_any_groups<'a>(items: &'a [DepTree], out: &mut Vec<&'a Vec<DepTree>>) {
    for item in items {
        match item {
            DepTree::AnyOf(members) => {
                out.push(members);
                collect_any_groups(members, out);
            }
            DepTree::AllOf(members) => collect_any_groups(members, out),
            DepTree::Atom(_) => {}
        }
    }
}

/// Whether the member is satisfied by what is installed right now: a plain
/// atom matching an installed instance of its own package, a conjunction
/// only when every leg is, a nested `||` when any leg is. Blockers and
/// unparseable text never match.
fn or_member_matches_installed(member: &DepTree, root: &Path) -> bool {
    match member {
        DepTree::Atom(text) => {
            let probe = portage_dep::without_use(text).to_string();
            parse_dep_atom(text, "dropped_pin_vdb_select").is_some_and(|a| {
                a.blocker == portage_dep::Blocker::None
                    && installed_refs(root, &a.category, &a.package)
                        .iter()
                        .any(|r| {
                            let cand = format!(
                                // match-str-ok: installed ref row (r.repo), not a Candidate
                                "{}/{}-{}:{}/{}::{}",
                                a.category, a.package, r.version, r.slot, r.sub_slot, r.repo
                            );
                            let candidates = [cand.as_str()];
                            portage_dep::match_from_list(&probe, &candidates)
                                .is_some_and(|m| !m.is_empty())
                        })
            })
        }
        DepTree::AllOf(members) => members.iter().all(|m| or_member_matches_installed(m, root)),
        DepTree::AnyOf(members) => members.iter().any(|m| or_member_matches_installed(m, root)),
    }
}

/// Every flat atom text inside the member (through conjunctions).
fn or_member_flat_atoms(member: &DepTree) -> Vec<&String> {
    match member {
        DepTree::Atom(text) => vec![text],
        DepTree::AllOf(members) | DepTree::AnyOf(members) => {
            members.iter().flat_map(or_member_flat_atoms).collect()
        }
    }
}

/// `||`-branch texts of `depstr` that do not match an installed instance while a
/// sibling branch does. An approximation of `dep_zapdeps` kept only for the
/// slot-operator probe of an installed parent; the pin scan chooses with
/// `consumer_or_demotions`. Verbatim plus constraint-normalised. Empty when
/// `depstr` holds no `||` group, fails to parse, or no branch matches.
// Portage: dep_check.py, dep_zapdeps (installed-preference bins)
pub(crate) fn or_branch_vdb_deselected_set(
    root: &Path,
    depstr: &str,
    use_flags: &HashSet<String>,
) -> HashSet<String> {
    let mut demoted = HashSet::new();
    if !depstr.contains("||") {
        return demoted;
    }
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let Some(items) = reduced_dep_tree(&tokens, use_flags) else {
        return demoted;
    };
    let mut groups: Vec<&Vec<DepTree>> = Vec::new();
    collect_any_groups(&items, &mut groups);
    for group in groups {
        if !group.iter().any(|m| or_member_matches_installed(m, root)) {
            continue;
        }
        for member in group {
            if or_member_matches_installed(member, root) {
                continue;
            }
            for text in or_member_flat_atoms(member) {
                demoted.insert(text.clone());
                if let Some(a) = parse_dep_atom(text, "dropped_pin_vdb_select") {
                    demoted.insert(reverse_dep_constraint_atom(text, &a));
                }
            }
        }
    }
    demoted
}

/// Installed reverse dependencies an upgrade breaks: pin or drop them.
pub(crate) fn step_reverse_dep_pins(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    grown: &mut BacktrackParams,
    scan: ReverseScan,
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
    let ReverseScan { enforced, dropped } = scan;
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
                    // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
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
                            // The built `:S/SS=` form renders first, like
                            // Portage's missed-update parents.
                            reason: MaskReason::SlotConflict {
                                parents: pin
                                    .built_render
                                    .iter()
                                    .map(|built| (pin.consumer.clone(), built.clone()))
                                    .chain(std::iter::once((
                                        pin.consumer.clone(),
                                        pin.atom.clone(),
                                    )))
                                    .collect(),
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
