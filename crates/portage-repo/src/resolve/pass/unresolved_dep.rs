//! A dependency with no version to merge: feedback for backtracking, and its unsatisfied entry.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;
use std::ops::ControlFlow;

/// The values [`record_unresolved_dep`] reads from its caller.
pub(crate) struct RecordUnresolvedDepArgs<'a> {
    pub(crate) current_atom: &'a str,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) key: (String, String),
    pub(crate) outcome: PretendOutcome,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) state: &'a mut PassState,
    pub(crate) config: &'a Config,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) atom_root: &'a Path,
    pub(crate) item_targets_running_root: bool,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) depth: u32,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) atom: &'a Atom,
}

/// The values [`maybe_missing_dep_backtrack`] reads from its caller.
pub(crate) struct MaybeMissingDepBacktrackArgs<'a> {
    pub(crate) current_atom: &'a str,
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) key: &'a (String, String),
    pub(crate) state: &'a mut PassState,
    pub(crate) config: &'a Config,
    pub(crate) owner: &'a Option<(String, String)>,
}

/// Remember a missing-dep backtrack trigger for a dependency that matches
/// nothing: with backtracking live and a merge-bound parent not yet tried,
/// mask `!=parent-cpv` and retry. The entry still builds so an unfixable dep
/// reports unchanged.
pub(crate) fn maybe_missing_dep_backtrack(args: MaybeMissingDepBacktrackArgs<'_>) {
    let MaybeMissingDepBacktrackArgs {
        current_atom,
        outcome,
        ctx,
        bp,
        key,
        state,
        config,
        owner,
    } = args;
    // The dep matches nothing. With backtracking live and a merge-bound parent
    // not yet tried, mask `!=parent-cpv` and retry (one trigger per pass); the
    // entry still builds below so an unfixable dep reports unchanged.
    // Portage: depgraph.py (_feedback_missing_dep), backtracking.py
    let bare_atom = current_atom
        .split_once('[')
        .map_or(current_atom, |(b, _)| b);
    let masked_probe = bp.masked_negatives_for(key);
    if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && ctx.backtrack_max > 0
        && state.missing_dep_trigger.is_none()
        && !atom_currently_satisfiable(&ctx.repos, bare_atom, config, &masked_probe)
        && let Some((pc, pp)) = owner.clone()
    {
        let parent_cpv = state
            .entries
            .iter()
            .find(|e| e.category == pc && e.package == pp)
            .and_then(|e| match &e.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    Some(version.clone())
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some(to.clone())
                }
                _ => None,
            });
        if let Some(pv) = parent_cpv {
            let neg = format!("!={pc}/{pp}-{pv}");
            if !bp.missing_dep_masked.contains(&neg) {
                state.missing_dep_trigger = Some(((pc, pp), neg, current_atom.to_string()));
            }
        } else {
            // An installed parent whose dep can't be satisfied masks its highest
            // instance instead (Portage masks by package object; `[installed]`
            // hides only that instance, so the same-version ebuild stays visible
            // for the next pass).
            if let Some(iv) = installed_candidates(ctx.root, &pc, &pp)
                .into_iter()
                .map(|(v, _, _)| v)
                .max_by(|a, b| vercmp_ordering(a, b))
            {
                let neg = format!("!={pc}/{pp}-{iv}[installed]");
                if !bp.missing_dep_masked.contains(&neg) {
                    state.missing_dep_trigger = Some(((pc, pp), neg, current_atom.to_string()));
                }
            }
        }
    }
}

/// The values [`record_installed_slot_collision`] reads from its caller.
pub(crate) struct RecordInstalledSlotCollisionArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) atom_root: &'a Path,
    pub(crate) key: &'a (String, String),
    pub(crate) item_targets_running_root: bool,
    pub(crate) state: &'a mut PassState,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) current_atom: &'a str,
}

/// Record the installed-vs-merge slot collision for an `AlreadyInstalled`
/// atom beside a merge-bound one in the same slot.
///
/// Returns the installed node's slot for the repeat-dedup key below; the vdb
/// `SLOT` read is reused there.
pub(crate) fn record_installed_slot_collision(
    args: RecordInstalledSlotCollisionArgs<'_>,
) -> Option<String> {
    let RecordInstalledSlotCollisionArgs {
        outcome,
        atom_root,
        key,
        item_targets_running_root,
        state,
        ctx,
        config,
        current_atom,
    } = args;
    // An installed instance beside a merge-bound one in the same slot that this
    // atom rejects is a slot collision (Portage's `_package_tracker` holds both
    // nodes). Recorded before the `other_outcomes` dedup, since a repeat visit
    // under a different atom text is how the second constraint arrives. No
    // `continue`: the solvable/unsolvable split is the backtracker's
    // (`collect_feedback` reconciles a jointly-satisfiable slot on retry).
    // The vdb `SLOT` file only (one read per installed node); the fuller lookup
    // is deferred to the rare branches that build a record.
    let installed_node_slot = match outcome {
        PretendOutcome::AlreadyInstalled { version } => {
            // The instance lives in this atom's own resolving root.
            Some(read_vdb_slot(atom_root, &key.0, &key.1, version).0)
        }
        _ => None,
    };
    if let PretendOutcome::AlreadyInstalled { version } = outcome {
        let inst_slot = installed_node_slot.clone().unwrap_or_default();
        let slot_key = (
            key.0.clone(),
            key.1.clone(),
            inst_slot.clone(),
            item_targets_running_root,
        );
        if let Some(&existing_idx) = state.resolved_slots.get(&slot_key)
            && let Some(existing_version) =
                merge_bound_version(&state.entries[existing_idx].outcome).cloned()
        {
            // Same sub-slot-carrying match string as the merge-outcome check
            // (a built `cat/pkg:0/2=` puller matches no `:slot`-only string).
            let existing_sub = slot_conflict_meta(&ctx.repos, &key.0, &key.1, &existing_version).0;
            let existing_str = if existing_sub.is_empty() {
                format!("{}/{}-{existing_version}:{inst_slot}", key.0, key.1)
            } else {
                format!(
                    "{}/{}-{existing_version}:{inst_slot}/{existing_sub}",
                    key.0, key.1
                )
            };
            if portage_dep::match_from_list(current_atom, &[existing_str.as_str()])
                        .is_none_or(|m| m.is_empty())
                        // Rebuild-trigger forgiveness needs a backtrack budget (the scan
                        // schedules the consumer); at `--backtrack=0` Portage's in-pass
                        // solver sees the conflict and withholds the update.
                        && !(ctx.backtrack_max > 0
                            && built_slot_operator_rebuild_trigger(current_atom, &existing_str))
            {
                record_slot_conflict(
                    &mut state.slot_conflicts,
                    build_slot_conflict(BuildSlotConflictArgs {
                        repos: &ctx.repos,
                        config,
                        root: ctx.root,
                        category: &key.0,
                        package: &key.1,
                        slot: &inst_slot,
                        existing_version: &existing_version,
                        current_atom,
                        current_version: version,
                        slot_pullers: &state.slot_pullers,
                        installed_side: Some(InstalledSide::Current),
                    }),
                );
            }
        }
        state
            .installed_slots
            .entry(slot_key)
            .or_insert_with(|| version.clone());
    }
    installed_node_slot
}

/// What [`compute_nvc_suggestions`] hands back to its caller.
pub(crate) struct NvcSuggestionsOut {
    pub(crate) keyword_suggestion: Option<(String, String)>,
    pub(crate) use_suggestion: Option<(String, Vec<(String, bool)>)>,
    pub(crate) parent_use_suggestion: Option<ParentUseSuggestion>,
}

/// The values [`compute_nvc_suggestions`] reads from its caller.
pub(crate) struct ComputeNvcSuggestionsArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) atom: &'a Atom,
    pub(crate) config: &'a Config,
    pub(crate) owner: Option<&'a (String, String)>,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) entries: &'a [GraphEntry],
}

/// The `--autounmask` keyword, USE and parent-USE suggestions for a
/// dependency's own miss.
pub(crate) fn compute_nvc_suggestions(args: ComputeNvcSuggestionsArgs<'_>) -> NvcSuggestionsOut {
    let ComputeNvcSuggestionsArgs {
        outcome,
        bp,
        ctx,
        key,
        atom,
        config,
        owner,
        unevaluated_atom,
        entries,
    } = args;
    // Keyword suggestion for a dependency's own miss (see
    // `GraphEntry::keyword_suggestion`).
    let keyword_suggestion = if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && bp.autounmask_suggest_keywords
    {
        suggested_keyword_candidate(&ctx.repos, &key.0, &key.1, config)
    } else {
        None
    };
    // `atom.use_deps` is already conditional-evaluated (only plain `flag` /
    // `-flag` forms survive). See `GraphEntry::use_suggestion`.
    let use_suggestion =
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && bp.autounmask_suggest_use {
            suggested_use_candidate(&ctx.repos, &key.0, &key.1, atom.use_deps.as_deref(), config)
        } else {
            None
        };
    // Parent-use suggestion needs a conditional use-dep evaluated away and a
    // parent to flip (every miss here is a dependency's, so `owner` holds).
    // See `suggested_parent_use_candidate`.
    let parent_use_suggestion =
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && bp.autounmask_suggest_use {
            owner
                .zip(unevaluated_atom.as_deref())
                .and_then(|(owner, unevaluated)| {
                    suggested_parent_use_candidate(&ctx.repos, entries, unevaluated, owner, config)
                })
        } else {
            None
        };
    NvcSuggestionsOut {
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
    }
}

/// The values [`push_unresolved_entry`] reads from its caller.
pub(crate) struct PushUnresolvedEntryArgs<'a> {
    pub(crate) key: (String, String),
    pub(crate) outcome: PretendOutcome,
    pub(crate) already_installed_deps: Vec<DepEdge>,
    pub(crate) keyword_suggestion: Option<(String, String)>,
    pub(crate) use_suggestion: Option<(String, Vec<(String, bool)>)>,
    pub(crate) parent_use_suggestion: Option<ParentUseSuggestion>,
    pub(crate) item_targets_running_root: bool,
    pub(crate) state: &'a mut PassState,
}

/// Push the unsatisfied entry carrying its resolving root and suggestions.
pub(crate) fn push_unresolved_entry(args: PushUnresolvedEntryArgs<'_>) {
    let PushUnresolvedEntryArgs {
        key,
        outcome,
        already_installed_deps,
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
        item_targets_running_root,
        state,
    } = args;
    state.entries.push(GraphEntry {
        discovery: 0,
        category: key.0,
        package: key.1,
        outcome,
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: None,
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
        // The node carries its resolving root (Portage's `Package.root_config`).
        targets_running_root: item_targets_running_root,
        remote_binary: false,
        build_id: None,
        deps: already_installed_deps,
    });
}
