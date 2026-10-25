//! Queueing one package's dependencies.

use crate::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

/// The installed or newly selected package whose dependencies
/// [`enqueue_dependencies`] queues.
#[derive(Clone, Copy)]
pub(crate) struct DepOwner<'a> {
    /// The walk's target root, so an installed parent's own deps resolve per
    /// group like every other owner. `root` stays the vdb root for metadata.
    pub target_root: &'a Path,
    pub category: &'a str,
    pub package: &'a str,
    pub version: &'a str,
    pub child_depth: u32,
}

/// The emerge options [`enqueue_dependencies`] consults.
#[derive(Clone, Copy)]
pub(crate) struct EnqueueOptions<'a> {
    pub dynamic_deps: bool,
    pub ignore_built_slot_operator_deps: bool,
    pub with_bdeps: bool,
    pub root_deps_running_root: Option<&'a Path>,
    /// No `circular_dependency` map: this deep-walk's parents are
    /// `AlreadyInstalled` entries only, which Portage's node-identity-keyed
    /// lookup always misses, so the empty map is exactly faithful here.
    /// `update` (`--update`): the downgrade-guard's queued-update lookahead.
    pub update: bool,
}

/// The pass state [`enqueue_dependencies`] appends to.
pub(crate) struct EnqueueSinks<'a> {
    pub queue: &'a mut VecDeque<QueueItem>,
    pub pending_blockers: &'a mut Vec<PendingBlocker>,
    pub entries: &'a mut Vec<GraphEntry>,
    pub root_deps_build_seen: &'a mut HashSet<(String, String)>,
    /// Per-pass `installed_dep_string` memo: keyed
    /// `(category, package, version, key)`, Effective results only.
    pub installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
    /// This installed package's own flattened non-blocker atoms become
    /// slot-conflict pullers, like the main loop's own recording.
    pub slot_pullers: &'a mut SlotPullers,
    /// Deferred `||` groups, pushed when this expansion owns any; `run_pass`
    /// pops them once the whole queue drains.
    pub disjunctive_stack: &'a mut Vec<DeferredDisjunction>,
}

/// Reads `category/package-version`'s own dependency metadata and enqueues
/// each flattened token: blockers into `pending_blockers`, the rest into
/// `queue`. Shared by the main loop and `--deep`'s AlreadyInstalled walk.
/// Silently does nothing when the version or its metadata is unreadable.
///
/// `dynamic_deps` (`--dynamic-deps`, on by default): when `true`, walk the
/// repo's current ebuild metadata plus the vdb's built `:=` atoms, flattened
/// against the vdb `USE`; when `false`, the vdb-recorded snapshot alone.
///
/// `with_bdeps` (`--with-bdeps`): when `false`, DEPEND/BDEPEND are left out
/// entirely -- the one place that distinction is ever made.
///
/// `root_deps_running_root` (`--root-deps`): `None` is a strict no-op; when
/// `Some`, already-flattened atoms satisfied from the running root are
/// dropped from the queue.
pub(crate) fn enqueue_dependencies(
    source: &SelectSource,
    owner: &DepOwner,
    opts: &EnqueueOptions,
    sinks: EnqueueSinks,
    // The backtrack loop's accumulated per-`cat/pkg` masks, consulted by the
    // `||` branch selection; empty on the first pass.
    disj_constraints: &HashMap<(String, String), Vec<String>>,
) {
    let SelectSource {
        root,
        config,
        local_binpkg,
        gate,
    } = *source;
    let DepOwner {
        target_root,
        category,
        package,
        version,
        child_depth,
    } = *owner;
    let owner_key = (category.to_string(), package.to_string());
    let owner_version = version.to_string();
    let EnqueueOptions {
        dynamic_deps,
        ignore_built_slot_operator_deps,
        with_bdeps,
        root_deps_running_root,
        update,
    } = *opts;
    let EnqueueSinks {
        queue,
        pending_blockers,
        entries,
        root_deps_build_seen,
        installed_meta_memo,
        slot_pullers,
        disjunctive_stack,
    } = sinks;
    // The installed package's own dependency metadata: the live ebuild view
    // while the version still has an ebuild, else the vdb-recorded snapshot
    // for an installed-only package (never a priority search, never absent).
    let metadata = installed_walk_metadata(gate.repos, root, category, package, version);

    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };

    // The USE conditionals are evaluated against the installed recorded USE,
    // never a fresh profile recompute.
    let use_flags = read_vdb_flag_set(root, category, package, version, "USE");
    // Per-key strings for the group-root tags below.
    let mut keyed_depstr: HashMap<String, String> = HashMap::new();
    let depstr = {
        let mut depstr = String::new();
        for dep_key in dep_keys {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            let s = installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category,
                package,
                version,
                live: Some(&metadata),
                key: dep_key,
                layer,
                memo: installed_meta_memo,
            });
            depstr.push_str(&s);
            depstr.push(' ');
            keyed_depstr.insert(dep_key.to_string(), s);
        }
        depstr
    };
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    // This installed owner's own per-group roots. Inactive on same-root runs.
    let cross_active =
        root_deps_running_root.is_some_and(|rr| rr.as_os_str() != target_root.as_os_str());
    let owner_is_running = root.as_os_str() != target_root.as_os_str();
    let running_tags: HashSet<String> = if cross_active {
        dep_running_tags(&keyed_depstr, &use_flags, owner_is_running)
    } else {
        Default::default()
    };
    let buildtime_tokens = collect_buildtime_tokens(CollectBuildtimeTokensArgs {
        with_bdeps,
        dynamic_deps,
        root,
        ignore_built_slot_operator_deps,
        category,
        package,
        version,
        metadata: &metadata,
        installed_meta_memo,
        use_flags: &use_flags,
    });
    // The node key for the resolver map (installed deep-walk owner).
    let self_node: RootCp = (false, category.to_string(), package.to_string());
    // Branch selection shares the `disjunction_preference` helper; same
    // drain-state snapshot as the main walk. `||` groups defer to the
    // drained graph instead: only the plains flatten here, with the same
    // closures (never invoked on `||`-free input).
    // Portage: depgraph.py, _queue_disjunctive_deps
    let (plains, groups) = split_off_disjunctions(&tokens, &use_flags);
    let flat_deps = match flatten_deps_for_enqueue(FlattenDepsForEnqueueArgs {
        queue,
        tokens: &plains,
        use_flags: &use_flags,
        cross_active,
        running_tags: &running_tags,
        root_deps_running_root,
        root,
        entries,
        config,
        disj_constraints,
        self_node: &self_node,
        update,
        gate,
    }) {
        ControlFlow::Continue(out) => out,
        ControlFlow::Break(()) => return,
    };

    let root_deps_satisfied: HashSet<String> = if with_bdeps {
        root_deps_running_root
            .map(|root| {
                root_deps_satisfied_atoms(RootDepsSatisfiedAtomsArgs {
                    metadata: &metadata,
                    use_flags: &use_flags,
                    config,
                    running_root: root,
                    dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"], // Installed-walk defaults.
                    empty_groups_always_true: false,
                    gate,
                })
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };

    // Unsatisfied running-root build deps become their own entries (kept as a
    // `Vec` for deterministic order); flag-on coinciding-roots only.
    let root_deps_split = root_deps_running_root.filter(|_| !cross_active);
    let root_deps_unsatisfied: Vec<String> = if with_bdeps {
        root_deps_running_root
            .map(|root| {
                unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
                    metadata: &metadata,
                    use_flags: &use_flags,
                    config,
                    running_root: root,
                    dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                    empty_groups_always_true: false,
                    gate,
                })
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(running_root) = root_deps_split {
        for atom_str in &root_deps_unsatisfied {
            entries.extend(resolve_root_deps_build_entries(
                ResolveRootDepsBuildEntriesArgs {
                    running_root,
                    atom_str,
                    config,
                    owner: owner_key.clone(),
                    seen: root_deps_build_seen,
                    local_binpkg,
                    gate,
                },
            ));
        }
    }

    // Same plains-reversed + `||`-last split as the main walk's
    // `enqueue_flat_deps`.
    let universe = or_group_universe(&tokens);
    if !groups.is_empty() {
        disjunctive_stack.push(DeferredDisjunction {
            groups,
            use_flags: use_flags.clone(),
            empty_groups_always_true: false,
            owner: owner_key.clone(),
            owner_version: owner_version.clone(),
            depth: child_depth,
            owner_targets_running_root: entries
                .iter()
                .find(|e| e.category == owner_key.0 && e.package == owner_key.1)
                .is_some_and(|e| e.targets_running_root),
            site: DeferredSite::Installed {
                buildtime_tokens: buildtime_tokens.clone(),
                or_universe: universe.clone(),
                running_tags: running_tags.clone(),
                root_deps_satisfied: root_deps_satisfied.clone(),
                root_deps_unsatisfied: root_deps_unsatisfied.clone(),
                root_deps_split: root_deps_split.is_some(),
                resolve_root: root.to_path_buf(),
                cross_active,
                root_deps_running_root: root_deps_running_root.map(|p| p.to_path_buf()),
            },
        });
    }
    let (mut disj, mut plains): (Vec<String>, Vec<String>) = flat_deps
        .into_iter()
        .partition(|t| t != "||" && universe.contains(t));
    plains.reverse();
    disj.reverse();
    enqueue_plain_tokens(EnqueuePlainTokensArgs {
        plains,
        disj,
        use_flags: &use_flags,
        buildtime_tokens: &buildtime_tokens,
        pending_blockers,
        owner_key,
        owner_version: &owner_version,
        root_deps_split,
        root_deps_satisfied: &root_deps_satisfied,
        root_deps_unsatisfied: &root_deps_unsatisfied,
        root,
        config,
        slot_pullers,
        queue,
        child_depth,
        entries,
        running_tags: &running_tags,
    });
}

/// The values [`collect_buildtime_tokens`] reads from its caller.
pub(crate) struct CollectBuildtimeTokensArgs<'a> {
    pub(crate) with_bdeps: bool,
    pub(crate) dynamic_deps: bool,
    pub(crate) root: &'a Path,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
    pub(crate) use_flags: &'a HashSet<String>,
}

/// Flatten the build-time dependency keys into the set of blocker-token atoms; empty without with_bdeps.
pub(crate) fn collect_buildtime_tokens(args: CollectBuildtimeTokensArgs<'_>) -> HashSet<String> {
    let CollectBuildtimeTokensArgs {
        with_bdeps,
        dynamic_deps,
        root,
        ignore_built_slot_operator_deps,
        category,
        package,
        version,
        metadata,
        installed_meta_memo,
        use_flags,
    } = args;
    // The blocker tokens' build-time provenance, flattened from just the
    // build-time keys. Empty without `with_bdeps`.
    let buildtime_tokens: HashSet<String> = if with_bdeps {
        let mut joined = String::new();
        for dep_key in ["DEPEND", "BDEPEND"] {
            let layer = if dynamic_deps {
                InstalledMetaLayer::Effective
            } else {
                InstalledMetaLayer::Raw
            };
            joined.push_str(&installed_dep_string(InstalledDepStringArgs {
                root,
                dynamic_deps,
                ignore_built_slot_operator_deps,
                category,
                package,
                version,
                live: Some(metadata),
                key: dep_key,
                layer,
                memo: installed_meta_memo,
            }));
            joined.push(' ');
        }
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(&toks, use_flags, portage_use_reduce::MatchMode::Normal)
            .map(|v| v.into_iter().filter(|t| t != "||").collect())
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    buildtime_tokens
}

/// The values [`enqueue_plain_tokens`] reads from its caller.
pub(crate) struct EnqueuePlainTokensArgs<'a> {
    pub(crate) plains: Vec<String>,
    pub(crate) disj: Vec<String>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) buildtime_tokens: &'a HashSet<String>,
    pub(crate) pending_blockers: &'a mut Vec<PendingBlocker>,
    pub(crate) owner_key: (String, String),
    pub(crate) owner_version: &'a str,
    pub(crate) root_deps_split: Option<&'a Path>,
    pub(crate) root_deps_satisfied: &'a HashSet<String>,
    pub(crate) root_deps_unsatisfied: &'a [String],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) slot_pullers: &'a mut SlotPullers,
    pub(crate) queue: &'a mut VecDeque<QueueItem>,
    pub(crate) child_depth: u32,
    pub(crate) entries: &'a mut Vec<GraphEntry>,
    pub(crate) running_tags: &'a HashSet<String>,
}

/// Queue the plain dependency tokens, recording blockers and slot pullers.
pub(crate) fn enqueue_plain_tokens(args: EnqueuePlainTokensArgs<'_>) {
    let EnqueuePlainTokensArgs {
        plains,
        disj,
        use_flags,
        buildtime_tokens,
        pending_blockers,
        owner_key,
        owner_version,
        root_deps_split,
        root_deps_satisfied,
        root_deps_unsatisfied,
        root,
        config,
        slot_pullers,
        queue,
        child_depth,
        entries,
        running_tags,
    } = args;
    for (tok, from_disjunction) in plains
        .into_iter()
        .map(|t| (t, false))
        .chain(disj.into_iter().map(|t| (t, true)))
    {
        if tok == "||" {
            continue;
        }
        // Every token's conditional use-deps are evaluated against the
        // parent's USE before classification or queueing; `raw` is kept for
        // the `unevaluated` queue field only.
        let raw = tok;
        let evaluated =
            portage_dep::evaluate_atom_conditionals(&raw, use_flags).unwrap_or_else(|| raw.clone());
        let unevaluated = if evaluated != raw {
            Some(raw.clone())
        } else {
            None
        };
        let tok = evaluated;
        if let Some(dep_atom) = parse_dep_atom(&tok, "enqueue")
            && dep_atom.blocker != portage_dep::Blocker::None
        {
            // Provenance against the build-time-keys flatten above.
            let buildtime = buildtime_tokens.contains(&tok);
            pending_blockers.push(PendingBlocker {
                atom_str: tok,
                strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                buildtime,
                target_category: dep_atom.category,
                target_package: dep_atom.package,
                owner_key: owner_key.clone(),
                owner_version: owner_version.to_string(),
                owner_merging: false,
                owner_installed: false,
            });
            continue;
        }
        if root_deps_split.is_some() && root_deps_satisfied.contains(&tok) {
            // Already satisfied from the running root: no graph node needed.
            continue;
        }
        if root_deps_split.is_some() && root_deps_unsatisfied.contains(&tok) {
            // Already handled above (a new running-root entry, or dropped).
            continue;
        }
        // A dependency the installed parent itself satisfies is its graph node
        // already -- no New/Upgrade/Reinstall decision is ever made for it. A
        // self atom on a *different* slot still queues normally.
        if let Some(dep_atom) = parse_dep_atom(&tok, "enqueue")
            && (dep_atom.category, dep_atom.package) == owner_key
            && best_installed_matching(root, &tok, config)
                .is_some_and(|version| version == owner_version)
        {
            continue;
        }
        // Record the installed parent as a puller before queueing, so its
        // built `:S/SS=` atoms reach the `need_rebuild` scan.
        if let Some(dep_atom) = parse_dep_atom(&tok, "enqueue") {
            slot_pullers
                .entry((dep_atom.category.clone(), dep_atom.package.clone()))
                .or_default()
                .push((
                    owner_key.0.clone(),
                    owner_key.1.clone(),
                    owner_version.to_string(),
                    tok.clone(),
                ));
        }
        queue.push_back(QueueItem {
            atom: tok,
            depth: child_depth,
            owner: Some(owner_key.clone()),
            owner_version: Some(owner_version.to_string()),
            // The installed owner's own root flag.
            owner_targets_running_root: entries
                .iter()
                .find(|e| e.category == owner_key.0 && e.package == owner_key.1)
                .is_some_and(|e| e.targets_running_root),
            unevaluated,
            // The per-group root travels with the atom, keyed on the raw token.
            targets_running_root: running_tags.contains(&raw),
            buildtime_hard: false,
            from_disjunction,
        });
    }
}

/// The values [`flatten_deps_for_enqueue`] reads from its caller.
pub(crate) struct FlattenDepsForEnqueueArgs<'a> {
    pub(crate) queue: &'a mut VecDeque<QueueItem>,
    pub(crate) tokens: &'a [String],
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) cross_active: bool,
    pub(crate) running_tags: &'a HashSet<String>,
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a mut Vec<GraphEntry>,
    pub(crate) config: &'a Config,
    pub(crate) disj_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) self_node: &'a (bool, String, String),
    pub(crate) update: bool,
    pub(crate) gate: CategoryGate<'a>,
}

/// Flatten the installed dependency strings, choosing one branch of each disjunction as the walk does.
pub(crate) fn flatten_deps_for_enqueue(
    args: FlattenDepsForEnqueueArgs<'_>,
) -> ControlFlow<(), Vec<String>> {
    let FlattenDepsForEnqueueArgs {
        queue,
        tokens,
        use_flags,
        cross_active,
        running_tags,
        root_deps_running_root,
        root,
        entries,
        config,
        disj_constraints,
        self_node,
        update,
        gate,
    } = args;
    let queued: Vec<QueueItem> = queue.iter().cloned().collect();
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal, // Installed recursion defaults.
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
                    root
                };
                let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                    entries
                        .iter()
                        .filter(|e| e.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                    queued
                        .iter()
                        .filter(|q| q.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
                promote_tied_alternative(PromoteTiedAlternativeArgs {
                    config,
                    root: grp_root,
                    entries: entries_view,
                    constraints: disj_constraints,
                    alts,
                    queued: queued_view,
                    gate,
                })
            },
        },
        &mut |atoms: &[String]| {
            // Same root scoping as the main walk's closures.
            let grp_running = group_running(atoms, cross_active, running_tags);
            let grp_root: &Path = if grp_running {
                root_deps_running_root.expect("running-root group without a running root")
            } else {
                root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_active.then(|| {
                entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_active.then(|| {
                queued
                    .iter()
                    .filter(|q| q.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let queued_view: &[QueueItem] = scoped_queued.as_deref().unwrap_or(&queued);
            disjunction_preference(DisjunctionPreferenceArgs {
                config,
                root: grp_root,
                entries: entries_view,
                self_cp: self_node,
                constraints: disj_constraints, // The empty map (see the parameter doc).
                circular_dependency: &HashMap::new(),
                root_deps_running_root,
                atoms,
                queued: queued_view,
                update,
                gate,
                gate_entries: entries,
                gate_queued: &queued,
            })
        },
    ) else {
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(flat_deps)
}

/// Whether every atom of the group belongs to the running root.
fn group_running(atoms: &[String], cross_active: bool, running_tags: &HashSet<String>) -> bool {
    cross_active && atoms.iter().all(|a| running_tags.contains(a))
}
