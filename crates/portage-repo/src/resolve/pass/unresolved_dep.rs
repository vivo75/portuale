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

/// The values [`dedup_repeat_visit`] reads from its caller.
pub(crate) struct DedupRepeatVisitArgs<'a> {
    pub(crate) key: &'a (String, String),
    pub(crate) installed_node_slot: Option<String>,
    pub(crate) item_targets_running_root: bool,
    pub(crate) state: &'a mut PassState,
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) current_atom: &'a str,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) owner: Option<&'a (String, String)>,
}

/// Drop repeat visits: repeats key by the installed instance's own slot, so a
/// second installed instance in another slot is a distinct node whose deps
/// still walk. `Break` when this visit was already recorded.
pub(crate) fn dedup_repeat_visit(args: DedupRepeatVisitArgs<'_>) -> ControlFlow<()> {
    let DedupRepeatVisitArgs {
        key,
        installed_node_slot,
        item_targets_running_root,
        state,
        outcome,
        bp,
        unevaluated_atom,
        current_atom,
        ctx,
        config,
        owner,
    } = args;
    // Repeats key by the installed instance's own slot (Portage's
    // `pkg.slot_atom` identity): a second installed instance in another slot is
    // a distinct node whose deps still walk. Misses keep cp-scoped dedup.
    let other_key = (
        key.0.clone(),
        key.1.clone(),
        installed_node_slot,
        item_targets_running_root,
    );
    if !state.other_outcomes.insert(other_key) {
        // A deduped miss gets no entry, so under an active mask the pass must
        // still read as a dead end (Portage abandons the attempt for
        // `get_best_run`). Gated on an active mask.
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && !bp.runtime_pkg_mask.is_empty()
        {
            state.suppressed_nvc = true;
        }
        // A deduped miss still refreshes the disclosure when its atom is the
        // live one Portage reports (see `live_miss_atom_supersedes`).
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
            let display_atom = unevaluated_atom.as_deref().unwrap_or(current_atom);
            if state
                .nvc_dep_atoms
                .get(key)
                .is_some_and(|old| live_miss_atom_supersedes(old, display_atom))
            {
                let masked_rows = masked_candidates_for_atom(&ctx.repos, display_atom, config);
                let use_rows = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
                    repos: &ctx.repos,
                    root: ctx.root,
                    entries: &state.entries,
                    display_atom,
                    evaluated_atom: current_atom,
                    owner,
                    config,
                });
                state.nvc_dep_atoms.entry((*key).clone()).and_modify(|old| {
                    if live_miss_atom_supersedes(old, display_atom) {
                        *old = display_atom.to_string();
                    }
                });
                for rep in state
                    .masked_deps
                    .iter_mut()
                    .filter(|r| r.category == key.0 && r.package == key.1)
                {
                    if live_miss_atom_supersedes(&rep.atom, display_atom) {
                        rep.atom = display_atom.to_string();
                        if let Some(masked) = masked_rows.clone() {
                            rep.masked = masked;
                        }
                    }
                }
                for rep in state
                    .use_unsat_deps
                    .iter_mut()
                    .filter(|r| r.category == key.0 && r.package == key.1)
                {
                    if live_miss_atom_supersedes(&rep.atom, display_atom) {
                        rep.atom = display_atom.to_string();
                        if let Some(rows) = use_rows.clone() {
                            rep.rows = rows;
                        }
                    }
                }
                for rep in state
                    .plain_miss_deps
                    .iter_mut()
                    .filter(|r| r.category == key.0 && r.package == key.1)
                {
                    if live_miss_atom_supersedes(&rep.atom, display_atom) {
                        rep.atom = display_atom.to_string();
                    }
                }
            }
        }
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}

/// The values [`enqueue_installed_deps`] reads from its caller.
pub(crate) struct EnqueueInstalledDepsArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) depth: u32,
    pub(crate) atom_root: &'a Path,
    pub(crate) key: &'a (String, String),
    pub(crate) state: &'a mut PassState,
    pub(crate) config: &'a Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Queue an installed package's own deps too once `deep` allows recursion at
/// this depth (never for a miss, never under `nodeps`).
///
/// Returns the `deps` display list, computed under the same condition.
pub(crate) fn enqueue_installed_deps(args: EnqueueInstalledDepsArgs<'_>) -> Vec<DepEdge> {
    let EnqueueInstalledDepsArgs {
        outcome,
        ctx,
        depth,
        atom_root,
        key,
        state,
        config,
        union_constraints,
    } = args;
    // An installed package's own deps walk too once `deep` allows recursion at
    // this depth (never for a miss, never under `nodeps`); `deps` below is
    // computed under the same condition. Portage: _add_pkg
    // (_ignored_deps vs order).
    let mut already_installed_deps: Vec<DepEdge> = Vec::new();
    if let PretendOutcome::AlreadyInstalled { version } = outcome
        && !ctx.nodeps
        && ctx.deep.recurses_at(depth)
    {
        // `deps` reads the same installed-metadata view the recursion walks, so
        // `--debug`/`--tree` can't contradict the queued child.
        if let Some(metadata) =
            live_metadata_for_installed(&ctx.repos, atom_root, &key.0, &key.1, version)
        {
            // Installed recorded USE, not effective profile USE, so a `flag?`-gated
            // dep shown here matches what the recursion queued.
            let use_flags = read_vdb_flag_set(atom_root, &key.0, &key.1, version, "USE");
            let portage_order_keys: &[&str] = if ctx.with_bdeps {
                &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
            } else {
                &["RDEPEND", "IDEPEND", "PDEPEND"]
            };
            let layer = if ctx.dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            let mut effective: HashMap<String, String> = HashMap::new();
            for k in portage_order_keys {
                let s = installed_dep_string(InstalledDepStringArgs {
                    root: atom_root,
                    dynamic_deps: ctx.dynamic_deps,
                    ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
                    category: &key.0,
                    package: &key.1,
                    version,
                    live: Some(&metadata),
                    key: k,
                    layer,
                    memo: &mut state.installed_meta_memo,
                });
                if !s.trim().is_empty() {
                    effective.insert((*k).to_string(), s);
                }
            }
            // An installed package is `pkg.built`: build-time deps are `optional`.
            already_installed_deps = merge_order::dep_edges_from_metadata(
                &effective,
                &use_flags,
                portage_order_keys,
                true,
            );
        }
        enqueue_dependencies(
            &SelectSource {
                repos: &ctx.repos,
                root: atom_root,
                config,
                local_binpkg: &ctx.local_binpkg,
            },
            &DepOwner {
                target_root: ctx.root,
                category: &key.0,
                package: &key.1,
                version,
                child_depth: depth + 1,
            },
            &ctx.enqueue_options(),
            EnqueueSinks {
                queue: &mut state.queue,
                pending_blockers: &mut state.pending_blockers,
                entries: &mut state.entries,
                root_deps_build_seen: &mut state.root_deps_build_seen,
                installed_meta_memo: &mut state.installed_meta_memo,
                slot_pullers: &mut state.slot_pullers,
            },
            union_constraints,
        );
    }
    already_installed_deps
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

/// The values [`record_miss_disclosure`] reads from its caller.
pub(crate) struct RecordMissDisclosureArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) current_atom: &'a str,
    pub(crate) key: &'a (String, String),
    pub(crate) state: &'a mut PassState,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) owner: Option<&'a (String, String)>,
}

/// Record the masked / USE-unsatisfied / plain-miss disclosure for a
/// dependency `NoVisibleCandidate` (one per `cat/pkg`). A parent-flip rescue
/// records no disclosure at all.
pub(crate) fn record_miss_disclosure(args: RecordMissDisclosureArgs<'_>) {
    let RecordMissDisclosureArgs {
        outcome,
        unevaluated_atom,
        current_atom,
        key,
        state,
        ctx,
        config,
        owner,
    } = args;
    // Masked-dependency disclosure: a miss matching only masked ebuilds records
    // the "all masked" block (one per `cat/pkg`) instead of the bare line. The
    // display atom is the unevaluated form, so conditional deps show as written.
    // Computed every pass; only the final pass renders. Portage:
    // _show_unsatisfied_dep.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
        let display_atom = unevaluated_atom.as_deref().unwrap_or(current_atom);
        state
            .nvc_dep_atoms
            .entry((*key).clone())
            .and_modify(|old| {
                // A live slot-operator miss supersedes a built one.
                if live_miss_atom_supersedes(old, display_atom) {
                    *old = display_atom.to_string();
                }
            })
            .or_insert_with(|| display_atom.to_string());
        // A parent-flip rescue records no disclosure at all (Portage drops the
        // item from display); the change block is its whole output.
        // `nvc_dep_atoms` is still kept for the abort reason.
        if !state.parent_flip_recorded.contains(key) {
            if let Some(masked) = masked_candidates_for_atom(&ctx.repos, display_atom, config) {
                // A supersede refreshes the just-computed rows too.
                match state
                    .masked_deps
                    .iter_mut()
                    .find(|r| r.category == key.0 && r.package == key.1)
                {
                    Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                        r.atom = display_atom.to_string();
                        r.masked = masked;
                    }
                    Some(_) => {}
                    None => state.masked_deps.push(MaskedDepReport {
                        category: key.0.clone(),
                        package: key.1.clone(),
                        atom: display_atom.to_string(),
                        masked,
                        // Chains walk post-loop out of the final entries.
                        chain: Vec::new(),
                    }),
                }
            }
            // USE-unsatisfied disclosure: visible ebuilds that can't satisfy the
            // atom's `[use]` deps. Portage renders this block before the masked
            // one (`show_missing_use` wins).
            if let Some(rows) = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
                repos: &ctx.repos,
                root: ctx.root,
                entries: &state.entries,
                display_atom,
                evaluated_atom: current_atom,
                owner,
                config,
            }) {
                // A supersede refreshes the just-computed rows too.
                match state
                    .use_unsat_deps
                    .iter_mut()
                    .find(|r| r.category == key.0 && r.package == key.1)
                {
                    Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                        r.atom = display_atom.to_string();
                        r.rows = rows;
                    }
                    Some(_) => {}
                    None => state.use_unsat_deps.push(UseUnsatDepReport {
                        category: key.0.clone(),
                        package: key.1.clone(),
                        atom: display_atom.to_string(),
                        rows,
                        chain: Vec::new(),
                    }),
                }
            } else if !state
                .masked_deps
                .iter()
                .any(|r: &MaskedDepReport| r.category == key.0 && r.package == key.1)
                && !state
                    .use_unsat_deps
                    .iter()
                    .any(|r: &UseUnsatDepReport| r.category == key.0 && r.package == key.1)
                && !state
                    .plain_miss_deps
                    .iter()
                    .any(|r: &PlainMissDepReport| r.category == key.0 && r.package == key.1)
            {
                // Neither sibling claimed the miss: nothing version-matches, or no
                // failure is a USE one (e.g. a profile-forced flag). Portage's
                // `there are no ebuilds to satisfy` block.
                match state
                    .plain_miss_deps
                    .iter_mut()
                    .find(|r| r.category == key.0 && r.package == key.1)
                {
                    Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                        r.atom = display_atom.to_string();
                    }
                    Some(_) => {}
                    None => state.plain_miss_deps.push(PlainMissDepReport {
                        category: key.0.clone(),
                        package: key.1.clone(),
                        atom: display_atom.to_string(),
                        chain: Vec::new(),
                    }),
                }
            }
        }
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

/// A dependency with no version to merge: feedback for backtracking, and its unsatisfied entry.
pub(crate) fn record_unresolved_dep(args: RecordUnresolvedDepArgs<'_>) {
    let RecordUnresolvedDepArgs {
        current_atom,
        bp,
        key,
        outcome,
        ctx,
        state,
        config,
        owner,
        atom_root,
        item_targets_running_root,
        unevaluated_atom,
        depth,
        union_constraints,
        atom,
    } = args;
    maybe_missing_dep_backtrack(MaybeMissingDepBacktrackArgs {
        current_atom,
        outcome: &outcome,
        ctx,
        bp,
        key: &key,
        state: &mut *state,
        config,
        owner,
    });
    let installed_node_slot = record_installed_slot_collision(RecordInstalledSlotCollisionArgs {
        outcome: &outcome,
        atom_root,
        key: &key,
        item_targets_running_root,
        state: &mut *state,
        ctx,
        config,
        current_atom,
    });
    if dedup_repeat_visit(DedupRepeatVisitArgs {
        key: &key,
        installed_node_slot,
        item_targets_running_root,
        state: &mut *state,
        outcome: &outcome,
        bp,
        unevaluated_atom,
        current_atom,
        ctx,
        config,
        owner: owner.as_ref(),
    })
    .is_break()
    {
        return;
    }
    let already_installed_deps = enqueue_installed_deps(EnqueueInstalledDepsArgs {
        outcome: &outcome,
        ctx,
        depth,
        atom_root,
        key: &key,
        state: &mut *state,
        config,
        union_constraints,
    });
    let NvcSuggestionsOut {
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
    } = compute_nvc_suggestions(ComputeNvcSuggestionsArgs {
        outcome: &outcome,
        bp,
        ctx,
        key: &key,
        atom,
        config,
        owner: owner.as_ref(),
        unevaluated_atom,
        entries: &state.entries,
    });
    record_miss_disclosure(RecordMissDisclosureArgs {
        outcome: &outcome,
        unevaluated_atom,
        current_atom,
        key: &key,
        state: &mut *state,
        ctx,
        config,
        owner: owner.as_ref(),
    });
    push_unresolved_entry(PushUnresolvedEntryArgs {
        key,
        outcome,
        already_installed_deps,
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
        item_targets_running_root,
        state: &mut *state,
    });
}
