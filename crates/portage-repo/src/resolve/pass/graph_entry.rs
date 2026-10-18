//! Add the chosen candidate to the graph, with its autounmask suggestions.

use crate::resolve::pass::*;
use portage_profile::Config;

/// What [`push_graph_entry`] hands back to its caller.
pub(crate) struct PushGraphEntryOut {
    pub(crate) entry_idx: usize,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) candidate_remote: bool,
    pub(crate) candidate_build_id: Option<String>,
    pub(crate) already_installed_outcome: bool,
    pub(crate) candidate_str: String,
}

/// The values [`push_graph_entry`] reads from its caller.
pub(crate) struct PushGraphEntryArgs<'a> {
    pub(crate) state: &'a mut PassState,
    pub(crate) slot_key: &'a (String, String, String, bool),
    pub(crate) key: &'a (String, String),
    pub(crate) slot: &'a str,
    pub(crate) depth: u32,
    pub(crate) resolved: &'a Candidate,
    pub(crate) config: &'a Config,
    pub(crate) version: &'a str,
    pub(crate) sub_slot: &'a str,
    pub(crate) repo_name: &'a str,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) outcome: PretendOutcome,
    pub(crate) item_targets_running_root: bool,
}

/// Add the chosen candidate to the graph, with its autounmask suggestions.
pub(crate) fn push_graph_entry(args: PushGraphEntryArgs<'_>) -> PushGraphEntryOut {
    let PushGraphEntryArgs {
        state,
        slot_key,
        key,
        slot,
        depth,
        resolved,
        config,
        version,
        sub_slot,
        repo_name,
        bp,
        ctx,
        outcome,
        item_targets_running_root,
    } = args;
    let entry_idx = state.entries.len();
    state.resolved_slots.insert(slot_key.clone(), entry_idx);
    // Remember the first-resolving depth for a later in-pass autounmask
    // delta re-expansion (see `resolved_depths`).
    state
        .resolved_depths
        .insert((key.0.clone(), key.1.clone(), slot.to_string()), depth);
    let candidate_source = resolved.source;
    // Portage: output.py (remote_binary = pkg.remote)
    let candidate_remote = resolved.remote;
    // Only a binary package's version gets the `-{build_id}` suffix.
    // Portage: output.py, _append_build_id
    let candidate_build_id = if candidate_source == CandidateSource::Binary {
        resolved.build_id.clone()
    } else {
        None
    };
    let provenance = visibility_provenance(resolved, &key.0, &key.1, config);
    let keyword_mask =
        keyword_mask_marker(resolved, &key.0, &key.1, config, &provenance.mask_entry);
    let resolved_cpv_str = format!(
        "{}/{}-{version}:{slot}/{sub_slot}::{repo_name}",
        key.0, key.1
    );
    // Record the implicit `=<cpv> <keyword>` change for a candidate accepted
    // over unaccepted KEYWORDS; the gate is per-category, so a multi-masked
    // candidate still records every part.
    // Portage: depgraph.py, _display_autounmask (unstable_keyword_msg)
    if bp.autounmask_suggest_keywords
        && !keywords_accepted(KeywordsAcceptedArgs {
            keywords: &effective_keywords(
                config,
                &resolved_cpv_str,
                &key.0,
                &key.1,
                &resolved.keywords,
            ),
            candidate_str: &resolved_cpv_str,
            build_id: resolved.build_id.as_deref(),
            category: &key.0,
            package: &key.1,
            accept_keywords: &config.accept_keywords,
            package_accept_keywords: &config.package_accept_keywords,
        })
        && let Some(kw) = suggested_keyword(resolved)
    {
        state.autounmask_keyword_changes.push(AutounmaskChange {
            atom: format!("={}/{}-{version}", key.0, key.1),
            token: kw.to_string(),
            dep_chain: Vec::new(),
            trigger: None,
        });
    }
    // Record the missing licenses for a candidate accepted over unaccepted
    // LICENSE (`>=` / `>=…:slot` / `=` form). Per-category gate.
    // Portage: depgraph.py, _display_autounmask (license_msg)
    if bp.autounmask_suggest_license {
        let missing = missing_licenses(resolved, &key.0, &key.1, &resolved_cpv_str, config);
        if !missing.is_empty() {
            let all = list_candidates(&ctx.repos, &key.0, &key.1).unwrap_or_default();
            state.autounmask_license_changes.push(AutounmaskChange {
                atom: check_if_latest_atom_form(CheckIfLatestAtomFormArgs {
                    resolved,
                    all_candidates: &all,
                    category: &key.0,
                    package: &key.1,
                    config,
                    check_visibility: false,
                }),
                token: missing.join(" "),
                dep_chain: Vec::new(),
                trigger: None,
            });
        }
    }
    // Record `=<cpv>` for a candidate accepted over an active `package.mask`.
    // Per-category gate. Portage: _display_autounmask (p_mask_change_msg)
    if bp.autounmask_suggest_masks
        && provenance.mask_entry.is_some()
        && provenance.unmask_entry.is_none()
    {
        state.autounmask_mask_changes.push(AutounmaskChange {
            atom: format!("={}/{}-{version}", key.0, key.1),
            token: String::new(),
            dep_chain: Vec::new(),
            trigger: None,
        });
    }
    // A `New` entry whose cat/pkg is installed in some other slot (the
    // in-slot decision already happened in `resolve_pretend`).
    // Portage: _get_installed_best (new_slot)
    let new_slot = matches!(outcome, PretendOutcome::New { .. })
        && !installed_candidates(ctx.root, &key.0, &key.1).is_empty();
    // Kept before `outcome` moves: the REQUIRED_USE gate is `not pkg.built`
    // and `AlreadyInstalled` is exactly the installed case.
    // Portage: depgraph.py, _add_pkg
    let already_installed_outcome = matches!(outcome, PretendOutcome::AlreadyInstalled { .. });
    let candidate_str = format!(
        "{}/{}-{version}:{slot}/{sub_slot}::{repo_name}",
        key.0, key.1
    );
    // `interactive` in USE-evaluated `PROPERTIES`. Every entry here is a merge,
    // so no separate operation check is needed.
    // Portage: output.py
    let interactive = evaluated_metadata_tokens(EvaluatedMetadataTokensArgs {
        value_str: &resolved.properties,
        candidate: resolved,
        category: &key.0,
        package: &key.1,
        candidate_str: &candidate_str,
        config,
    })
    .contains("interactive");
    // `myoldbest`: the installed version(s) this entry replaces -- in-slot for
    // Upgrade/Downgrade, all slots for a new-slot New, the same version on a
    // slot/sub-slot/repo move for Reinstall. Version-sorted; empty for new.
    // Portage: _get_installed_best; output.py (vardb.cpv_exists arm)
    let mut oldbest: Vec<InstalledRef> = match &outcome {
        PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. } => {
            installed_refs(ctx.root, &key.0, &key.1)
                .into_iter()
                .filter(|r| r.slot == slot)
                .collect()
        }
        PretendOutcome::New { .. } if new_slot => installed_refs(ctx.root, &key.0, &key.1),
        PretendOutcome::Reinstall { version, .. } => installed_refs(ctx.root, &key.0, &key.1)
            .into_iter()
            .filter(|r| {
                &r.version == version
                    && (r.slot != slot || r.sub_slot != sub_slot || r.repo != repo_name)
            })
            .collect(),
        _ => Vec::new(),
    };
    oldbest.sort_by(|a, b| vercmp_ordering(&a.version, &b.version));
    state.entries.push(GraphEntry {
        discovery: 0,
        category: key.0.clone(),
        package: key.1.clone(),
        outcome,
        blockers: Vec::new(),
        slot: Some(slot.to_string()),
        sub_slot: Some(sub_slot.to_string()),
        repo_name: Some(repo_name.to_string()),
        oldbest,
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask,
        new_slot,
        interactive,
        // Filled in below once metadata/USE are read; a binary (`pkg.built`)
        // is never fetch-restricted.
        // Portage: output.py
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: candidate_source,
        provenance,
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        // The node carries its resolving root (Portage's `Package.root_config`).
        targets_running_root: item_targets_running_root,
        remote_binary: candidate_remote,
        build_id: candidate_build_id.clone(),
        deps: Vec::new(),
    });
    PushGraphEntryOut {
        entry_idx,
        candidate_source,
        candidate_remote,
        candidate_build_id,
        already_installed_outcome,
        candidate_str,
    }
}
