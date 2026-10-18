//! Portage's deferred `||` stack (`_dep_disjunctive_stack`).
//!
//! Portage never chooses an `||` branch while expanding the owner: it queues
//! the plain deps immediately and pushes every `||` group (one entry per
//! `(pkg, dep_root, priority)`, the whole group list resolved together) onto
//! `_dep_disjunctive_stack`. `_create_graph` pops one entry only when the
//! plain `_dep_stack` is fully drained, resolving each group against the graph
//! as it is at that moment. Choosing at expand time instead picks `x` for
//! `rb` (`B A`, `a` expands before `b`'s plain `y` is queued); deferring sees
//! `y` in the graph and picks `y`.
// Portage: _emerge/depgraph.py, _queue_disjunctive_deps,
// _pop_disjunction, _create_graph

use crate::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;

/// Split raw dep-string `tokens` into plain tokens (kept, `||`-free) and one
/// raw token list per `|| ( ... )` group (`["||", "(", ..., ")"]`, nested
/// structure inside alternatives kept intact).
///
/// `flag? ( ... )` conditionals are evaluated against `uselist` here, like
/// Portage's `use_reduce` running before `_queue_disjunctive_deps`: an
/// inactive conditional (with its groups) is dropped, an active one is
/// entered and its groups are extracted bare. On any structural surprise the
/// input returns unsplit, so the downstream flattener errors exactly as it
/// would without deferral.
pub(crate) fn split_off_disjunctions(
    tokens: &[String],
    uselist: &HashSet<String>,
) -> (Vec<String>, Vec<Vec<String>>) {
    let Some((plains, groups)) = split_tokens(tokens, uselist) else {
        return (tokens.to_vec(), Vec::new());
    };
    (plains, groups)
}

/// Recursive body of [`split_off_disjunctions`]: `None` is "bail out, the
/// caller keeps the input unsplit".
fn split_tokens(
    tokens: &[String],
    uselist: &HashSet<String>,
) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut plains: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "||" {
            let group = capture_group(tokens, i + 1)?;
            let step = group.len() + 3;
            let mut raw = vec!["||".to_string(), "(".to_string()];
            raw.extend(group);
            raw.push(")".to_string());
            groups.push(raw);
            i += step;
        } else if tok == "(" {
            let inner = capture_group(tokens, i)?;
            let (sub_plains, sub_groups) = split_tokens(&inner, uselist)?;
            // An emptied group leaves no wrapper: `flag? ( )`/`( )` would
            // fail the downstream flattener (`expected: dependency
            // string`), while contributing nothing is exactly what the
            // unflattened string reduces to.
            if !sub_plains.is_empty() {
                plains.push("(".to_string());
                plains.extend(sub_plains);
                plains.push(")".to_string());
            }
            groups.extend(sub_groups);
            i += inner.len() + 2;
        } else if tok == ")" {
            return None;
        } else if tok.ends_with('?') {
            let inner = capture_group(tokens, i + 1)?;
            let active =
                portage_use_reduce::is_active(tok, uselist, portage_use_reduce::MatchMode::Normal)
                    .ok()?;
            if active {
                let (sub_plains, sub_groups) = split_tokens(&inner, uselist)?;
                // Same emptied-wrapper drop as the plain-group arm above.
                if !sub_plains.is_empty() {
                    plains.push(tok.clone());
                    plains.push("(".to_string());
                    plains.extend(sub_plains);
                    plains.push(")".to_string());
                }
                groups.extend(sub_groups);
            }
            i += inner.len() + 3;
        } else {
            plains.push(tok.clone());
            i += 1;
        }
    }
    Some((plains, groups))
}

/// The balanced `( ... )` contents starting at `open` (the `(` itself, or the
/// index after `||`/`flag?` which must be a `(`): `None` when unbalanced or
/// malformed.
fn capture_group(tokens: &[String], open: usize) -> Option<Vec<String>> {
    if tokens.get(open).map(String::as_str) != Some("(") {
        return None;
    }
    let mut depth = 0;
    for (j, tok) in tokens.iter().enumerate().skip(open) {
        if tok == "(" {
            depth += 1;
        } else if tok == ")" {
            depth -= 1;
            if depth == 0 {
                return Some(tokens[open + 1..j].to_vec());
            }
        }
    }
    None
}

/// One deferred `||` entry: every group of a single expansion, resolved
/// together at pop time against the drained graph.
pub(crate) struct DeferredDisjunction {
    /// Raw group token lists, declaration order, each
    /// `["||", "(", ..., ")"]` with the owner's conditionals already applied.
    pub groups: Vec<Vec<String>>,
    /// The owner's effective USE at expansion (frozen: Portage evaluates
    /// `use_reduce` at `_add_pkg_deps` time and defers only the choice).
    pub use_flags: HashSet<String>,
    /// Per-package EAPI empty-group rule for the pop-time flatten.
    pub empty_groups_always_true: bool,
    pub owner: (String, String),
    pub owner_version: String,
    /// Main walk: the entry's own depth (`enqueue_flat_deps` queues at
    /// `depth + 1`). Installed walk: the children's depth outright.
    pub depth: u32,
    pub owner_targets_running_root: bool,
    pub site: DeferredSite,
}

/// The expansion site a deferred entry returns to: the two walks queue
/// chosen atoms through different functions with different context.
pub(crate) enum DeferredSite {
    Main {
        buildtime_atoms: HashSet<String>,
        runtime_atoms: HashSet<String>,
        bdepend_atoms: HashSet<String>,
        or_universe: HashSet<String>,
        running_tags: HashSet<String>,
        /// `--root-deps` existence-split sets for the pop-time filter (both
        /// empty, and no entries, unless same-root `--root-deps`).
        root_deps_satisfied: HashSet<String>,
        root_deps_unsatisfied: Vec<String>,
    },
    Installed {
        buildtime_tokens: HashSet<String>,
        or_universe: HashSet<String>,
        running_tags: HashSet<String>,
        root_deps_satisfied: HashSet<String>,
        root_deps_unsatisfied: Vec<String>,
        root_deps_split: bool,
        /// The vdb root this installed owner resolves against.
        resolve_root: PathBuf,
        cross_active: bool,
        root_deps_running_root: Option<PathBuf>,
    },
    /// An `--autounmask-use` flip's groups: the flipped package's own
    /// re-expansion (`expand_resolved_slot_with_flipped_use`), pushed instead
    /// of choosing at flip time. Plains queue in-walk as before; the groups
    /// resolve here against the drained graph, like Portage's
    /// `_dep_disjunctive_stack`, which never chooses while expanding.
    // Portage: _emerge/depgraph.py, _queue_disjunctive_deps
    Flip {
        buildtime_atoms: HashSet<String>,
        runtime_atoms: HashSet<String>,
        bdepend_atoms: HashSet<String>,
        or_universe: HashSet<String>,
        running_tags: HashSet<String>,
        root_deps_satisfied: HashSet<String>,
        root_deps_unsatisfied: Vec<String>,
        /// The flipped package's own metadata, for the pop-time merge-order
        /// edges of newly-queued targets.
        delta_meta: Arc<HashMap<String, String>>,
        /// Recomputes the merge-order key order at pop time.
        buildpkgonly_narrow: bool,
        /// The flipped slot, to find its entry for the edge push.
        slot: String,
    },
}

/// The values [`pop_disjunction`] reads from its caller.
pub(crate) struct PopDisjunctionArgs<'a> {
    pub state: &'a mut PassState,
    pub ctx: &'a ResolveCtx<'a>,
    pub config: &'a Config,
    pub bp: &'a BacktrackParams,
    pub union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Pop one deferred entry (newest first, like Portage's LIFO
/// `_dep_disjunctive_stack.pop()`) and resolve each group against the drained
/// graph, queueing the chosen atoms `from_disjunction`. `false` means the
/// stack is empty: the walk is done.
pub(crate) fn pop_disjunction(args: PopDisjunctionArgs<'_>) -> bool {
    let PopDisjunctionArgs {
        state,
        ctx,
        config,
        bp,
        union_constraints,
    } = args;
    let Some(entry) = state.disjunctive_stack.pop() else {
        return false;
    };
    let queued: Vec<QueueItem> = state.queue.iter().cloned().collect();
    for group in &entry.groups {
        let chosen = match &entry.site {
            DeferredSite::Main { running_tags, .. } => resolve_main_group(ResolveMainGroupArgs {
                group,
                entry: &entry,
                running_tags,
                queued: &queued,
                state: &*state,
                ctx,
                config,
                bp,
                union_constraints,
            }),
            DeferredSite::Installed {
                running_tags,
                resolve_root,
                cross_active,
                root_deps_running_root,
                ..
            } => resolve_installed_group(ResolveInstalledGroupArgs {
                group,
                entry: &entry,
                running_tags,
                resolve_root,
                cross_active: *cross_active,
                root_deps_running_root: root_deps_running_root.as_deref(),
                queued: &queued,
                state: &*state,
                ctx,
                config,
                union_constraints,
            }),
            DeferredSite::Flip { running_tags, .. } => resolve_flip_group(ResolveFlipGroupArgs {
                group,
                entry: &entry,
                running_tags,
                queued: &queued,
                state: &*state,
                ctx,
                config,
                bp,
                union_constraints,
            }),
        };
        let ControlFlow::Continue(chosen) = chosen else {
            continue;
        };
        enqueue_chosen_atoms(EnqueueChosenAtomsArgs {
            chosen,
            entry: &entry,
            state,
            ctx,
            config,
            bp,
            union_constraints,
        });
    }
    true
}

/// The values [`resolve_flip_group`] reads from its caller.
struct ResolveFlipGroupArgs<'a> {
    group: &'a [String],
    entry: &'a DeferredDisjunction,
    running_tags: &'a HashSet<String>,
    queued: &'a [QueueItem],
    state: &'a PassState,
    ctx: &'a ResolveCtx<'a>,
    config: &'a Config,
    bp: &'a BacktrackParams,
    union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Resolve one flip-walk group against the drained graph: the same closures
/// as the expansion-time flatten, with pop-time views and the backtrack
/// loop's own circular map (a flipped owner is never given the virtual
/// pullers join — like the expansion-time flatten).
fn resolve_flip_group(args: ResolveFlipGroupArgs<'_>) -> ControlFlow<(), Vec<String>> {
    let ResolveFlipGroupArgs {
        group,
        entry,
        running_tags,
        queued,
        state,
        ctx,
        config,
        bp,
        union_constraints,
    } = args;
    resolve_target_scope_group(ResolveTargetScopeGroupArgs {
        group,
        use_flags: &entry.use_flags,
        empty_groups_always_true: entry.empty_groups_always_true,
        owner: &entry.owner,
        owner_targets_running_root: entry.owner_targets_running_root,
        running_tags,
        circular: &bp.circular_dependency,
        queued,
        state,
        ctx,
        config,
        union_constraints,
    })
}

/// The values [`resolve_target_scope_group`] reads from its caller.
struct ResolveTargetScopeGroupArgs<'a> {
    group: &'a [String],
    use_flags: &'a HashSet<String>,
    empty_groups_always_true: bool,
    owner: &'a (String, String),
    owner_targets_running_root: bool,
    running_tags: &'a HashSet<String>,
    circular: &'a HashMap<RootCp, Vec<CircularDepChild>>,
    queued: &'a [QueueItem],
    state: &'a PassState,
    ctx: &'a ResolveCtx<'a>,
    config: &'a Config,
    union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Resolve one target-rooted deferred group against the drained graph: the
/// same closures as the expansion-time flatten, with pop-time views. Shared
/// by the main walk and the flip path so a fix to one can never silently
/// miss the other again.
fn resolve_target_scope_group(
    args: ResolveTargetScopeGroupArgs<'_>,
) -> ControlFlow<(), Vec<String>> {
    let ResolveTargetScopeGroupArgs {
        group,
        use_flags,
        empty_groups_always_true,
        owner,
        owner_targets_running_root,
        running_tags,
        circular,
        queued,
        state,
        ctx,
        config,
        union_constraints,
    } = args;
    let cross_running_root: Option<&Path> = ctx.cross_root();
    let grp_running =
        cross_running_root.is_some() && group.iter().all(|a| running_tags.contains(a));
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
    let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(queued);
    let self_node: RootCp = (owner_targets_running_root, owner.0.clone(), owner.1.clone());
    let circular_view: &HashMap<RootCp, Vec<CircularDepChild>> = circular;
    let Ok(flat) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: group,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true,
            tie_break: &mut |alts: &[Vec<String>]| {
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
            disjunction_preference(DisjunctionPreferenceArgs {
                repos: &ctx.repos,
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: &self_node,
                constraints: union_constraints,
                circular_dependency: circular_view,
                root_deps_running_root: ctx.root_deps_running_root,
                atoms,
                queued: queued_view,
                update: ctx.update,
                gate_entries: &state.entries,
                gate_queued: queued,
            })
        },
    ) else {
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(flat)
}

/// The values [`resolve_main_group`] reads from its caller.
struct ResolveMainGroupArgs<'a> {
    group: &'a [String],
    entry: &'a DeferredDisjunction,
    running_tags: &'a HashSet<String>,
    queued: &'a [QueueItem],
    state: &'a PassState,
    ctx: &'a ResolveCtx<'a>,
    config: &'a Config,
    bp: &'a BacktrackParams,
    union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Resolve one main-walk group against the drained graph: the shared
/// target-scope core, with the virtual-pullers join for a `virtual/*` owner.
fn resolve_main_group(args: ResolveMainGroupArgs<'_>) -> ControlFlow<(), Vec<String>> {
    let ResolveMainGroupArgs {
        group,
        entry,
        running_tags,
        queued,
        state,
        ctx,
        config,
        bp,
        union_constraints,
    } = args;
    let self_node: RootCp = (
        entry.owner_targets_running_root,
        entry.owner.0.clone(),
        entry.owner.1.clone(),
    );
    // The virtual-pullers join, recomputed against the drained graph (only
    // ever non-trivial for a `virtual/*` owner).
    let circ_with_pullers: Option<HashMap<RootCp, Vec<CircularDepChild>>> =
        if self_node.1 == "virtual" && !bp.circular_dependency.is_empty() {
            let mut extra: Vec<CircularDepChild> = Vec::new();
            if let Some(pullers) = state.required_by_map.get(&entry.owner) {
                let mut pullers: Vec<&(String, String)> = pullers.iter().collect();
                pullers.sort();
                for p in pullers {
                    for flag in [false, true] {
                        if let Some(children) =
                            bp.circular_dependency
                                .get(&(flag, p.0.clone(), p.1.clone()))
                        {
                            extra.extend(children.iter().cloned());
                        }
                    }
                }
            }
            (!extra.is_empty()).then(|| {
                let mut m = bp.circular_dependency.clone();
                let bucket = m.entry(self_node.clone()).or_default();
                for c in extra {
                    if !bucket.contains(&c) {
                        bucket.push(c);
                    }
                }
                m
            })
        } else {
            None
        };
    let circular_view: &HashMap<RootCp, Vec<CircularDepChild>> = circ_with_pullers
        .as_ref()
        .unwrap_or(&bp.circular_dependency);
    resolve_target_scope_group(ResolveTargetScopeGroupArgs {
        group,
        use_flags: &entry.use_flags,
        empty_groups_always_true: entry.empty_groups_always_true,
        owner: &entry.owner,
        owner_targets_running_root: entry.owner_targets_running_root,
        running_tags,
        circular: circular_view,
        queued,
        state,
        ctx,
        config,
        union_constraints,
    })
}

/// The values [`resolve_installed_group`] reads from its caller.
struct ResolveInstalledGroupArgs<'a> {
    group: &'a [String],
    entry: &'a DeferredDisjunction,
    running_tags: &'a HashSet<String>,
    resolve_root: &'a Path,
    cross_active: bool,
    root_deps_running_root: Option<&'a Path>,
    queued: &'a [QueueItem],
    state: &'a PassState,
    ctx: &'a ResolveCtx<'a>,
    config: &'a Config,
    union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Resolve one installed-walk group: same closures as
/// `flatten_deps_for_enqueue`, with pop-time views.
fn resolve_installed_group(args: ResolveInstalledGroupArgs<'_>) -> ControlFlow<(), Vec<String>> {
    let ResolveInstalledGroupArgs {
        group,
        entry,
        running_tags,
        resolve_root,
        cross_active,
        root_deps_running_root,
        queued,
        state,
        ctx,
        config,
        union_constraints,
    } = args;
    // Installed parents are never merge-bound cycle nodes: empty map, like
    // the expansion-time flatten.
    let no_circular: HashMap<RootCp, Vec<CircularDepChild>> = HashMap::new();
    let self_node: RootCp = (false, entry.owner.0.clone(), entry.owner.1.clone());
    let disj_constraints = union_constraints;
    let Ok(flat) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens: group,
            uselist: &entry.use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true: false,
            tie_break: &mut |alts: &[Vec<String>]| {
                let grp_running = cross_active
                    && alts
                        .iter()
                        .flat_map(|v| v.iter())
                        .next()
                        .is_some_and(|a| running_tags.contains(a));
                let grp_root: &Path = if grp_running {
                    root_deps_running_root.expect("running-root group without a running root")
                } else {
                    resolve_root
                };
                let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                    state
                        .entries
                        .iter()
                        .filter(|e| e.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let entries_view: &[GraphEntry] =
                    scoped_entries.as_deref().unwrap_or(&state.entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                    queued
                        .iter()
                        .filter(|q| q.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(queued);
                promote_tied_alternative(PromoteTiedAlternativeArgs {
                    repos: &ctx.repos,
                    config,
                    root: grp_root,
                    entries: entries_view,
                    constraints: disj_constraints,
                    alts,
                    queued: queued_view,
                })
            },
        },
        &mut |atoms: &[String]| {
            let grp_running = cross_active && atoms.iter().all(|a| running_tags.contains(a));
            let grp_root: &Path = if grp_running {
                root_deps_running_root.expect("running-root group without a running root")
            } else {
                resolve_root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                state
                    .entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(&state.entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(queued);
            disjunction_preference(DisjunctionPreferenceArgs {
                repos: &ctx.repos,
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: &self_node,
                constraints: disj_constraints,
                circular_dependency: &no_circular,
                root_deps_running_root: ctx.root_deps_running_root,
                atoms,
                queued: queued_view,
                update: ctx.update,
                gate_entries: &state.entries,
                gate_queued: queued,
            })
        },
    ) else {
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(flat)
}

/// The values [`enqueue_chosen_atoms`] reads from its caller.
struct EnqueueChosenAtomsArgs<'a> {
    chosen: Vec<String>,
    entry: &'a DeferredDisjunction,
    state: &'a mut PassState,
    ctx: &'a ResolveCtx<'a>,
    config: &'a Config,
    bp: &'a BacktrackParams,
    union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Queue one entry's resolved atoms through the owning site's queueing
/// function, so blockers, pullers, root-deps and the `||`-last ordering all
/// behave exactly as for expansion-time choices.
fn enqueue_chosen_atoms(args: EnqueueChosenAtomsArgs<'_>) {
    let EnqueueChosenAtomsArgs {
        chosen,
        entry,
        state,
        ctx,
        config,
        bp,
        union_constraints,
    } = args;
    match &entry.site {
        DeferredSite::Main {
            buildtime_atoms,
            runtime_atoms,
            bdepend_atoms,
            or_universe,
            running_tags,
            root_deps_satisfied,
            root_deps_unsatisfied,
        } => {
            // The `--root-deps` existence split, mirroring `filter_root_deps`:
            // satisfied atoms drop, unsatisfied ones become their own
            // running-root entries. Inert without same-root `--root-deps`.
            let root_deps_split: Option<&Path> = ctx
                .root_deps_running_root
                .filter(|_| ctx.cross_root().is_none());
            let mut flat_deps: Vec<String> = Vec::new();
            for tok in chosen {
                if root_deps_satisfied.contains(&tok) {
                    continue;
                }
                if root_deps_unsatisfied.contains(&tok) {
                    continue;
                }
                flat_deps.push(tok);
            }
            if let Some(running_root) = root_deps_split {
                for atom_str in root_deps_unsatisfied {
                    state.entries.extend(resolve_root_deps_build_entries(
                        ResolveRootDepsBuildEntriesArgs {
                            repos: &ctx.repos,
                            running_root,
                            atom_str,
                            config,
                            owner: entry.owner.clone(),
                            seen: &mut state.root_deps_build_seen,
                            local_binpkg: &ctx.local_binpkg,
                        },
                    ));
                }
            }
            // Puller recording for the chosen atoms, mirroring
            // `record_slot_pullers_and_enqueue`.
            for tok in &flat_deps {
                if let Some(dep_atom) = portage_dep::parse_atom(tok)
                    && dep_atom.blocker == portage_dep::Blocker::None
                {
                    state
                        .slot_pullers
                        .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                        .or_default()
                        .push((
                            entry.owner.0.clone(),
                            entry.owner.1.clone(),
                            entry.owner_version.clone(),
                            tok.clone(),
                        ));
                }
            }
            enqueue_flat_deps(EnqueueFlatDepsArgs {
                flat_deps,
                key: &entry.owner,
                version: &entry.owner_version,
                depth: entry.depth,
                parent_use: &entry.use_flags,
                queue: &mut state.queue,
                pending_blockers: &mut state.pending_blockers,
                buildtime_atoms,
                runtime_atoms,
                bdepend_atoms,
                or_universe,
                running_atoms: running_tags,
                owner_targets_running_root: entry.owner_targets_running_root,
            });
        }
        DeferredSite::Flip {
            buildtime_atoms,
            runtime_atoms,
            bdepend_atoms,
            or_universe,
            running_tags,
            root_deps_satisfied,
            root_deps_unsatisfied,
            delta_meta,
            buildpkgonly_narrow,
            slot,
        } => {
            // The `--root-deps` existence split, mirroring the expansion-time
            // filter: satisfied atoms drop, unsatisfied ones become their own
            // running-root entries. Inert without same-root `--root-deps`.
            let root_deps_split: Option<&Path> = ctx
                .root_deps_running_root
                .filter(|_| ctx.cross_root().is_none());
            let mut flat_deps: Vec<String> = Vec::new();
            for tok in chosen {
                let evaluated = portage_dep::evaluate_atom_conditionals(&tok, &entry.use_flags)
                    .unwrap_or_else(|| tok.clone());
                if root_deps_satisfied.contains(&evaluated) {
                    continue;
                }
                if root_deps_unsatisfied.contains(&evaluated) {
                    continue;
                }
                flat_deps.push(tok);
            }
            if let Some(running_root) = root_deps_split {
                for atom_str in root_deps_unsatisfied {
                    state.entries.extend(resolve_root_deps_build_entries(
                        ResolveRootDepsBuildEntriesArgs {
                            repos: &ctx.repos,
                            running_root,
                            atom_str,
                            config,
                            owner: entry.owner.clone(),
                            seen: &mut state.root_deps_build_seen,
                            local_binpkg: &ctx.local_binpkg,
                        },
                    ));
                }
            }
            // Already-graphed atoms still record pullers and merge-order
            // edges — Portage adds the parent edge even for a previously
            // added package — but only genuinely new atoms queue: the drain
            // may have graphed the choice after the flip (that is the whole
            // point of deferring), and re-queueing a visited atom would
            // re-resolve it.
            let queued_atoms: HashSet<&str> = state.queue.iter().map(|q| q.atom.as_str()).collect();
            let mut seen_toks: Vec<String> = Vec::new();
            let mut new_toks: Vec<String> = Vec::new();
            for tok in &flat_deps {
                let evaluated = portage_dep::evaluate_atom_conditionals(tok, &entry.use_flags)
                    .unwrap_or_else(|| tok.clone());
                if state.visited_atoms.contains(&evaluated)
                    || queued_atoms.contains(evaluated.as_str())
                {
                    seen_toks.push(tok.clone());
                } else {
                    new_toks.push(tok.clone());
                }
            }
            // A text-new atom that cleanly re-resolves to its settled slot is
            // dropped before any bookkeeping, like at expansion time.
            let mut kept_toks: Vec<String> = Vec::new();
            for tok in &new_toks {
                let evaluated = portage_dep::evaluate_atom_conditionals(tok, &entry.use_flags)
                    .unwrap_or_else(|| tok.clone());
                if delta_atom_cleanly_reuses_resolved_slot(DeltaAtomCleanlyReusesResolvedSlotArgs {
                    ctx,
                    config,
                    union_constraints,
                    state: &*state,
                    bp,
                    evaluated: &evaluated,
                    atom_running: running_tags.contains(tok),
                }) {
                    continue;
                }
                kept_toks.push(tok.clone());
            }
            if seen_toks.is_empty() && kept_toks.is_empty() {
                return;
            }
            for tok in seen_toks.iter().chain(kept_toks.iter()) {
                if let Some(dep_atom) = portage_dep::parse_atom(tok)
                    && dep_atom.blocker == portage_dep::Blocker::None
                {
                    state
                        .slot_pullers
                        .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                        .or_default()
                        .push((
                            entry.owner.0.clone(),
                            entry.owner.1.clone(),
                            entry.owner_version.clone(),
                            tok.clone(),
                        ));
                    // The parent edge, like a popped queue item records it:
                    // Portage adds the parent atom even for a previously
                    // added package.
                    state
                        .required_by_map
                        .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                        .or_default()
                        .insert((entry.owner.0.clone(), entry.owner.1.clone()));
                }
            }
            enqueue_flat_deps(EnqueueFlatDepsArgs {
                flat_deps: kept_toks.clone(),
                key: &entry.owner,
                version: &entry.owner_version,
                depth: entry.depth,
                parent_use: &entry.use_flags,
                queue: &mut state.queue,
                pending_blockers: &mut state.pending_blockers,
                buildtime_atoms,
                runtime_atoms,
                bdepend_atoms,
                or_universe,
                running_atoms: running_tags,
                owner_targets_running_root: entry.owner_targets_running_root,
            });
            // Merge-order edges cover every edge-worthy target, visited or
            // newly queued: Portage records the parent edge even for a
            // previously added package, and the merge list order follows it.
            let new_targets: HashSet<(String, String)> = seen_toks
                .iter()
                .chain(kept_toks.iter())
                .filter_map(|tok| {
                    portage_dep::parse_atom(tok)
                        .map(|dep_atom| (dep_atom.category.clone(), dep_atom.package.clone()))
                })
                .collect();
            if !new_targets.is_empty() {
                let portage_order_keys: &[&str] = if *buildpkgonly_narrow {
                    &["DEPEND", "BDEPEND"]
                } else {
                    &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
                };
                let fresh_edges = merge_order::dep_edges_from_metadata(
                    delta_meta,
                    &entry.use_flags,
                    portage_order_keys,
                    false,
                );
                let slot_key = (
                    entry.owner.0.clone(),
                    entry.owner.1.clone(),
                    slot.clone(),
                    entry.owner_targets_running_root,
                );
                if let Some(&idx) = state.resolved_slots.get(&slot_key) {
                    let still_current = match &state.entries[idx].outcome {
                        PretendOutcome::New { version } => version == &entry.owner_version,
                        PretendOutcome::Upgrade { to, .. } => to == &entry.owner_version,
                        PretendOutcome::Downgrade { to, .. } => to == &entry.owner_version,
                        PretendOutcome::Reinstall { version, .. } => {
                            version == &entry.owner_version
                        }
                        _ => false,
                    };
                    if still_current {
                        let entry_deps = &mut state.entries[idx].deps;
                        for edge in fresh_edges {
                            if new_targets.contains(&(edge.category.clone(), edge.package.clone()))
                                && !entry_deps.contains(&edge)
                            {
                                entry_deps.push(edge);
                            }
                        }
                    }
                }
            }
        }
        DeferredSite::Installed {
            buildtime_tokens,
            or_universe,
            running_tags,
            root_deps_satisfied,
            root_deps_unsatisfied,
            root_deps_split,
            resolve_root,
            root_deps_running_root,
            ..
        } => {
            // Same plains-reversed + `||`-last split as the expansion-time
            // `enqueue_flat_deps`, kept here too.
            let universe = or_universe;
            let (mut disj, mut plains): (Vec<String>, Vec<String>) = chosen
                .into_iter()
                .partition(|t| t != "||" && universe.contains(t));
            plains.reverse();
            disj.reverse();
            let root_deps_split: Option<&Path> = if *root_deps_split {
                root_deps_running_root.as_deref()
            } else {
                None
            };
            enqueue_plain_tokens(EnqueuePlainTokensArgs {
                plains,
                disj,
                use_flags: &entry.use_flags,
                buildtime_tokens,
                pending_blockers: &mut state.pending_blockers,
                owner_key: entry.owner.clone(),
                owner_version: &entry.owner_version,
                root_deps_split,
                root_deps_satisfied,
                root_deps_unsatisfied,
                root: resolve_root,
                config,
                slot_pullers: &mut state.slot_pullers,
                queue: &mut state.queue,
                child_depth: entry.depth,
                entries: &mut state.entries,
                running_tags,
            });
        }
    }
}
