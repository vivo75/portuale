//! Re-expanding a resolved slot when USE changes.

use crate::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

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
        let existing_cand_str = format!(
            "{}/{}-{existing_version2}:{}/{}::{}",
            key2.0, key2.1, existing_cand.slot, existing_cand.sub_slot, existing_cand.repo_name
        );
        let existing_use = effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &existing_cand.iuse,
            keywords: &existing_cand.keywords,
            candidate_str: &existing_cand_str,
            build_id: existing_cand.build_id.as_deref(),
            category: &key2.0,
            package: &key2.1,
        });
        if !portage_dep::use_deps_satisfied_union(
            use_deps,
            &declared,
            &[&config.iuse_effective],
            &existing_use,
        ) {
            return false;
        }
    }
    true
}

/// Arguments of [`expand_resolved_slot_with_flipped_use`].
pub(crate) struct ExpandResolvedSlotWithFlippedUseArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) state: &'a mut PassState,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) key: &'a (String, String),
    pub(crate) existing_idx: usize,
    pub(crate) existing_cand: &'a Candidate,
    pub(crate) existing_version: &'a str,
    pub(crate) new_use: &'a HashSet<String>,
}

/// In-pass delta re-expansion for an already-resolved slot's
/// `--autounmask-use` flip: re-flattens the resolved package's dependency
/// strings under the flipped USE and queues every genuinely new atom through
/// the normal machinery, as if the flip had landed before the package's walk.
/// Additive only: pre-flip atoms stay queued, and merge-order edges are added
/// only for newly-queued targets. A no-op unless the flip reveals new atoms.
/// Only with `--autounmask-backtrack=y` does the driver re-drive the walk.
// Portage: depgraph.py, _add_pkg / _create_graph (dep-stack ordering)
pub(crate) fn expand_resolved_slot_with_flipped_use(
    args: ExpandResolvedSlotWithFlippedUseArgs<'_>,
) {
    let ExpandResolvedSlotWithFlippedUseArgs {
        ctx,
        config,
        union_constraints,
        state,
        bp,
        key,
        existing_idx,
        existing_cand,
        existing_version,
        new_use,
    } = args;
    // Portage never flips a built package: the delta only ever applies to a
    // fresh ebuild entry, the only shape re-read below.
    if state.entries[existing_idx].source != CandidateSource::Ebuild || ctx.nodeps {
        return;
    }
    let pf = format!("{}-{existing_version}", key.1);
    let Ok(delta_meta) = repo_aux_metadata(&existing_cand.repo_location, &key.0, &pf) else {
        return;
    };
    // The delta atoms queue one deeper than the flipped package's own depth.
    let slot = state.entries[existing_idx].slot.clone().unwrap_or_default();
    let existing_depth = state
        .resolved_depths
        .get(&(key.0.clone(), key.1.clone(), slot.clone()))
        .copied()
        .unwrap_or(0);
    // Same key-inclusion rule as the fresh expansion.
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
    // `RDEPEND`-first discovery order for the merge-order edges below, which
    // only point at newly-queued atoms.
    let portage_order_keys: &[&str] = if buildpkgonly_narrow {
        &["DEPEND", "BDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    };
    // Same build-time/run-time classification as the fresh expansion.
    let buildtime_atoms = flatten_keys(&["DEPEND", "BDEPEND"], &delta_meta, new_use);
    let runtime_atoms = flatten_keys(&["RDEPEND", "PDEPEND", "IDEPEND"], &delta_meta, new_use);
    // `BDEPEND` alone, for the same running-`BDEPEND`-soft rule as the fresh
    // expansion.
    let bdepend_atoms = flatten_keys(&["BDEPEND"], &delta_meta, new_use);
    // The flipped entry's own per-group roots, same rule as the fresh
    // expansion.
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
    // The `||` groups defer to the disjunctive stack (resolved at drain
    // against the graph as it stands), while the plains queue in-walk as
    // before. A malformed shape falls back to today's path: the splitter
    // returns its input unsplit, so there are no groups to defer.
    // Portage: depgraph.py, _queue_disjunctive_deps
    let (plains_tokens, groups) = split_off_disjunctions(&tokens, new_use);
    let flat_deps = match flatten_deps_for_flip(FlattenDepsForFlipArgs {
        state,
        tokens: &plains_tokens,
        new_use,
        delta_meta: &delta_meta,
        cross_running_root,
        running_tags: &running_tags,
        ctx,
        config,
        union_constraints,
        existing_idx,
        key,
        bp,
    }) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(()) => return,
    };
    // Same `--root-deps` running-root split as the fresh expansion (flag-on
    // coinciding-roots only).
    let root_deps_split: Option<&Path> = ctx
        .root_deps_running_root
        .filter(|_| cross_running_root.is_none());
    // (No-ops unless `root_deps_split` is set.)
    let root_deps_satisfied: HashSet<String> = root_deps_split
        .map(|root| {
            root_deps_satisfied_atoms(RootDepsSatisfiedAtomsArgs {
                metadata: &delta_meta,
                use_flags: new_use,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
        })
        .unwrap_or_default();
    let root_deps_unsatisfied: Vec<String> = root_deps_split
        .map(|root| {
            unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
                metadata: &delta_meta,
                use_flags: new_use,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
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
                ResolveRootDepsBuildEntriesArgs {
                    repos: &ctx.repos,
                    running_root,
                    atom_str,
                    config,
                    owner: key.clone(),
                    seen: &mut state.root_deps_build_seen,
                    local_binpkg: &ctx.local_binpkg,
                },
            ));
        }
    }
    // Deferred groups resolve at drain against the graph as it stands then;
    // pushing before the plains enqueue keeps an early return below from
    // dropping them.
    if !groups.is_empty() {
        state.disjunctive_stack.push(DeferredDisjunction {
            groups,
            use_flags: new_use.clone(),
            empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
            owner: key.clone(),
            owner_version: existing_version.to_string(),
            depth: existing_depth,
            owner_targets_running_root: state.entries[existing_idx].targets_running_root,
            site: DeferredSite::Flip {
                buildtime_atoms: buildtime_atoms.clone(),
                runtime_atoms: runtime_atoms.clone(),
                bdepend_atoms: bdepend_atoms.clone(),
                or_universe: or_group_universe(&tokens),
                running_tags: running_tags.clone(),
                root_deps_satisfied: root_deps_satisfied.clone(),
                root_deps_unsatisfied: root_deps_unsatisfied.clone(),
                delta_meta: delta_meta.clone(),
                buildpkgonly_narrow,
                slot,
            },
        });
    }
    if enqueue_flipped_tokens(EnqueueFlippedTokensArgs {
        state,
        flat_deps: &flat_deps,
        new_use,
        ctx,
        config,
        union_constraints,
        bp,
        running_tags: &running_tags,
        key,
        existing_version,
        existing_depth,
        buildtime_atoms: &buildtime_atoms,
        runtime_atoms: &runtime_atoms,
        bdepend_atoms: &bdepend_atoms,
        tokens: &tokens,
        existing_idx,
        delta_meta: &delta_meta,
        portage_order_keys,
    })
    .is_break()
    {
        return;
    }
    enqueue_test_deps_for_flip(EnqueueTestDepsForFlipArgs {
        ctx,
        existing_depth,
        new_use,
        delta_meta: &delta_meta,
        key,
        existing_cand,
        existing_version,
        config,
        tokens: &tokens,
        state,
        existing_idx,
    });
}

/// The values [`flatten_deps_for_flip`] reads from its caller.
pub(crate) struct FlattenDepsForFlipArgs<'a> {
    pub(crate) state: &'a mut PassState,
    pub(crate) tokens: &'a [String],
    pub(crate) new_use: &'a HashSet<String>,
    pub(crate) delta_meta: &'a Arc<HashMap<String, String>>,
    pub(crate) cross_running_root: Option<&'a Path>,
    pub(crate) running_tags: &'a HashSet<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) existing_idx: usize,
    pub(crate) key: &'a (String, String),
    pub(crate) bp: &'a BacktrackParams,
}

/// Flatten the flipped dependency strings, choosing one branch of each disjunction as the walk does.
pub(crate) fn flatten_deps_for_flip(
    args: FlattenDepsForFlipArgs<'_>,
) -> ControlFlow<(), Vec<String>> {
    let FlattenDepsForFlipArgs {
        state,
        tokens,
        new_use,
        delta_meta,
        cross_running_root,
        running_tags,
        ctx,
        config,
        union_constraints,
        existing_idx,
        key,
        bp,
    } = args;
    let queued: Vec<QueueItem> = state.queue.iter().cloned().collect();
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens,
            uselist: new_use,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                delta_meta.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
            tie_break: &mut |alts: &[Vec<String>]| {
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
                let entries_view: &[GraphEntry] =
                    scoped_entries.as_deref().unwrap_or(&state.entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
                    queued
                        .iter()
                        .filter(|q| q.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
                promote_tied_alternative(PromoteTiedAlternativeArgs {
                    repos: &ctx.repos,
                    config,
                    root: grp_root,
                    entries: entries_view,
                    constraints: union_constraints,
                    alts,
                    queued: queued_view,
                })
            },
        },
        &mut |atoms: &[String]| {
            let grp_running = group_running(atoms, cross_running_root, running_tags);
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
            // The node key carries the flipped entry's own root; the gate reads
            // the whole graph.
            let self_node: RootCp = (
                state.entries[existing_idx].targets_running_root,
                key.0.clone(),
                key.1.clone(),
            );
            disjunction_preference(DisjunctionPreferenceArgs {
                repos: &ctx.repos,
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: &self_node,
                constraints: union_constraints,
                circular_dependency: &bp.circular_dependency,
                root_deps_running_root: ctx.root_deps_running_root,
                atoms,
                queued: queued_view,
                update: ctx.update,
                gate_entries: &state.entries,
                gate_queued: &queued,
            })
        },
    ) else {
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(flat_deps)
}

/// The values [`enqueue_flipped_tokens`] reads from its caller.
pub(crate) struct EnqueueFlippedTokensArgs<'a> {
    pub(crate) state: &'a mut PassState,
    pub(crate) flat_deps: &'a [String],
    pub(crate) new_use: &'a HashSet<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) running_tags: &'a HashSet<String>,
    pub(crate) key: &'a (String, String),
    pub(crate) existing_version: &'a str,
    pub(crate) existing_depth: u32,
    pub(crate) buildtime_atoms: &'a HashSet<String>,
    pub(crate) runtime_atoms: &'a HashSet<String>,
    pub(crate) bdepend_atoms: &'a HashSet<String>,
    pub(crate) tokens: &'a [String],
    pub(crate) existing_idx: usize,
    pub(crate) delta_meta: &'a Arc<HashMap<String, String>>,
    pub(crate) portage_order_keys: &'a [&'a str],
}

/// Queue the genuinely new atoms of the flipped dependency strings and add their merge-order edges.
pub(crate) fn enqueue_flipped_tokens(args: EnqueueFlippedTokensArgs<'_>) -> ControlFlow<()> {
    let EnqueueFlippedTokensArgs {
        state,
        flat_deps,
        new_use,
        ctx,
        config,
        union_constraints,
        bp,
        running_tags,
        key,
        existing_version,
        existing_depth,
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
        tokens,
        existing_idx,
        delta_meta,
        portage_order_keys,
    } = args;
    // Only genuinely new atoms flow, compared in evaluated form: re-queueing a
    // visited/queued atom would duplicate puller rows and re-resolve it.
    let queued_atoms: HashSet<&str> = state.queue.iter().map(|q| q.atom.as_str()).collect();
    let mut new_toks: Vec<String> = Vec::new();
    for tok in flat_deps {
        let evaluated =
            portage_dep::evaluate_atom_conditionals(tok, new_use).unwrap_or_else(|| tok.clone());
        if state.visited_atoms.contains(&evaluated) || queued_atoms.contains(evaluated.as_str()) {
            continue;
        }
        new_toks.push(tok.clone());
    }
    if new_toks.is_empty() {
        return ControlFlow::Break(());
    }
    // A text-new atom that cleanly re-resolves to its settled slot is dropped
    // before any bookkeeping, or the pop would synthesize the phantom edge.
    let mut kept_toks: Vec<String> = Vec::new();
    for tok in &new_toks {
        let evaluated =
            portage_dep::evaluate_atom_conditionals(tok, new_use).unwrap_or_else(|| tok.clone());
        // The tag is raw-keyed: `tok` is still raw here (evaluated only for
        // the lookup).
        if delta_atom_cleanly_reuses_resolved_slot(DeltaAtomCleanlyReusesResolvedSlotArgs {
            ctx,
            config,
            union_constraints,
            state,
            bp,
            evaluated: &evaluated,
            atom_running: running_tags.contains(tok),
        }) {
            continue;
        }
        kept_toks.push(tok.clone());
    }
    if kept_toks.is_empty() {
        return ControlFlow::Break(());
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
    enqueue_flat_deps(EnqueueFlatDepsArgs {
        flat_deps: new_toks.clone(),
        key,
        version: existing_version,
        depth: existing_depth,
        parent_use: new_use,
        queue: &mut state.queue,
        pending_blockers: &mut state.pending_blockers,
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
        or_universe: &or_group_universe(tokens),
        running_atoms: running_tags,
        owner_targets_running_root: state.entries[existing_idx].targets_running_root,
    });
    // Merge-order edges are added ONLY for newly-queued targets: unioning the
    // whole flipped flatten would re-add edges for already-graphed targets and
    // over-constrain the walk. Pre-flip edges stay as the fresh expansion left
    // them.
    let new_targets: HashSet<(String, String)> = new_toks
        .iter()
        .filter_map(|tok| {
            portage_dep::parse_atom(tok)
                .map(|dep_atom| (dep_atom.category.clone(), dep_atom.package.clone()))
        })
        .collect();
    if !new_targets.is_empty() {
        let fresh_edges =
            merge_order::dep_edges_from_metadata(delta_meta, new_use, portage_order_keys, false);
        let entry_deps = &mut state.entries[existing_idx].deps;
        for edge in fresh_edges {
            if new_targets.contains(&(edge.category.clone(), edge.package.clone()))
                && !entry_deps.contains(&edge)
            {
                entry_deps.push(edge);
            }
        }
    }
    ControlFlow::Continue(())
}

/// The values [`enqueue_test_deps_for_flip`] reads from its caller.
pub(crate) struct EnqueueTestDepsForFlipArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) existing_depth: u32,
    pub(crate) new_use: &'a HashSet<String>,
    pub(crate) delta_meta: &'a Arc<HashMap<String, String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) existing_cand: &'a Candidate,
    pub(crate) existing_version: &'a str,
    pub(crate) config: &'a Config,
    pub(crate) tokens: &'a [String],
    pub(crate) state: &'a mut PassState,
    pub(crate) existing_idx: usize,
}

/// Queue the test? dependencies the USE flip newly enables (top-level only).
pub(crate) fn enqueue_test_deps_for_flip(args: EnqueueTestDepsForFlipArgs<'_>) {
    let EnqueueTestDepsForFlipArgs {
        ctx,
        existing_depth,
        new_use,
        delta_meta,
        key,
        existing_cand,
        existing_version,
        config,
        tokens,
        state,
        existing_idx,
    } = args;
    // `--with-test-deps` follow-up (top-level only): the flip may newly enable
    // `test?` deps the pre-flip USE hid.
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
                existing_cand.build_id.as_deref(),
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
                tokens,
                &test_uselist,
                portage_use_reduce::MatchMode::Normal,
                &subset,
            ) {
                // Same genuinely-new filter as the main delta above.
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
                enqueue_flat_deps(EnqueueFlatDepsArgs {
                    flat_deps: new_test_deps,
                    key,
                    version: existing_version,
                    depth: existing_depth,
                    parent_use: &test_uselist,
                    queue: &mut state.queue,
                    pending_blockers: &mut state.pending_blockers,
                    buildtime_atoms: &HashSet::new(),
                    runtime_atoms: &HashSet::new(),
                    bdepend_atoms: &HashSet::new(),
                    or_universe: &HashSet::new(), // A `test?` dep stays target-rooted.
                    running_atoms: &HashSet::new(),
                    owner_targets_running_root: state.entries[existing_idx].targets_running_root,
                });
            }
        }
    }
}

/// Whether every atom of the group belongs to the running root.
fn group_running(
    atoms: &[String],
    cross_running_root: Option<&Path>,
    running_tags: &HashSet<String>,
) -> bool {
    cross_running_root.is_some() && atoms.iter().all(|a| running_tags.contains(a))
}

/// Flatten the dependency strings of `keys` under `new_use` into their atoms.
fn flatten_keys(
    keys: &[&str],
    delta_meta: &HashMap<String, String>,
    new_use: &HashSet<String>,
) -> HashSet<String> {
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
}
