//! Fail-fast replay of the best pass.

use crate::*;

/// Backlog #244: `merge_order::replay_create_graph`'s USE model for one
/// entry -- the ebuild's profile-valid `IUSE`, its configured USE with
/// no autounmask change applied (`ctx.config`, never a pass's
/// autounmask-tiered view: the replay layers the needed changes itself),
/// and the forced/masked flags no change can touch. `None` for anything
/// but a merge-bound ebuild (an installed or binary package's USE is
/// fixed, so the replay does not check use-deps against it).
pub(crate) fn replay_use_of(
    ctx: &ResolveCtx<'_>,
    e: &GraphEntry,
) -> Option<merge_order::ReplayUse> {
    if e.source != CandidateSource::Ebuild {
        return None;
    }
    let version = match &e.outcome {
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => version,
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
        _ => return None,
    };
    let cands = list_candidates(&ctx.repos, &e.category, &e.package).ok()?;
    let cand = cands.iter().find(|c| {
        &c.version == version && e.repo_name.as_deref().is_none_or(|r| r == c.repo_name)
    })?;
    let declared: HashSet<String> = cand
        .iuse
        .split_whitespace()
        .map(|t| t.trim_start_matches(['+', '-']).to_string())
        .collect();
    let cpv = format!(
        "{}/{}-{version}:{}/{}::{}",
        e.category, e.package, cand.slot, cand.sub_slot, cand.repo_name
    );
    Some(merge_order::ReplayUse {
        iuse: valid_iuse(&declared, ctx.config),
        enabled: effective_use_flags(
            ctx.config,
            &cand.iuse,
            &cand.keywords,
            &cpv,
            &e.category,
            &e.package,
        )
        .as_ref()
        .clone(),
        fixed: forced_or_masked_flags(
            &cand.iuse,
            &cand.keywords,
            &cpv,
            &e.category,
            &e.package,
            ctx.config,
        ),
    })
}

/// Backlog #244: real `_create_graph`'s walk over `entries` (see
/// `merge_order::replay_create_graph`), with the USE model above.
pub(crate) fn replay_walk(
    ctx: &ResolveCtx<'_>,
    entries: &[GraphEntry],
    allow_flips: bool,
    needed_init: &HashMap<(String, String), HashMap<String, bool>>,
) -> merge_order::WalkReplay {
    let use_of = |i: usize| replay_use_of(ctx, &entries[i]);
    merge_order::replay_create_graph(merge_order::ReplayCreateGraphArgs {
        entries,
        top_level_atoms: ctx.atoms,
        root: ctx.root,
        allow_flips,
        needed_init,
        use_of: &use_of,
    })
}

/// Backlog #244: whether a pass carries any autounmask USE change -- the
/// only passes the autounmask-breakage replay has anything to decide on.
pub(crate) fn pass_has_use_changes(params: &BacktrackParams, pass: &PassResult) -> bool {
    !pass.autounmask_use_changes.is_empty()
        || !pass.use_overlay.is_empty()
        || !params.autounmask_use_config.is_empty()
}

/// Real `_autounmask_breakage` (`depgraph.py:12262-12280`): the params of
/// the one final clean pass -- every autounmask change dropped and
/// suggestion fully off (`myparams["autounmask"] = False`).
pub(crate) fn breakage_clean_params(params: &BacktrackParams) -> BacktrackParams {
    let mut grown = params.clone();
    grown.autounmask_disabled = true;
    grown.autounmask_suggest_keywords = false;
    grown.autounmask_suggest_use = false;
    grown.autounmask_suggest_license = false;
    grown.autounmask_suggest_masks = false;
    grown.autounmask_use_config.clear();
    grown.autounmask_use_change_records.clear();
    grown.backtrack_config = None;
    grown
}

/// Backlog #244: real's clean pass after an autounmask breakage runs
/// with `allow_backtracking=False` (`depgraph.py:12270-12279`) -- one
/// pass, reported as it stands, then cut to what real's fail-fast walk
/// reached ([`fail_fast_truncate`]).
pub(crate) fn run_breakage_clean_pass(
    ctx: &ResolveCtx<'_>,
    initial: &BacktrackParams,
    restarts: u64,
) -> Result<GraphResult, Error> {
    // Real builds the clean depgraph with no `backtrack_parameters` at
    // all: none of the search's masks or config carries over.
    let clean = breakage_clean_params(initial);
    let pass = run_pass(ctx, &clean, false)?;
    let mut result = assemble_result(ctx, &clean, pass, ctx.config, restarts);
    fail_fast_truncate(ctx, &mut result);
    Ok(result)
}

/// Backlog #244: real `_create_graph` returns at the first dependency
/// that fails (`depgraph.py:3254-3271`, `:3483-3522`); portuale's pass
/// walks everything and classifies afterwards. Replay real's walk over
/// the result (autounmask off -- this runs on the breakage clean pass)
/// and keep only what that walk reached:
///
/// - the unsatisfied-dependency report of the first failing atom, with
///   its own dep chain (real `_get_dep_chain` from the failing parent up
///   the admitting edges to the argument) -- the other failing atoms
///   were never reached;
/// - the slot conflicts between instances admitted before the failure,
///   instances and parents in admission order (real's package tracker
///   insertion order) -- a conflict the walk would only have formed
///   after the failure never existed.
///
/// Leaves `result` untouched when the replay finds no failure or the
/// pass has no report for the failing atom (nothing provably better).
pub(crate) fn fail_fast_truncate(ctx: &ResolveCtx<'_>, result: &mut GraphResult) {
    if !matches!(result.outcome, ResolveOutcome::Aborted { .. }) {
        return;
    }
    let r = replay_walk(ctx, &result.entries, false, &HashMap::new());
    let Some(f) = r.failure.clone() else {
        return;
    };
    let entries = &result.entries;
    let edge = &entries[f.parent].deps[f.edge];
    let (fc, fp) = (edge.category.clone(), edge.package.clone());
    let same = |c: &str, p: &str, a: &str| c == fc && p == fp && a == edge.atom;
    let has_report = result
        .use_unsat_deps
        .iter()
        .any(|d| same(&d.category, &d.package, &d.atom))
        || result
            .plain_miss_deps
            .iter()
            .any(|d| same(&d.category, &d.package, &d.atom));
    if !has_report {
        return;
    }
    // The failing parent's own chain, innermost first.
    let mut chain: Vec<(String, String)> = Vec::new();
    let mut child = (fc.clone(), fp.clone());
    let mut cur = f.parent;
    let mut guard = 0usize;
    loop {
        let e = &entries[cur];
        let Some(line) = chain_node_line(e, ctx.root, &ctx.repos, ctx.config, &child) else {
            break;
        };
        chain.push(line);
        if let Some(ai) = r.admitted_arg[cur] {
            chain.push((ctx.atoms[ai].clone(), "argument".to_string()));
            break;
        }
        let Some((p, _)) = r.admitted_by[cur] else {
            break;
        };
        child = (e.category.clone(), e.package.clone());
        cur = p;
        guard += 1;
        if guard > entries.len() {
            break;
        }
    }
    result
        .use_unsat_deps
        .retain(|d| same(&d.category, &d.package, &d.atom));
    for d in &mut result.use_unsat_deps {
        d.chain = chain.clone();
    }
    result
        .plain_miss_deps
        .retain(|d| same(&d.category, &d.package, &d.atom));
    for d in &mut result.plain_miss_deps {
        d.chain = chain.clone();
    }
    result.masked_deps.clear();
    // Slot conflicts: only admitted instances, in admission order.
    let rank: HashMap<(String, String, String), usize> = r
        .admitted
        .iter()
        .enumerate()
        .filter_map(|(pos, &i)| {
            let e = &entries[i];
            merge_bound_version(&e.outcome)
                .map(|v| ((e.category.clone(), e.package.clone(), v.clone()), pos))
        })
        .collect();
    let parent_rank = |cpv: &str| -> Option<usize> {
        let (c, p, v) = split_cpv(cpv.split(':').next().unwrap_or(cpv))?;
        rank.get(&(c, p, v)).copied()
    };
    for c in &mut result.slot_conflicts {
        c.instances.retain(|inst| {
            rank.contains_key(&(c.category.clone(), c.package.clone(), inst.version.clone()))
        });
        c.instances.sort_by_key(|inst| {
            rank[&(c.category.clone(), c.package.clone(), inst.version.clone())]
        });
        for inst in &mut c.instances {
            inst.parents
                .retain(|p| p.parent_cpv.is_empty() || parent_rank(&p.parent_cpv).is_some());
        }
    }
    result.slot_conflicts.retain(|c| c.instances.len() >= 2);
    if let ResolveOutcome::Aborted { reason, .. } = &mut result.outcome
        && let AbortReason::UnsatisfiedAtom { atom, parent_cpv }
        | AbortReason::MaskedDep { atom, parent_cpv } = reason
    {
        let pe = &entries[f.parent];
        if let Some(v) = merge_bound_version(&pe.outcome) {
            *parent_cpv = format!("{}/{}-{v}", pe.category, pe.package);
        }
        *atom = edge.atom.clone();
    }
}

/// Backlog #264: real `pkg_use_display(pkg, modified_use=...)` for a
/// walked entry the #244 replay placed in a slot-conflict block -- the
/// header/parent line shows the flip-applied state (real
/// `_pkg_use_enabled`), grouped into `USE` + `USE_EXPAND` like every
/// other conflict line. The merge-row display
/// (`GraphEntry::use_expand_display`) is the wrong helper here: it is
/// real `_create_use_string`, which knows only `IUSE`-declared flags
/// and drops unchanged ones at plain `-p`, while real renders the
/// conflict block through `pkg_use_display` (implicit-expand groups
/// included, every flag shown).
///
/// The modified set is the walk-time effective USE
/// ([`candidate_iuse_and_use`], the same source
/// [`skipped_update_use_display_for`] assembles) overridden by the
/// entry's own `use_flags_display` states -- the replay flips land
/// there first (`apply_replay_flips`), and only `IUSE`-declared flags
/// can flip (the flip loop skips anything outside it, same as real's
/// changed-state reporting). `None` when the candidate or its metadata
/// is unreadable; the caller then keeps the pre-replay display.
pub(crate) fn replay_conflict_use_display(
    ctx: &ResolveCtx<'_>,
    entry: &GraphEntry,
    version: &str,
) -> Option<Vec<(String, String)>> {
    let candidates = list_candidates(&ctx.repos, &entry.category, &entry.package).ok()?;
    let cand = candidates
        .iter()
        .filter(|c| c.version == version)
        .filter(|c| entry.repo_name.as_deref().is_none_or(|r| r == c.repo_name))
        .max_by_key(|c| c.repo_priority)
        .cloned()?;
    let pf = format!("{}-{version}", entry.package);
    let metadata = repo_aux_metadata(&cand.repo_location, &entry.category, &pf).ok()?;
    let (_, base) = candidate_iuse_and_use(&cand, &entry.category, &entry.package, ctx.config)?;
    let mut modified: HashSet<String> = base.iter().cloned().collect();
    for (flag, on) in &entry.use_flags_display {
        if *on {
            modified.insert(flag.clone());
        } else {
            modified.remove(flag);
        }
    }
    Some(assemble_tree_pkg_use_display(
        AssembleTreePkgUseDisplayArgs {
            cand: &cand,
            metadata: &metadata,
            enabled: &modified,
            config: ctx.config,
            category: &entry.category,
            package: &entry.package,
            version,
        },
    ))
}

/// Backlog #244: a slot-conflict block as real's package tracker holds it
/// after the walk `r` replayed over `entries` (see
/// `merge_order::replay_create_graph`): instances in admission order,
/// each instance's parents every edge that landed on it in walk order --
/// including a requester whose use-dep the instance no longer satisfies
/// once a later requester's autounmask change flipped it (real keeps the
/// edge; the conflict block lists and marks both) -- and each instance's
/// `USE="…"` the flip-applied `pkg_use_display` (backlog #264: real
/// `_pkg_use_enabled` through `UseFlagDisplay.pkg_use_display`, grouped,
/// not the merge row's `_create_use_string`). Conflicts or
/// instances the replay cannot place are left as they are.
pub(crate) fn replay_slot_conflicts(
    ctx: &ResolveCtx<'_>,
    entries: &[GraphEntry],
    r: &merge_order::WalkReplay,
    conflicts: &mut [SlotConflict],
) {
    let find = |c: &str, p: &str, v: &str| {
        entries.iter().position(|e| {
            e.category == c
                && e.package == p
                && merge_bound_version(&e.outcome).is_some_and(|x| x == v)
        })
    };
    let pos: HashMap<usize, usize> = r
        .admitted
        .iter()
        .enumerate()
        .map(|(k, &i)| (i, k))
        .collect();
    for c in conflicts.iter_mut() {
        let idx: Vec<Option<usize>> = c
            .instances
            .iter()
            .map(|inst| {
                find(&c.category, &c.package, &inst.version).filter(|i| pos.contains_key(i))
            })
            .collect();
        if idx.iter().any(Option::is_none) {
            continue;
        }
        for (inst, i) in c.instances.iter_mut().zip(idx.iter().flatten()) {
            let parents: Vec<SlotConflictParent> = r
                .edges
                .iter()
                .filter(|(_, _, t)| t == i)
                .filter_map(|&(p, ei, _)| {
                    let pe = &entries[p];
                    let v = merge_bound_version(&pe.outcome)?;
                    let (sub, repo, slot) =
                        slot_conflict_meta(&ctx.repos, &pe.category, &pe.package, v);
                    Some(SlotConflictParent {
                        parent_cpv: format!(
                            "{}/{}-{v}:{slot}/{sub}::{repo}",
                            pe.category, pe.package
                        ),
                        atom: pe.deps[ei].atom.clone(),
                        use_display: replay_conflict_use_display(ctx, pe, v)
                            .unwrap_or_else(|| pe.use_expand_display.clone()),
                        installed: false,
                    })
                })
                .collect();
            if !parents.is_empty() {
                inst.parents = parents;
            }
            let version = inst.version.clone();
            if let Some(disp) = replay_conflict_use_display(ctx, &entries[*i], &version) {
                inst.use_display = disp;
            }
        }
        let mut order: Vec<(usize, SlotConflictInstance)> = idx
            .iter()
            .flatten()
            .map(|i| pos[i])
            .zip(c.instances.drain(..))
            .collect();
        order.sort_by_key(|(k, _)| *k);
        c.instances = order.into_iter().map(|(_, inst)| inst).collect();
    }
}

/// Backlog #244: the USE changes real's walk needed on this pass
/// (`r.needed`, beyond the ones `base` already carried in from earlier
/// passes) applied to the entries they land on -- the row's and the
/// slot-conflict block's `USE="…"` show the changed state (real
/// `_pkg_use_enabled`) -- and recorded in `changes` when the pass
/// itself did not record them (portuale's walk skips the flip when the
/// requester's atom first lands on another instance of the slot), with
/// the requester that first needed the change as the chain trigger.
pub(crate) fn apply_replay_flips(
    ctx: &ResolveCtx<'_>,
    entries: &mut [GraphEntry],
    r: &merge_order::WalkReplay,
    base: &HashMap<(String, String), HashMap<String, bool>>,
    changes: &mut Vec<AutounmaskChange>,
) {
    let origin_cp: HashMap<(String, String), (String, String)> = r
        .flip_origin
        .iter()
        .map(|(cp, &(p, _))| {
            (
                cp.clone(),
                (entries[p].category.clone(), entries[p].package.clone()),
            )
        })
        .collect();
    let mut needed: Vec<&(String, String)> = r.needed.keys().collect();
    needed.sort();
    for cp in needed {
        let flags = &r.needed[cp];
        let mut new: Vec<(String, bool)> = flags
            .iter()
            .filter(|(f, v)| base.get(cp).and_then(|m| m.get(*f)) != Some(v))
            .map(|(f, v)| (f.clone(), *v))
            .collect();
        if new.is_empty() {
            continue;
        }
        new.sort();
        let Ok(cands) = list_candidates(&ctx.repos, &cp.0, &cp.1) else {
            continue;
        };
        for e in entries.iter_mut().filter(|e| {
            e.category == cp.0 && e.package == cp.1 && e.source == CandidateSource::Ebuild
        }) {
            let Some(version) = merge_bound_version(&e.outcome).cloned() else {
                continue;
            };
            if !new
                .iter()
                .all(|(f, _)| e.use_flags_display.iter().any(|(g, _)| g == f))
            {
                continue;
            }
            let Some(cand) = cands.iter().find(|c| {
                c.version == version && e.repo_name.as_deref().is_none_or(|r| r == c.repo_name)
            }) else {
                continue;
            };
            for (f, on) in &new {
                for (g, v) in e.use_flags_display.iter_mut() {
                    if g == f {
                        *v = *on;
                    }
                }
            }
            let cpv = format!(
                "{}/{}-{version}:{}/{}::{}",
                cp.0, cp.1, cand.slot, cand.sub_slot, cand.repo_name
            );
            let forced =
                forced_or_masked_flags(&cand.iuse, &cand.keywords, &cpv, &cp.0, &cp.1, ctx.config);
            let no_reinst = HashSet::new();
            e.use_expand_display = build_use_expand_display(BuildUseExpandDisplayArgs {
                use_flags_display: &e.use_flags_display,
                config: ctx.config,
                installed: None,
                forced: &forced,
                all_flags: true,
                reinst_flags: &no_reinst,
            });
            e.use_expand_display_p = build_use_expand_display(BuildUseExpandDisplayArgs {
                use_flags_display: &e.use_flags_display,
                config: ctx.config,
                installed: None,
                forced: &forced,
                all_flags: false,
                reinst_flags: &no_reinst,
            });
            let atom = autounmask_use_atom_form(cand, &cands, &cp.0, &cp.1, ctx.config);
            let token = new
                .iter()
                .map(|(f, on)| if *on { f.clone() } else { format!("-{f}") })
                .collect::<Vec<_>>()
                .join(" ");
            if !changes.iter().any(|c| c.atom == atom && c.token == token) {
                changes.push(AutounmaskChange {
                    atom,
                    token,
                    dep_chain: Vec::new(),
                    trigger: origin_cp.get(cp).cloned(),
                });
            }
        }
    }
}
