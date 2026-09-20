//! Fail-fast replay of the best pass.

use crate::*;

/// `merge_order::replay_create_graph`'s USE model for one entry: the ebuild's
/// profile-valid `IUSE`, its configured USE with no autounmask change applied
/// (the replay layers the needed changes itself), and the forced/masked flags.
/// `None` for anything but a merge-bound ebuild.
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
        enabled: effective_use_flags(EffectiveUseFlagsArgs {
            config: ctx.config,
            iuse: &cand.iuse,
            keywords: &cand.keywords,
            candidate_str: &cpv,
            category: &e.category,
            package: &e.package,
        })
        .as_ref()
        .clone(),
        fixed: forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
            iuse: &cand.iuse,
            keywords: &cand.keywords,
            candidate_str: &cpv,
            category: &e.category,
            package: &e.package,
            config: ctx.config,
        }),
    })
}

/// Portage's `_create_graph` walk over `entries` (see
/// `merge_order::replay_create_graph`), with the USE model above.
// Portage: _emerge/depgraph.py, _create_graph
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

/// The params of the one final clean pass: every autounmask change dropped
/// and suggestion fully off.
// Portage: depgraph.py, _autounmask_breakage
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

/// Portage's `pkg_use_display` for a walked entry the replay placed in a
/// slot-conflict block: the flip-applied state, grouped into `USE` +
/// `USE_EXPAND`. `None` when the candidate or its metadata is unreadable; the
/// caller then keeps the pre-replay display.
// Portage: UseFlagDisplay.pkg_use_display; _pkg_use_enabled
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
