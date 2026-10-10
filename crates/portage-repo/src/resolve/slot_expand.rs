//! Re-expanding a resolved slot when USE changes.

use crate::*;

/// Backlog #218 (L0 20260928T063310Z): eager clean-reuse classification
/// for delta atoms.
///
/// Real's rule (`3rdparty/portage`, `lib/_emerge/depgraph.py`): the
/// flipped package was already *walked* pre-flip, so a newly-gated atom
/// that names an already-graphed slot never re-enters real's `_dep_stack`
/// (`if not previously_added: dep_stack.append(pkg)`, `_add_pkg`, :3820)
/// and records no `_add_parent_atom` owner row (`:3583-3604`).
/// Live-verified on `gui-libs/gtk:4`: real walks freetype with `harfbuzz`
/// off (its `Children` are bzip2 + libpng only) and the `harfbuzz` flip
/// lands later via pango's `freetype-2.5.0.1:2[harfbuzz,...]` atom -- so
/// real's graph carries no freetype -> harfbuzz edge at all.
///
/// Portuale's delta queues through the normal machinery, so without this
/// filter a delta atom that merely re-names a settled slot still pops:
/// the pop folds the flipped package into `required_by_map` (plus the
/// edge-kind map, slot want-lists and pullers), and `build_digraph`'s
/// `required_by` fallback then synthesizes exactly the phantom edge real
/// never has -- reordering Kahn's walk for unrelated packages (gtk:4 went
/// set-identical but #30 -> #22 with findings byte-identical, and even
/// dropping the narrowed new-target `deps` edges did not move it).
///
/// So each text-new delta atom is pre-resolved here with the pop site's
/// own arguments and dropped -- never queued, so no puller row and no
/// pop-time bookkeeping at all -- when the pop is provably
/// bookkeeping-only: a merge-bound outcome for the already-resolved
/// version of an already-resolved slot, with the atom's slot and use-dep
/// constraints satisfied (or absent). Anything else -- a genuinely new
/// slot, a version or slot conflict, unsatisfied use-deps that may fold a
/// further (nested-cascade) flip, an installed or unsatisfiable outcome, a
/// blocker -- queues exactly as before. `resolve_pretend` writes no
/// global state (the unparsed-token counter fires at the pop site, not
/// inside it), so the speculative resolution is side-effect free; and its
/// inputs are all pass-fixed (`ctx`, `config`, `union_constraints`), so
/// the verdict cannot drift between here and the pop.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn delta_atom_cleanly_reuses_resolved_slot(
    ctx: &ResolveCtx,
    config: &portage_profile::Config,
    union_constraints: &HashMap<(String, String), Vec<String>>,
    state: &PassState,
    bp: &BacktrackParams,
    evaluated: &str,
    // Track X Slice B (#242): this delta atom's own resolving root (its
    // queue tag) -- the speculative selection and the slot lookup both
    // run against it, so a running-root atom never reuses a target-root
    // slot (and vice versa).
    atom_running: bool,
) -> bool {
    let Some(atom) = portage_dep::parse_atom(evaluated) else {
        return false;
    };
    if atom.blocker != portage_dep::Blocker::None {
        return false;
    }
    let key2 = (atom.category.clone(), atom.package.clone());
    // Complete mode diverts dependency atoms around `resolve_pretend`
    // (graph-or-installed only); never second-guess that branch.
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
    // Only a merge-bound outcome can hit the already-resolved branch
    // below; an installed or unsatisfiable outcome takes the pop site's
    // own installed/NVC paths, which this filter must not swallow.
    let version = match &outcome {
        PretendOutcome::New { version } => version.clone(),
        PretendOutcome::Upgrade { to, .. } => to.clone(),
        PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::Reinstall { version, .. } => version.clone(),
        _ => return false,
    };
    // The same winning-candidate pick the pop site resolves the slot
    // from ("ebuild type is the last resort", then repo priority).
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
        // A genuinely new slot: the aucascleaf case. Queue it.
        return false;
    };
    let existing_version2 = match &state.entries[existing_idx2].outcome {
        PretendOutcome::New { version } => version.clone(),
        PretendOutcome::Upgrade { to, .. } => to.clone(),
        PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::Reinstall { version, .. } => version.clone(),
        // `resolved_slots` only ever indexes these four (the pop site's
        // own `unreachable!`); conservative here, never a drop.
        _ => return false,
    };
    if version != existing_version2 {
        // A version conflict: the pop site records it. Queue it.
        return false;
    }
    // Include the already-resolved version's own sub-slot, exactly like
    // the pop site (a built `cat/pkg:0/2=` puller matches no `:slot`-only
    // string).
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
        // The pop site's bound-instance side record would fire. Queue it.
        return false;
    }
    // `match_from_list` matched version + slot only: a `[flag]` use-dep
    // still has to be satisfied by the settled package, else the pop
    // site folds a flip (possibly a nested cascade). Only a satisfied
    // atom is a clean reuse.
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
        let existing_use = effective_use_flags(
            config,
            &existing_cand.iuse,
            &existing_cand.keywords,
            &existing_cand_str,
            &key2.0,
            &key2.1,
        );
        if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use) {
            return false;
        }
    }
    true
}

/// Backlog #218: in-pass delta re-expansion for an already-resolved
/// slot's `--autounmask-use` flip.
///
/// Real's rule (`3rdparty/portage`, `lib/_emerge/depgraph.py`): `_add_dep`
/// only pushes onto `_dep_stack` (`_add_pkg`, :3820: `if not
/// previously_added: dep_stack.append(pkg)`), and `_create_graph`
/// (:3254-3271) pops that stack LIFO -- so `aucasctop`'s two deps are
/// both *added* before either is *walked*, `aucasclate` pops first, and
/// its `aucascmid[cascade]` atom flips the still-unwalked `aucascmid`
/// through `_pkg_use_enabled(pkg, target_use)` (:7669), which records
/// `_needed_use_config_changes` synchronously. When `aucascmid` then pops,
/// `_add_pkg_deps` (:4151) reduces `cascade? ( dev-libs/aucascleaf )`
/// with the flip already in force, so `aucascleaf` joins the graph in the
/// same attempt (live-verified: `fixtures/abort-captures/
/// dev-libs_aucasctop.pretend-debug.stdout` shows one continuous walk --
/// `aucascmid` added `USE="-cascade"`, flipped by `aucasclate`'s dep,
/// then walked with `cascade?` live).
///
/// Portuale fuses resolve+expand at pop time (FIFO), so the flipped
/// package's `flag?`-gated deps were already flattened pre-flip by the
/// time the later atom's already-resolved-slot re-check folds the flip.
/// This helper restores real's observable ordering within the same pass:
/// it re-flattens the already-resolved package's dependency strings under
/// the flipped USE and queues every genuinely new atom through the normal
/// `enqueue_flat_deps` machinery (owner, depth, disjunction preference,
/// pullers, merge-order edges), exactly as if the flip had landed before
/// the package's walk. Only with `--autounmask-backtrack=y` does the
/// driver additionally re-drive the whole walk (`autounmask_grew`); the
/// default keeps real's single attempt.
///
/// Additive only, by design: atoms the pre-flip expansion already queued
/// stay queued (a flag turned *off* cannot unqueue them mid-pass -- the
/// narrowing half is reconciled by the next whole-walk pass, same as the
/// pre-#218 behaviour), and merge-order edges are added only for
/// newly-queued targets -- never re-derived for already-graphed ones
/// (a whole-flatten union over-constrained Kahn's walk and reordered
/// unrelated packages on L0). A no-op unless the flip actually reveals
/// new atoms, so every non-autounmask walk is untouched.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn expand_resolved_slot_with_flipped_use(
    ctx: &ResolveCtx,
    config: &portage_profile::Config,
    union_constraints: &HashMap<(String, String), Vec<String>>,
    state: &mut PassState,
    bp: &BacktrackParams,
    key: &(String, String),
    existing_idx: usize,
    existing_cand: &Candidate,
    existing_version: &str,
    new_use: &HashSet<String>,
) {
    // Real never flips a built package (`_pkg_use_enabled` returns the
    // baked USE for `pkg.built`, `depgraph.py:7676`; `can_adjust_use =
    // not pkg.built`, `:8131`): the delta only ever applies to a fresh
    // ebuild entry, which is also the only shape whose md5-cache
    // metadata this re-reads below. (`resolved_slots` only ever indexes
    // New/Upgrade/Downgrade/Reinstall outcomes, so no outcome check.)
    if state.entries[existing_idx].source != CandidateSource::Ebuild || ctx.nodeps {
        return;
    }
    let pf = format!("{}-{existing_version}", key.1);
    let Ok(delta_meta) = repo_aux_metadata(&existing_cand.repo_location, &key.0, &pf) else {
        return;
    };
    // The flipped package's own depth (real `pkg.depth`): the delta atoms
    // queue one deeper, exactly where the fresh expansion put its own.
    let slot = state.entries[existing_idx].slot.clone().unwrap_or_default();
    let existing_depth = state
        .resolved_depths
        .get(&(key.0.clone(), key.1.clone(), slot))
        .copied()
        .unwrap_or(0);
    // The same key-inclusion rule as the fresh expansion above (an
    // ebuild source, so only the `--buildpkgonly`-without-`--deep`
    // runtime blanking applies).
    let buildpkgonly_narrow = ctx.buildpkgonly && matches!(ctx.deep, Deep::NotRequested);
    let dep_keys: &[&str] = if buildpkgonly_narrow {
        &["DEPEND", "BDEPEND"]
    } else {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    };
    let mut depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = delta_meta.get(*dep_key) {
            depstr.push_str(d);
            depstr.push(' ');
        }
    }
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    // Real `RDEPEND`-first discovery order for the merge-order edges,
    // same `portage_order_keys` as the fresh expansion above. Applied
    // below, after the genuinely-new filter: only edges pointing at a
    // newly-queued atom are added (see the narrowing note there).
    let portage_order_keys: &[&str] = if buildpkgonly_narrow {
        &["DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    };
    // The same build-time/run-time classification as the fresh expansion
    // above (an ebuild source always classifies from both key sets).
    let flatten_keys = |keys: &[&str]| -> HashSet<String> {
        let joined: String = keys
            .iter()
            .filter_map(|k| delta_meta.get(*k))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(&toks, new_use, portage_use_reduce::MatchMode::Normal)
            .map(|v| v.into_iter().filter(|t| t != "||").collect())
            .unwrap_or_default()
    };
    let buildtime_atoms = flatten_keys(&["DEPEND", "BDEPEND"]);
    let runtime_atoms = flatten_keys(&["RDEPEND", "PDEPEND", "IDEPEND"]);
    // Track X Slice B fix (#242): `BDEPEND` alone, for the same
    // running-`BDEPEND`-soft rule the fresh expansion applies (see
    // `enqueue_flat_deps`'s own `bdepend_atoms` doc comment).
    let bdepend_atoms = flatten_keys(&["BDEPEND"]);
    // Track X Slice B (#242): the flipped entry's own per-group roots --
    // same rule as the fresh expansion (a flipped running-root entry
    // re-resolves its build groups against the running root).
    let cross_running_root: Option<&Path> = ctx.cross_root();
    let running_tags: HashSet<String> = cross_running_root
        .map(|_| {
            dep_running_tags(
                &delta_meta,
                new_use,
                state.entries[existing_idx].targets_running_root,
            )
        })
        .unwrap_or_default();
    let group_running = |atoms: &[String]| -> bool {
        cross_running_root.is_some() && atoms.iter().all(|a| running_tags.contains(a))
    };
    let queued: Vec<QueueItem> = state.queue.iter().cloned().collect();
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        &tokens,
        new_use,
        portage_use_reduce::MatchMode::Normal,
        md5_dict::eapi_empty_groups_always_true(
            delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
        ),
        &mut |atoms: &[String]| {
            let grp_running = group_running(atoms);
            let grp_root: &Path = if grp_running {
                cross_running_root.expect("running-root group without a cross root")
            } else {
                ctx.root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_running_root.map(|_| {
                state
                    .entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(&state.entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
            // Track X Slice C (#242): the node key carries the flipped
            // entry's own root (real's node-identity keying); the gate
            // reads the whole graph (same slices the bins would see on
            // a same-root run).
            let self_node: RootCp = (
                state.entries[existing_idx].targets_running_root,
                key.0.clone(),
                key.1.clone(),
            );
            disjunction_preference(
                &ctx.repos,
                config,
                grp_root,
                entries_view,
                &self_node,
                union_constraints,
                &bp.circular_dependency,
                ctx.root_deps_running_root,
                atoms,
                queued_view,
                ctx.update,
                &state.entries,
                &queued,
            )
        },
        &mut |alts: &[Vec<String>]| {
            let grp_running = cross_running_root.is_some()
                && alts
                    .iter()
                    .flat_map(|v| v.iter())
                    .next()
                    .is_some_and(|a| running_tags.contains(a));
            let grp_root: &Path = if grp_running {
                cross_running_root.expect("running-root group without a cross root")
            } else {
                ctx.root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_running_root.map(|_| {
                state
                    .entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(&state.entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
            promote_tied_alternative(
                &ctx.repos,
                config,
                grp_root,
                entries_view,
                union_constraints,
                alts,
                queued_view,
            )
        },
    ) else {
        return;
    };
    // The same `--root-deps` running-root split as the fresh expansion
    // above (Track X Slice B: flag-on coinciding-roots only -- the unified
    // per-group walk owns every cross-root atom).
    let root_deps_split: Option<&Path> = ctx
        .root_deps_running_root
        .filter(|_| cross_running_root.is_none());
    // above (both no-ops unless `root_deps_split` is set).
    let root_deps_satisfied: HashSet<String> = root_deps_split
        .map(|root| {
            root_deps_satisfied_atoms(
                &delta_meta,
                new_use,
                &ctx.repos,
                config,
                root,
                &["DEPEND", "BDEPEND", "IDEPEND"],
                md5_dict::eapi_empty_groups_always_true(
                    delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            )
        })
        .unwrap_or_default();
    let root_deps_unsatisfied: Vec<String> = root_deps_split
        .map(|root| {
            unsatisfied_root_deps_atoms(
                &delta_meta,
                new_use,
                &ctx.repos,
                config,
                root,
                &["DEPEND", "BDEPEND", "IDEPEND"],
                md5_dict::eapi_empty_groups_always_true(
                    delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            )
        })
        .unwrap_or_default();
    let flat_deps: Vec<String> = flat_deps
        .into_iter()
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, new_use)
                .unwrap_or_else(|| tok.clone());
            !root_deps_satisfied.contains(&evaluated)
        })
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, new_use)
                .unwrap_or_else(|| tok.clone());
            !root_deps_unsatisfied.contains(&evaluated)
        })
        .collect();
    if let Some(running_root) = ctx.root_deps_running_root {
        for atom_str in &root_deps_unsatisfied {
            state.entries.extend(resolve_root_deps_build_entries(
                &ctx.repos,
                running_root,
                atom_str,
                config,
                key.clone(),
                &mut state.root_deps_build_seen,
                &ctx.local_binpkg,
            ));
        }
    }
    // Only genuinely new atoms flow: anything the pre-flip expansion
    // already visited or queued keeps its single resolution (re-queueing
    // it would duplicate puller rows and re-resolve a settled atom).
    // Compared in evaluated form, the same text `visited_atoms` and the
    // queue itself carry.
    let queued_atoms: HashSet<&str> = state.queue.iter().map(|q| q.atom.as_str()).collect();
    let mut new_toks: Vec<String> = Vec::new();
    for tok in &flat_deps {
        let evaluated =
            portage_dep::evaluate_atom_conditionals(tok, new_use).unwrap_or_else(|| tok.clone());
        if state.visited_atoms.contains(&evaluated) || queued_atoms.contains(evaluated.as_str()) {
            continue;
        }
        new_toks.push(tok.clone());
    }
    if new_toks.is_empty() {
        return;
    }
    // L0 20260928T063310Z: a text-new atom that cleanly re-resolves to
    // its already-settled slot (`delta_atom_cleanly_reuses_resolved_slot`)
    // is dropped before any bookkeeping -- queueing it would record the
    // flipped package as an owner at pop and synthesize the phantom edge
    // real never has. The genuinely-new survivors below are untouched.
    let mut kept_toks: Vec<String> = Vec::new();
    for tok in &new_toks {
        let evaluated =
            portage_dep::evaluate_atom_conditionals(tok, new_use).unwrap_or_else(|| tok.clone());
        // Track X Slice B (#242): the tag is raw-keyed -- `tok` here is
        // still the raw survivor (evaluated only for the lookup below).
        if delta_atom_cleanly_reuses_resolved_slot(
            ctx,
            config,
            union_constraints,
            state,
            bp,
            &evaluated,
            running_tags.contains(tok),
        ) {
            continue;
        }
        kept_toks.push(tok.clone());
    }
    if kept_toks.is_empty() {
        return;
    }
    let new_toks = kept_toks;
    for tok in &new_toks {
        if let Some(dep_atom) = portage_dep::parse_atom(tok)
            && dep_atom.blocker == portage_dep::Blocker::None
        {
            state
                .slot_pullers
                .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                .or_default()
                .push((
                    key.0.clone(),
                    key.1.clone(),
                    existing_version.to_string(),
                    tok.clone(),
                ));
        }
    }
    enqueue_flat_deps(
        new_toks.clone(),
        key,
        existing_version,
        existing_depth,
        new_use,
        &mut state.queue,
        &mut state.pending_blockers,
        &buildtime_atoms,
        &runtime_atoms,
        &bdepend_atoms,
        &or_group_universe(&tokens),
        &running_tags,
        state.entries[existing_idx].targets_running_root,
    );
    // Narrowing (L0 20260928T054625Z): merge-order edges are added ONLY
    // for newly-queued targets. Unioning the whole flipped flatten here
    // also re-added edges for already-graphed targets (a `||` re-choice
    // under the later queue state, an evaluated-form difference), which
    // over-constrained Kahn's walk and reordered unrelated packages
    // (gui-libs/gtk:4 went set-identical but order-shifted). Pre-flip
    // edges stay exactly as the fresh expansion left them -- the
    // additive approximation; the flipped package's genuinely new deps
    // still get their real edge (and cycle classification) below.
    let new_targets: HashSet<(String, String)> = new_toks
        .iter()
        .filter_map(|tok| {
            portage_dep::parse_atom(tok)
                .map(|dep_atom| (dep_atom.category.clone(), dep_atom.package.clone()))
        })
        .collect();
    if !new_targets.is_empty() {
        let fresh_edges =
            merge_order::dep_edges_from_metadata(&delta_meta, new_use, portage_order_keys, false);
        let entry_deps = &mut state.entries[existing_idx].deps;
        for edge in fresh_edges {
            if new_targets.contains(&(edge.category.clone(), edge.package.clone()))
                && !entry_deps.contains(&edge)
            {
                entry_deps.push(edge);
            }
        }
    }
    // `--with-test-deps` follow-up, same gating as the fresh expansion
    // above (top-level packages only): the flipped USE may newly enable
    // `test?` deps the pre-flip `use_flags` hid.
    if ctx.with_test_deps && existing_depth == 0 && !new_use.contains("test") {
        let iuse_flags: HashSet<String> = delta_meta
            .get("IUSE")
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        let candidate_str = format!(
            "{}/{}-{existing_version}:{}/{}::{}",
            key.0, key.1, existing_cand.slot, existing_cand.sub_slot, existing_cand.repo_name
        );
        let test_masked = config.use_mask.contains("test")
            || specificity_ordered_flags(
                &config.package_use_mask,
                &candidate_str,
                &key.0,
                &key.1,
                HashSet::new(),
            )
            .contains("test");
        if iuse_flags.contains("test") && !test_masked {
            let mut test_uselist = new_use.clone();
            test_uselist.insert("test".to_string());
            let subset: HashSet<String> = ["test".to_string()].into_iter().collect();
            if let Ok(test_deps) = portage_use_reduce::use_reduce_flat_subset(
                &tokens,
                &test_uselist,
                portage_use_reduce::MatchMode::Normal,
                &subset,
            ) {
                // Same genuinely-new filter as the main delta above: the
                // pre-flip expansion already queued this package's test
                // deps under the old USE (evaluated here against the
                // flipped test uselist, like the fresh path evaluates at
                // queue time).
                let queued_now: HashSet<&str> =
                    state.queue.iter().map(|q| q.atom.as_str()).collect();
                let new_test_deps: Vec<String> = test_deps
                    .into_iter()
                    .filter(|tok| {
                        let evaluated = portage_dep::evaluate_atom_conditionals(tok, &test_uselist)
                            .unwrap_or_else(|| tok.clone());
                        !state.visited_atoms.contains(&evaluated)
                            && !queued_now.contains(evaluated.as_str())
                    })
                    .collect();
                enqueue_flat_deps(
                    new_test_deps,
                    key,
                    existing_version,
                    existing_depth,
                    &test_uselist,
                    &mut state.queue,
                    &mut state.pending_blockers,
                    &HashSet::new(),
                    &HashSet::new(),
                    &HashSet::new(),
                    &HashSet::new(),
                    // Track X Slice B (#242): a `test?` dep stays
                    // target-rooted (same gate as the fresh expansion).
                    &HashSet::new(),
                    state.entries[existing_idx].targets_running_root,
                );
            }
        }
    }
}
