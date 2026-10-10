//! A dependency with no version to merge: feedback for backtracking, and its unsatisfied entry.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

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
    // Real `_feedback_missing_dep`: this dependency atom has
    // no matching package. If backtracking is live and its
    // parent is a merge-bound entry not yet tried, remember to
    // mask `!=parent-cpv` and retry -- so a `||` group whose
    // chosen alternative has an unsatisfiable subtree yields to
    // the next alternative, and so a top-level-selected version
    // whose dependency is unmaskable yields to the next older
    // version (backlog #198: `akk0a-2` masked, `akk0a-1` picked,
    // real `depgraph.py:3493-3503` + `backtracking.py:197-207`
    // carry no top-level exemption -- the mask names one cpv,
    // never the requested package itself). Only the first
    // such trigger per pass is acted on (like real, one
    // backtrack node at a time); the NVC entry is still built
    // below so an un-fixable dep is reported unchanged.
    // Real (depgraph.py:3473-3483) does NOT missing-dep
    // backtrack when only a USE change would satisfy the dep
    // (`_select_package(dep.atom.without_use)` still finds a
    // package) -- that path is the autounmask machinery's, and
    // an unfixable `[flag]` dep is reported as a plain NVC.
    // C3: real's "no matching package" is mask-aware (a masked
    // candidate matches nothing), so the probe consults the
    // negatives, not the bare tree: a dependency unsatisfiable
    // only because backtracking masked its candidates masks its
    // parent and retries, reaching the same downgrade the old
    // puller pre-masking produced (e.g. `btconsumer-2.0` under a
    // masked `bttarget-2.0` falls back to `btconsumer-1.0` on
    // the retry). Positives stay out (real selection has no
    // enforcement counterpart); a USE-only failure still
    // satisfies the probe and stays out, exactly as before.
    let bare_atom = current_atom
        .split_once('[')
        .map_or(current_atom, |(b, _)| b);
    let masked_probe = bp.masked_negatives_for(&key);
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
            // Backlog #276: real masks an *installed* parent
            // whose dep cannot be satisfied (`_add_dep`
            // `depgraph.py:3456-3470` -> `runtime_pkg_mask`
            // "missing dependency") and restarts; the
            // merge-bound-only arming above never fires for it,
            // so the run aborts on the installed parent's built
            // atom where real steers. Fall back to the highest
            // installed instance. The `[installed]` marker hides
            // only that instance (real keys the mask by package
            // object -- see [`is_installed_only_mask`]), so the
            // same-version ebuild stays visible and the next
            // pass walks it, exactly like real's second
            // backtracking try masking the ebuild.
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
    // #57, direction 1: this atom settled on an *installed*
    // instance, and the same `(cat, pkg, slot)` may already hold a
    // merge-bound one that this atom rejects -- real's
    // `_package_tracker` sees both nodes and calls that a slot
    // collision (`othermod`'s `<paired-2.0` landing on installed
    // 1.0 while `needer`'s `>=paired-2.0` already graphed 2.0).
    // Recorded *before* the `other_outcomes` dedup, since a
    // repeat visit of this `cat/pkg` under a different atom text
    // is exactly how the second, conflicting constraint arrives.
    //
    // Deliberately no `continue`: real keeps the installed node
    // in the graph and still merges the other instance, and the
    // solvable/unsolvable split (real
    // `_solve_non_slot_operator_slot_conflicts`) is the
    // backtracker's -- `collect_feedback`'s `slot_want`
    // solvability check reconciles a jointly-satisfiable slot on
    // the retry and this record disappears with it; only a slot
    // no single version can satisfy survives to be reported.
    // The vdb `SLOT` file only -- one read per installed node,
    // reused by the slot-conflict record below and the
    // `other_outcomes` dedup key. `installed_refs` would re-list
    // every version and read each one's `repository` too, and
    // this runs for every `AlreadyInstalled` outcome in the walk
    // (thousands on a `-puD @world`); the fuller lookup is
    // deferred to the two rare branches that actually build a
    // record.
    let installed_node_slot = match &outcome {
        PretendOutcome::AlreadyInstalled { version } => {
            // Track X Slice B (#242): the instance lives in this
            // atom's own resolving root.
            Some(read_vdb_slot(atom_root, &key.0, &key.1, version).0)
        }
        _ => None,
    };
    if let PretendOutcome::AlreadyInstalled { version } = &outcome {
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
            // Same sub-slot-carrying match string the
            // merge-outcome check below builds, for the same
            // reason (a built `cat/pkg:0/2=` puller matches no
            // `:slot`-only string).
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
                        // #270: the "this is a rebuild trigger, not a conflict"
                        // forgiveness presumes the slot-operator rebuild scan
                        // will schedule the consumer, and that scan only runs
                        // with a backtrack budget (`backtrack_max > 0`, like
                        // its own call site). At `--backtrack=0` nothing
                        // schedules the rebuild, so real's in-pass solver
                        // (`_solve_non_slot_operator_slot_conflicts`,
                        // `depgraph.py:1774-2115`) sees the conflict and
                        // withholds the provider update instead of merging it
                        // alone (real 3.0.82.2: `[sct, oldc, newc]` only).
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
    // AlreadyInstalled repeats are keyed by the installed
    // instance's own slot (real's `pkg.slot_atom` node identity):
    // a *second* installed instance of the same cp in another
    // slot is a distinct node whose deps must still be walked
    // (#74 S2a -- the old cp key let the first slot processed
    // shadow the rest, dropping `llvm-core/clang:23`'s own build
    // deps). NoVisibleCandidate has no instance to key by: `None`
    // keeps the old cp-scoped repeat dedup.
    let other_key = (
        key.0.clone(),
        key.1.clone(),
        installed_node_slot,
        item_targets_running_root,
    );
    if !state.other_outcomes.insert(other_key) {
        // #57: a deduped `NoVisibleCandidate` never gets its own
        // entry, so under an active `runtime_pkg_mask` the
        // `has_nvc` scan must still treat the pass as a dead end
        // (real's `_create_graph` returns 0 on an unsatisfiable
        // dep and the attempt is abandoned in favour of
        // `get_best_run`; the argv order `othermod needer` used
        // to settle on the masked try otherwise). Gated on an
        // active mask so no unmasked pass changes behaviour.
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && !bp.runtime_pkg_mask.is_empty()
        {
            state.suppressed_nvc = true;
        }
        // Backlog #286: a deduped miss still refreshes the
        // disclosure when its atom is the live one real reports
        // (see `live_miss_atom_supersedes`): the repeat edge
        // carries the atom the first edge lacked, and real's
        // fail-fast walk would have hit it first. Rows refresh
        // alongside (both row builders read the new atom only).
        // Cheap gate first: the row builders run only when the
        // recorded atom is a supersedeable built one.
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
            let display_atom = unevaluated_atom.as_deref().unwrap_or(current_atom);
            if state
                .nvc_dep_atoms
                .get(&key)
                .is_some_and(|old| live_miss_atom_supersedes(old, display_atom))
            {
                let masked_rows = masked_candidates_for_atom(&ctx.repos, display_atom, config);
                let use_rows = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
                    repos: &ctx.repos,
                    root: ctx.root,
                    entries: &state.entries,
                    display_atom,
                    evaluated_atom: current_atom,
                    owner: owner.as_ref(),
                    config,
                });
                state.nvc_dep_atoms.entry(key.clone()).and_modify(|old| {
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
        return;
    }
    // `--deep`: an AlreadyInstalled package's own further
    // dependencies are walked too, once `deep` allows recursion
    // at this package's own depth (see `Deep::recurses_at`'s own
    // doc comment) -- never for NoVisibleCandidate (no version to
    // look anything up by), and never when `nodeps` disables the
    // dependency walk entirely, matching every other outcome's
    // own `nodeps` handling further below. Real's own `_add_pkg`
    // gate is the same: an installed package whose deps aren't
    // recursed into gets pushed onto `_ignored_deps`, never
    // `.order`, so `deps` (below) is likewise only ever
    // computed under this identical condition.
    let mut already_installed_deps: Vec<DepEdge> = Vec::new();
    if let PretendOutcome::AlreadyInstalled { version } = &outcome
        && !ctx.nodeps
        && ctx.deep.recurses_at(depth)
    {
        // `GraphEntry::deps` for an AlreadyInstalled
        // entry: A2 (#26) builds it from the *same* installed
        // metadata view the recursion below walks
        // (`installed_dep_string`): the live ebuild's deps plus
        // the vdb's own built `:=` atoms by default (backlog #25
        // S1b), the vdb snapshot under `--dynamic-deps=n`.
        // So `--debug`'s `Depstring` line and `--tree`'s edges can no
        // longer contradict the child the walk actually queued.
        // Backlog #86: the display list reads the same
        // recorded-repo view the recursion walks (never a
        // priority search), so `--debug`/`--tree` cannot
        // contradict the queued child.
        // Track X Slice B (#242): this installed instance lives
        // in the atom's own resolving root (real `pkg.root`).
        if let Some(metadata) =
            live_metadata_for_installed(&ctx.repos, atom_root, &key.0, &key.1, version)
        {
            // Installed recorded USE, not effective profile
            // USE -- see `enqueue_dependencies`'s own note
            // just below; a `flag?`-gated dep this display
            // list shows must match what the recursion
            // actually queued.
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
            // An installed package is `pkg.built`, so
            // real marks its build-time deps `optional`.
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
    // `--autounmask`'s own keyword-suggestion sub-feature,
    // extended here to a *dependency's* own `NoVisibleCandidate`
    // -- see `GraphEntry::keyword_suggestion`'s own doc comment.
    let keyword_suggestion = if matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && bp.autounmask_suggest_keywords
    {
        suggested_keyword_candidate(&ctx.repos, &key.0, &key.1, config)
    } else {
        None
    };
    // `--autounmask-use`'s own suggestion sub-feature -- see
    // `GraphEntry::use_suggestion`'s own doc comment. `atom.
    // use_deps` is the dependency atom's own use-dep spec
    // (already conditional-evaluated by `enqueue_flat_deps`
    // before this atom was ever queued, so only plain `flag`/
    // `-flag` forms survive to be checked here).
    let use_suggestion =
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && bp.autounmask_suggest_use {
            suggested_use_candidate(&ctx.repos, &key.0, &key.1, atom.use_deps.as_deref(), config)
        } else {
            None
        };
    // `--autounmask-use`'s own second, architecturally distinct
    // suggestion sub-feature -- see `suggested_parent_use_
    // candidate`'s own doc comment. Only ever attempted when
    // this atom actually had a conditional use-dep evaluated
    // away (`unevaluated_atom.is_some()`) and has a real parent
    // to flip a flag on (`owner.is_some()`, always true here: a
    // top-level atom's own `NoVisibleCandidate` already aborted
    // the whole call via the fatal check above, so any
    // `NoVisibleCandidate` reaching this point is necessarily a
    // dependency's own, which always has an `owner`).
    let parent_use_suggestion =
        if matches!(outcome, PretendOutcome::NoVisibleCandidate) && bp.autounmask_suggest_use {
            owner
                .as_ref()
                .zip(unevaluated_atom.as_deref())
                .and_then(|(owner, unevaluated)| {
                    suggested_parent_use_candidate(
                        &ctx.repos,
                        &state.entries,
                        unevaluated,
                        owner,
                        config,
                    )
                })
        } else {
            None
        };
    // Masked-dependency disclosure (real
    // `_show_unsatisfied_dep`'s "All ebuilds that could satisfy
    // … have been masked" block for a *dependency* -- the Tier
    // 2.20 half of finding S's top-level report): a dependency
    // `NoVisibleCandidate` whose atom still matches ebuilds by
    // version, every one of which is masked, is recorded here
    // (one per `cat/pkg`, like the entry itself) instead of
    // rendering the bare `!!! no visible ebuild` line. The
    // display atom is the unevaluated form as queued, so a
    // USE-conditional dep shows as written. Computed every
    // pass; only the final pass's reports are rendered.
    if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
        let display_atom = unevaluated_atom.as_deref().unwrap_or(current_atom);
        state
            .nvc_dep_atoms
            .entry(key.clone())
            .and_modify(|old| {
                // Backlog #286: a live slot-operator miss
                // supersedes a built one (see
                // `live_miss_atom_supersedes`).
                if live_miss_atom_supersedes(old, display_atom) {
                    *old = display_atom.to_string();
                }
            })
            .or_insert_with(|| display_atom.to_string());
        // A dep the `'parent_flip` off-arm collected into a
        // parent USE flip records no disclosure at all: real
        // drops the collected item from
        // `_unsatisfied_deps_for_display` (`depgraph.py:5820`
        // `remaining_items`), so neither the masked block, nor
        // the use-unsat block, nor the plain-miss block prints
        // for it -- the change block is its whole output.
        // (`nvc_dep_atoms` above is kept: `abort_outcome` still
        // needs the atom for the abort reason.)
        if !state.parent_flip_recorded.contains(&key) {
            if let Some(masked) = masked_candidates_for_atom(&ctx.repos, display_atom, config) {
                // Backlog #286: live supersedes built (see
                // `live_miss_atom_supersedes`); the rows were just
                // computed for the new atom, so a supersede
                // refreshes them too.
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
                        // Walked post-loop out of the final entries
                        // (see below).
                        chain: Vec::new(),
                    }),
                }
            }
            // USE-unsatisfied-dependency disclosure (backlog #20,
            // real `_show_unsatisfied_dep`'s separate "no ebuilds
            // built with USE flags to satisfy" path): a dependency
            // `NoVisibleCandidate` whose atom names version-matching
            // ebuilds that are *visible* but cannot satisfy the
            // atom's own `[use]` deps. Real's precedence puts this
            // block before the masked one (`show_missing_use` wins),
            // and the caller renders it first. Computed every pass;
            // only the final pass's reports are rendered.
            if let Some(rows) = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
                repos: &ctx.repos,
                root: ctx.root,
                entries: &state.entries,
                display_atom,
                evaluated_atom: current_atom,
                owner: owner.as_ref(),
                config,
            }) {
                // Backlog #286: live supersedes built (see
                // `live_miss_atom_supersedes`); the rows were just
                // computed for the new atom, so a supersede
                // refreshes them too.
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
                // Neither sibling block claimed this miss: nothing
                // matches by version at all, or no candidate's
                // failure is a `Change USE:`/`Missing IUSE:` one
                // (e.g. a profile-forced flag). Real's
                // `emerge: there are no ebuilds to satisfy "<atom>"`
                // block (#135 (d)). Backlog #286: live supersedes
                // built (see `live_miss_atom_supersedes`).
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
        // Track X Slice B (#242): the node carries its resolving
        // root -- real `Package.root_config` / `pkg.root`.
        targets_running_root: item_targets_running_root,
        remote_binary: false,
        build_id: None,
        deps: already_installed_deps,
    });
}
