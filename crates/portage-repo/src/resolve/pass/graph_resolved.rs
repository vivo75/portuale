//! Pick the resolved candidate and graph it, reporting slot collisions.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// What [`graph_resolved_entry`] hands back to its caller.
pub(crate) struct GraphResolvedEntryOut<'a> {
    pub(crate) resolved: &'a Candidate,
    pub(crate) entry_idx: usize,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) candidate_remote: bool,
    pub(crate) candidate_build_id: Option<String>,
    pub(crate) already_installed_outcome: bool,
    pub(crate) candidate_str: String,
    pub(crate) repo_location: PathBuf,
    pub(crate) keywords: Vec<String>,
}

/// The values [`graph_resolved_entry`] reads from its caller.
pub(crate) struct GraphResolvedEntryArgs<'a, 's> {
    pub(crate) repo_candidates: &'a [Candidate],
    pub(crate) version: &'s str,
    pub(crate) key: &'s (String, String),
    pub(crate) item_targets_running_root: bool,
    pub(crate) state: &'s mut PassState,
    pub(crate) ctx: &'s ResolveCtx<'s>,
    pub(crate) current_atom: &'s str,
    pub(crate) atom: &'s Atom,
    pub(crate) config: &'s Config,
    pub(crate) bp: &'s BacktrackParams,
    pub(crate) owner: &'s Option<(String, String)>,
    pub(crate) union_constraints: &'s HashMap<(String, String), Vec<String>>,
    pub(crate) depth: u32,
    pub(crate) outcome: PretendOutcome,
}

/// Pick the resolved candidate and graph it, reporting slot collisions.
///
/// The version tie prefers the binary over the ebuild; an already-resolved
/// slot re-checks instead of re-resolving. Falls through only when the merge
/// still graphs.
pub(crate) fn graph_resolved_entry<'a, 's>(
    args: GraphResolvedEntryArgs<'a, 's>,
) -> ControlFlow<(), GraphResolvedEntryOut<'a>> {
    let GraphResolvedEntryArgs {
        repo_candidates,
        version,
        key,
        item_targets_running_root,
        state,
        ctx,
        current_atom,
        atom,
        config,
        bp,
        owner,
        union_constraints,
        depth,
        outcome,
    } = args;
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == *version)
        .max_by(|a, b| {
            // "Ebuild type is the last resort": a binary beats the ebuild at a
            // version tie. Portage: _wrapped_select_pkg_highest_available_imp
            let src_rank = |c: &Candidate| match c.source {
                CandidateSource::Binary => 1u8,
                CandidateSource::Ebuild => 0,
            };
            src_rank(a)
                .cmp(&src_rank(b))
                .then(a.repo_priority.cmp(&b.repo_priority))
        })
    else {
        return ControlFlow::Break(());
    };
    let slot = resolved.slot.clone();
    let sub_slot = resolved.sub_slot.clone();
    let repo_location = resolved.repo_location.clone();
    let repo_name = resolved.repo_name.clone();
    let keywords = resolved.keywords.clone();

    // Node identity includes the resolving root: a running-root atom never
    // reuses a target-root slot (the cross-root in-graph boundary).
    let slot_key = (
        key.0.clone(),
        key.1.clone(),
        slot.clone(),
        item_targets_running_root,
    );
    if step_existing_slot(StepExistingSlotArgs {
        state: &mut *state,
        slot_key: &slot_key,
        ctx,
        key,
        slot: &slot,
        current_atom,
        atom,
        config,
        version,
        bp,
        owner,
        union_constraints,
    })
    .is_break()
    {
        return ControlFlow::Break(());
    }
    // The mirror of the `AlreadyInstalled` check (opposite argv order): the
    // slot holds no merge but an installed node this atom rejects. Falls
    // through -- the merge still graphs, while the collision reports.
    if let Some(installed_version) = state.installed_slots.get(&slot_key).cloned() {
        let installed_sub = installed_refs(ctx.root, &key.0, &key.1)
            .into_iter()
            .find(|r| r.version == installed_version)
            .map(|r| r.sub_slot)
            .unwrap_or_default();
        let installed_str = if installed_sub.is_empty() {
            // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
            format!("{}/{}-{installed_version}:{slot}", key.0, key.1)
        } else {
            format!(
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                "{}/{}-{installed_version}:{slot}/{installed_sub}",
                key.0, key.1
            )
        };
        if portage_dep::match_from_list(current_atom, &[installed_str.as_str()])
            .is_none_or(|m| m.is_empty())
        {
            record_slot_conflict(
                &mut state.slot_conflicts,
                build_slot_conflict(BuildSlotConflictArgs {
                    repos: &ctx.repos,
                    config,
                    root: ctx.root,
                    category: &key.0,
                    package: &key.1,
                    slot: &slot,
                    existing_version: &installed_version,
                    current_atom,
                    current_version: version,
                    slot_pullers: &state.slot_pullers,
                    installed_side: Some(InstalledSide::Existing),
                }),
            );
        }
    }
    let PushGraphEntryOut {
        entry_idx,
        candidate_source,
        candidate_remote,
        candidate_build_id,
        already_installed_outcome,
        candidate_str,
    } = push_graph_entry(PushGraphEntryArgs {
        state: &mut *state,
        slot_key: &slot_key,
        key,
        slot: &slot,
        depth,
        resolved,
        config,
        version,
        sub_slot: &sub_slot,
        repo_name: &repo_name,
        bp,
        ctx,
        outcome,
        item_targets_running_root,
    });
    ControlFlow::Continue(GraphResolvedEntryOut {
        resolved,
        entry_idx,
        candidate_source,
        candidate_remote,
        candidate_build_id,
        already_installed_outcome,
        candidate_str,
        repo_location,
        keywords,
    })
}
