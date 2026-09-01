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
