//! Re-expanding a resolved slot when USE changes.

use crate::*;

/// Arguments of [`delta_atom_cleanly_reuses_resolved_slot`].
pub(crate) struct DeltaAtomCleanlyReusesResolvedSlotArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) state: &'a PassState,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) evaluated: &'a str,
    /// This delta atom's own resolving root (its queue tag): selection and slot
    /// lookup both run against it, so a running-root atom never reuses a
    /// target-root slot (and vice versa).
    pub(crate) atom_running: bool,
}

/// Drops a text-new delta atom before queueing when the pop is provably
/// bookkeeping-only: it speculatively re-resolves to the already-resolved
/// version of an already-resolved slot, with slot and use-dep constraints
/// satisfied. Without this the pop folds the flipped package into the puller
/// maps and synthesizes a phantom merge-order edge Portage never has (a
/// newly-gated atom naming an already-graphed slot never re-enters its
/// `_dep_stack`). Anything else -- a new slot, a version/slot conflict,
/// unsatisfied use-deps, an installed/unsatisfiable outcome, a blocker --
/// queues exactly as before. Side-effect free: `resolve_pretend` writes no
/// global state, and all inputs are pass-fixed.
/// Never second-guesses complete mode's graph-or-installed branch.
// Portage: depgraph.py, _add_pkg (previously-added check)
pub(crate) fn delta_atom_cleanly_reuses_resolved_slot(
    args: DeltaAtomCleanlyReusesResolvedSlotArgs<'_>,
) -> bool {
    let DeltaAtomCleanlyReusesResolvedSlotArgs {
        ctx,
        config,
        union_constraints,
        state,
        bp,
        evaluated,
        atom_running,
    } = args;
    let Some(atom) = portage_dep::parse_atom(evaluated) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return false;
    }
    let key2 = (atom.category.clone(), atom.package.clone());
    // Complete mode diverts dependency atoms around `resolve_pretend`; never
    // second-guess that branch.
    if ctx.complete
        && !ctx.complete_locked_merges.is_empty()
        && !ctx.complete_locked_merges.contains(&key2)
    {
        return false;
    }
    let empty_constraints: Vec<String> = Vec::new();
    let extra_constraints = union_constraints.get(&key2).unwrap_or(&empty_constraints);
    let select_root: &Path = if atom_running {
        ctx.cross_root()
            .expect("running-root atom without a cross root")
    } else {
        ctx.root
    };
    let Ok(outcome) = resolve_pretend(
        &SelectSource {
            repos: &ctx.repos,
            root: select_root,
            config,
            local_binpkg: &ctx.local_binpkg,
        },
        evaluated,
        &SelectOptions {
            extra_constraints,
            ..ctx.select_options(bp)
        },
    ) else {
        return false;
    };
    // Only a merge-bound outcome can hit the already-resolved branch below;
    // installed/unsatisfiable outcomes take the pop site's own paths.
    let version = match &outcome {
        PretendOutcome::New { version } => version.clone(),
        PretendOutcome::Upgrade { to, .. } => to.clone(),
        PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::Reinstall { version, .. } => version.clone(),
        _ => return false,
    };
    // The same winning-candidate pick as the pop site.
    let repo_candidates = list_candidates(&ctx.repos, &key2.0, &key2.1).unwrap_or_default();
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by(|a, b| {
            let src_rank = |c: &Candidate| match c.source {
                CandidateSource::Binary => 1u8,
                CandidateSource::Ebuild => 0,
            };
            src_rank(a)
                .cmp(&src_rank(b))
                .then(a.repo_priority.cmp(&b.repo_priority))
        })
    else {
        return false;
    };
    let slot_key2 = (
        key2.0.clone(),
        key2.1.clone(),
        resolved.slot.clone(),
        atom_running,
    );
    let Some(&existing_idx2) = state.resolved_slots.get(&slot_key2) else {
        return false;
    };
    let existing_version2 = match &state.entries[existing_idx2].outcome {
        PretendOutcome::New { version } => version.clone(),
        PretendOutcome::Upgrade { to, .. } => to.clone(),
        PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::Reinstall { version, .. } => version.clone(),
        // `resolved_slots` only ever indexes the four merge-bound outcomes.
        _ => return false,
    };
    if version != existing_version2 {
        return false;
    }
    // Include the already-resolved version's own sub-slot, like the pop site.
    let existing_sub = slot_conflict_meta(&ctx.repos, &key2.0, &key2.1, &existing_version2).0;
    let existing_match_str = if existing_sub.is_empty() {
        format!(
            "{}/{}-{existing_version2}:{}",
            key2.0, key2.1, resolved.slot
        )
    } else {
        format!(
            "{}/{}-{existing_version2}:{}/{}",
            key2.0, key2.1, resolved.slot, existing_sub
        )
    };
    let bare_equals = atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
        && atom.slot.is_none()
        && atom.sub_slot.is_none();
    if portage_dep::match_from_list(evaluated, &[existing_match_str.as_str()])
        .is_none_or(|m| m.is_empty())
        || (bare_equals
            && built_equals_shift_unrebuildable(BuiltEqualsShiftUnrebuildableArgs {
                root: ctx.root,
                repos: &ctx.repos,
                category: &key2.0,
                package: &key2.1,
                existing_sub: &existing_sub,
                pullers: &state.slot_pullers,
                excluded: ctx.excluded,
            }))
    {
        return false;
    }
    if bare_equals
        && let Some((inst_ver, _)) = built_equals_binding(ctx.root, &ctx.repos, &key2.0, &key2.1)
        && inst_ver != existing_version2
        && built_equals_shift_unrebuildable(BuiltEqualsShiftUnrebuildableArgs {
            root: ctx.root,
            repos: &ctx.repos,
            category: &key2.0,
            package: &key2.1,
            existing_sub: &existing_sub,
            pullers: &state.slot_pullers,
            excluded: ctx.excluded,
        })
    {
        return false;
    }
    // Version + slot matched; the `[flag]` use-deps must also hold against the
    // settled package, else the pop site folds a flip. Only then a clean reuse.
    if let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty()) {
        let all_cands = list_candidates(&ctx.repos, &key2.0, &key2.1).unwrap_or_default();
        let Some(existing_cand) = all_cands.iter().find(|c| c.version == existing_version2) else {
            return false;
        };
        let declared: HashSet<String> = existing_cand
            .iuse
            .split_whitespace()
            .map(|t| t.trim_start_matches(['+', '-']).to_string())
            .collect();
        let iuse_set = valid_iuse(&declared, config);
        let existing_cand_str = format!(
            "{}/{}-{existing_version2}:{}/{}::{}",
            key2.0, key2.1, existing_cand.slot, existing_cand.sub_slot, existing_cand.repo_name
        );
        let existing_use = effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &existing_cand.iuse,
            keywords: &existing_cand.keywords,
            candidate_str: &existing_cand_str,
            category: &key2.0,
            package: &key2.1,
        });
        if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use) {
            return false;
        }
    }
    true
}
