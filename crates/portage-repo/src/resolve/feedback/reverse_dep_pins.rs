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

/// Mask-and-restart over a dropped reverse-dep pin: the first residual
/// record whose merge version is not yet masked becomes one
/// [`BacktrackFeedback::MissingDep`] mask (`!=cp-ver`, opposed by the
/// contradicting dropped pins), so the retry re-picks around it. `None`
/// when the pass settles as it always did (no record, already masked, no
/// contradicting pin to name, or a gate below holds).
fn dropped_pin_mask_restart(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // G0: the pass is already doomed by a non-residual abort driver --
    // autounmask USE activity this pass, or a hard masked/USE/plain miss.
    // Portage reports those before any backtrack heal (backtrack 0/20 on
    // the C3 probes), so a mask excursion here only loses the abort's own
    // rows. Only fresh pass activity gates: accumulated config from earlier
    // passes must not block a later heal.
    if !pass.autounmask_use_changes.is_empty()
        || !pass.use_overlay.is_empty()
        || !pass.masked_deps.is_empty()
        || !pass.use_unsat_deps.is_empty()
        || !pass.plain_miss_deps.is_empty()
    {
        return None;
    }
    // The gate is the assembly report itself: only a pin combination the
    // settle path would abort on restarts, so clean resolves never mask.
    let records = build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
        repos: &ctx.repos,
        config: ctx.config,
        root: ctx.root,
        entries: &pass.entries,
        slot_pullers: &pass.slot_pullers,
        dropped: &grown.dropped_pins,
    });
    // `dropped` accumulates across passes: a replaced consumer names no
    // conflict (mirrors the builder's own filter).
    let replaced: HashSet<(String, String)> = pass
        .entries
        .iter()
        .filter(|e| merge_bound_cpv(e).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    for record in &records {
        let owner = (record.category.clone(), record.package.clone());
        let neg = format!(
            "!={}/{}-{}",
            record.category, record.package, record.resolved_version
        );
        // The mask itself latches: a re-formed record settles as before, so
        // the search terminates.
        if grown
            .runtime_pkg_mask
            .get(&owner)
            .is_some_and(|bucket| bucket.iter().any(|m| m.neg == neg))
            || grown.missing_dep_masked.contains(&neg)
        {
            continue;
        }
        let Some(merge) = record.instances.iter().find(|i| !i.installed) else {
            continue;
        };
        // G1: the contradicted merge version must be hard-wanted by a
        // merge-bound third-party puller -- only then can the retry re-pick
        // around the mask. A bare explicit-arg want (keeper/igw/cgp) cannot
        // be re-picked: masking strands the argument, the retry errors, and
        // Portage aborts the residual loudly instead.
        if !merge
            .parents
            .iter()
            .any(|p| has_merge_bound_puller(pass, p))
        {
            continue;
        }
        let merge_match = format!(
            // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
            "{}/{}-{}:{}/{}::{}",
            record.category,
            record.package,
            record.resolved_version,
            record.slot,
            merge.sub_slot,
            merge.repo_name
        );
        let mut parents: Vec<((String, String, String), String)> = Vec::new();
        for pin in &grown.dropped_pins {
            if pin.cp != owner
                || replaced.contains(&(pin.consumer.0.clone(), pin.consumer.1.clone()))
            {
                continue;
            }
            // Only pins the merge contradicts oppose the mask; a stale match
            // would print a nonsense conflict line.
            if portage_dep::match_from_list(&pin.atom, &[merge_match.as_str()])
                .is_some_and(|m| !m.is_empty())
            {
                continue;
            }
            // G2: a pin from a `||` branch the merge satisfies elsewhere is
            // a stale alternative, not a break (Portage demotes it via
            // dep_zapdeps instead of backtrack-skipping the update).
            if or_branch_superseded(ctx, pass, pin) {
                continue;
            }
            let parent = (pin.consumer.clone(), pin.atom.clone());
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        }
        if parents.is_empty() {
            continue;
        }
        return Some(PassDecision::Feedback(BacktrackFeedback::MissingDep {
            base: Box::new(std::mem::take(grown)),
            owner,
            entry: MaskEntry {
                neg,
                reason: MaskReason::SlotConflict { parents },
            },
        }));
    }
    None
}

/// Whether a residual merge-instance parent is a merge-bound third-party
/// puller: the top-level `(Argument)` pseudo-puller (empty `parent_cpv`)
/// and non-merge-bound cps cannot be re-picked around, so a conflict they
/// alone want must abort, never mask.
fn has_merge_bound_puller(pass: &PassResult, parent: &SlotConflictParent) -> bool {
    if parent.parent_cpv.is_empty() {
        return false;
    }
    let Some(head) = parent.parent_cpv.split(':').next() else {
        return false;
    };
    let Some((pc, pp, pv)) = split_cpv(head) else {
        return false;
    };
    pass.entries.iter().any(|e| {
        e.category == pc
            && e.package == pp
            && merge_bound_version(&e.outcome).is_some_and(|v| v == &pv)
    })
}

/// One parsed dependency item for `||`-group analysis: plain atoms stay
/// strings; `||` groups and conjunctions keep their members. `flag?`
/// conditionals evaluate against the consumer USE at parse time (dead
/// branches vanish, like the pin scan never saw them).
enum DepItem {
    Atom(String),
    Any(Vec<DepItem>),
    All(Vec<DepItem>),
}

/// Whether a `flag?`/`!flag?` conditional is active (mirrors the
/// reducer's `is_active`; `??` reads flag `?` like there).
fn cond_active(cond: &str, use_flags: &HashSet<String>) -> bool {
    let (negated, flag) = match cond.strip_suffix('?') {
        Some(rest) => match rest.strip_prefix('!') {
            Some(f) => (true, f),
            None => (false, rest),
        },
        None => return false,
    };
    (use_flags.contains(flag) && !negated) || (!use_flags.contains(flag) && negated)
}

/// Parse one level (until `)` or end) with conditionals evaluated: `||`
/// groups become `Any`, plain parens `All`, live conditional bodies splice
/// transparently, dead ones vanish. `None` on malformed input (pins from
/// such a key stay, the conservative direction).
fn parse_dep_level(
    toks: &[String],
    mut pos: usize,
    use_flags: &HashSet<String>,
) -> Option<(Vec<DepItem>, usize)> {
    let mut out = Vec::new();
    while pos < toks.len() {
        match toks[pos].as_str() {
            ")" => return Some((out, pos + 1)),
            "||" => {
                if toks.get(pos + 1).map(String::as_str) != Some("(") {
                    return None;
                }
                let (inner, next) = parse_dep_level(toks, pos + 2, use_flags)?;
                out.push(DepItem::Any(inner));
                pos = next;
            }
            "(" => {
                let (inner, next) = parse_dep_level(toks, pos + 1, use_flags)?;
                out.push(DepItem::All(inner));
                pos = next;
            }
            t if t.ends_with('?') => {
                if toks.get(pos + 1).map(String::as_str) != Some("(") {
                    return None;
                }
                let (inner, next) = parse_dep_level(toks, pos + 2, use_flags)?;
                if cond_active(t, use_flags) {
                    out.extend(inner);
                }
                pos = next;
            }
            // `^^` (exactly-one) has no branch the pin scan separates: as a
            // plain atom it never parses, so its content stays conservative.
            _ => {
                out.push(DepItem::Atom(toks[pos].clone()));
                pos += 1;
            }
        }
    }
    Some((out, pos))
}

/// Every `Any` group anywhere in the tree (through conjunctions).
fn collect_any_groups<'a>(items: &'a [DepItem], out: &mut Vec<&'a Vec<DepItem>>) {
    for item in items {
        match item {
            DepItem::Any(members) => {
                out.push(members);
                collect_any_groups(members, out);
            }
            DepItem::All(members) => collect_any_groups(members, out),
            DepItem::Atom(_) => {}
        }
    }
}

/// Whether a member's atom names the pin: verbatim, or normalised the way
/// the scan normalised it into the pin.
fn or_member_matches_pin(member_atom: &str, pin: &RevDepPin) -> bool {
    if member_atom == pin.atom || member_atom == pin.raw_atom {
        return true;
    }
    parse_dep_atom(member_atom, "dropped_pin_or_branch")
        .is_some_and(|a| reverse_dep_constraint_atom(member_atom, &a) == pin.atom)
}

/// Whether a member is satisfied by the settled merge graph: a plain atom
/// matching a merge-bound entry (USE stripped, version+slot), a
/// conjunction only when all of it is, a nested `||` when any of it is.
fn or_member_satisfied(member: &DepItem, merge_candidates: &[String]) -> bool {
    match member {
        DepItem::Atom(atom) => {
            let probe = portage_dep::without_use(atom).to_string();
            let candidates: Vec<&str> = merge_candidates.iter().map(String::as_str).collect();
            portage_dep::match_from_list(&probe, &candidates).is_some_and(|m| !m.is_empty())
        }
        DepItem::All(members) => members
            .iter()
            .all(|m| or_member_satisfied(m, merge_candidates)),
        DepItem::Any(members) => members
            .iter()
            .any(|m| or_member_satisfied(m, merge_candidates)),
    }
}

/// Whether the member gives the consumer a merge-satisfied route avoiding
/// the pin's package: a plain atom of another package matching a
/// merge-bound entry; a conjunction only when all of it holds and some leg
/// routes around; a nested `||` when some alternative does both. A version
/// menu of the pin's own package never counts.
fn or_member_alternate_route(
    member: &DepItem,
    pin_cp: &(String, String),
    merge_candidates: &[String],
) -> bool {
    match member {
        DepItem::Atom(atom) => parse_dep_atom(atom, "dropped_pin_or_branch").is_some_and(|a| {
            a.blocker == portage_dep::Blocker::None
                && (a.category.clone(), a.package.clone()) != *pin_cp
                && or_member_satisfied(member, merge_candidates)
        }),
        DepItem::All(members) => {
            members
                .iter()
                .all(|m| or_member_satisfied(m, merge_candidates))
                && members
                    .iter()
                    .any(|m| or_member_alternate_route(m, pin_cp, merge_candidates))
        }
        DepItem::Any(members) => members
            .iter()
            .any(|m| or_member_alternate_route(m, pin_cp, merge_candidates)),
    }
}

/// Whether the pin comes from a `||` branch the merge satisfies elsewhere:
/// the consumer's recorded depstring (same layering the scan read) holds a
/// group containing the pin whose sibling alternative -- targeting another
/// package -- matches the settled merge graph. Portage demotes such a
/// branch via dep_zapdeps instead of backtrack-skipping the update, so the
/// pin must not drive a mask.
fn or_branch_superseded(ctx: &ResolveCtx<'_>, pass: &PassResult, pin: &RevDepPin) -> bool {
    let (cc, cp2, cv) = &pin.consumer;
    let use_flags = read_vdb_flag_set(ctx.root, cc, cp2, cv, "USE");
    let live = live_metadata_for_installed(&ctx.repos, ctx.root, cc, cp2, cv);
    let dep_keys: &[&str] = if ctx.with_bdeps {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND"]
    };
    let mut memo = HashMap::new();
    let merge_candidates: Vec<String> = pass
        .entries
        .iter()
        .filter_map(|e| {
            let version = merge_bound_version(&e.outcome)?;
            let slot = e.slot.as_deref().unwrap_or("0");
            Some(match e.sub_slot.as_deref().unwrap_or("") {
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                "" => format!("{}/{}-{version}:{slot}", e.category, e.package),
                sub => format!(
                    // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                    "{}/{}-{version}:{slot}/{sub}",
                    e.category, e.package
                ),
            })
        })
        .collect();
    for key in dep_keys {
        // Same layering the pin scan read (see `constraints_from_consumer`).
        let layer = if ctx.dynamic_deps {
            InstalledMetaLayer::Effective
        } else {
            InstalledMetaLayer::Raw
        };
        let depstr = installed_dep_string(InstalledDepStringArgs {
            root: ctx.root,
            dynamic_deps: ctx.dynamic_deps,
            ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
            category: cc,
            package: cp2,
            version: cv,
            live: live.as_deref(),
            key,
            layer,
            memo: &mut memo,
        });
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let Some((items, pos)) = parse_dep_level(&tokens, 0, &use_flags) else {
            continue;
        };
        if pos != tokens.len() {
            continue;
        }
        let mut groups: Vec<&Vec<DepItem>> = Vec::new();
        collect_any_groups(&items, &mut groups);
        for group in groups {
            // The alternative holding the pin (a conjunction subgroup counts
            // whole: its conjuncts are not alternatives of each other).
            let mut container: Option<usize> = None;
            for (idx, member) in group.iter().enumerate() {
                if or_member_matches_pin_recursive(member, pin) {
                    container = Some(idx);
                    break;
                }
            }
            let Some(held) = container else {
                continue;
            };
            if group
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != held)
                .any(|(_, sib)| or_member_alternate_route(sib, &pin.cp, &merge_candidates))
            {
                return true;
            }
        }
    }
    false
}

/// Whether the pin's atom occurs anywhere inside the member.
fn or_member_matches_pin_recursive(member: &DepItem, pin: &RevDepPin) -> bool {
    match member {
        DepItem::Atom(atom) => or_member_matches_pin(atom, pin),
        DepItem::All(members) | DepItem::Any(members) => members
            .iter()
            .any(|m| or_member_matches_pin_recursive(m, pin)),
    }
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
    // A dropped pin the settled merge contradicts would abort the run as a
    // residual slot conflict: with backtracking live, mask the contradicted
    // merge version and retry instead (no enforced restart pending here).
    // Portage: depgraph.py, _slot_confict_backtrack (mask over the conflict)
    if !added
        && ctx.backtrack_max > 0
        && let Some(decision) = dropped_pin_mask_restart(ctx, pass, grown)
    {
        return ControlFlow::Break(decision);
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
