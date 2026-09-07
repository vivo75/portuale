//! Fold the entry's dependency keys and queue the atoms they name.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

/// The values [`enqueue_entry_deps`] reads from its caller.
pub(crate) struct EnqueueEntryDepsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) depth: u32,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) config: &'a Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) candidate_str: &'a str,
}

/// What [`select_dep_tokens`] hands back to its caller.
pub(crate) struct SelectDepTokensOut {
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
    pub(crate) eapi_has_idepend: bool,
    pub(crate) tokens: Vec<String>,
}

/// Pick the dep keys this entry walks and flatten them to tokens.
///
/// A built package under `--usepkg`/`--getbinpkg` walks without
/// `DEPEND`/`BDEPEND`; `--buildpkgonly` without `--deep` blanks the
/// runtime keys of a non-built package instead.
pub(crate) fn select_dep_tokens(
    ctx: &ResolveCtx<'_>,
    depth: u32,
    metadata: &Arc<HashMap<String, String>>,
    candidate_source: CandidateSource,
) -> SelectDepTokensOut {
    // A built package (binary) under `--usepkg`/`--getbinpkg` walks without
    // `DEPEND`/`BDEPEND`; `--buildpkgonly` without `--deep` blanks the
    // runtime keys of a non-built package instead. Binaries keep theirs.
    // Portage: depgraph.py, _add_pkg_dep_string; `with_bdeps` carries the bit
    //
    // An `--onlydeps` root blanks `RDEPEND`/`PDEPEND` (and `IDEPEND` unless
    // kept) when the matching off-flags are given; build-time keys always
    // stay. Inert without `--onlydeps`. Portage: depgraph.py, _add_pkg_deps
    let onlydeps_blanks_runtime = ctx.onlydeps && depth == 0 && !ctx.onlydeps_with_rdeps;
    let onlydeps_blanks_idepend = onlydeps_blanks_runtime && !ctx.onlydeps_with_ideps;
    // Below EAPI 8 `IDEPEND` never reaches metadata, for ebuild and binary
    // alike (missing `EAPI` defaults to `"0"`).
    // Portage: bin/ebuild.sh (depend phase), eapi.py
    let eapi_has_idepend =
        md5_dict::eapi_has_idepend(metadata.get("EAPI").map(String::as_str).unwrap_or("0"));
    let keep_key = |key: &&str| {
        onlydeps_walk_dep_key(OnlydepsWalkDepKeyArgs {
            key,
            eapi_has_idepend,
            onlydeps_blanks_runtime,
            onlydeps_blanks_idepend,
        })
    };
    let base_keys: &[&str] = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    } else if ctx.buildpkgonly
        && candidate_source != CandidateSource::Binary
        && matches!(ctx.deep, Deep::NotRequested)
    {
        &["DEPEND", "BDEPEND"]
    } else {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    };
    // No per-package alloc on the hot path when nothing is blanked.
    let filtered_keys: Vec<&str>;
    let dep_keys: &[&str] = if onlydeps_blanks_runtime || !eapi_has_idepend {
        filtered_keys = base_keys.iter().copied().filter(keep_key).collect();
        &filtered_keys
    } else {
        base_keys
    };
    let mut depstr = String::new();
    for dep_key in dep_keys {
        if let Some(d) = metadata.get(*dep_key) {
            depstr.push_str(d);
            depstr.push(' ');
        }
    }
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    SelectDepTokensOut {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
        tokens,
    }
}

/// The values [`record_merge_order_deps`] reads from its caller.
pub(crate) struct RecordMergeOrderDepsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
    pub(crate) eapi_has_idepend: bool,
}

/// Record the entry's merge-order dep edges, cut to the running-root scope.
///
/// Discovery order is per-key, flattened separately so cross-key order can't
/// leak in. Blanked keys stay out of the digraph too.
pub(crate) fn record_merge_order_deps(args: RecordMergeOrderDepsArgs<'_>) {
    let RecordMergeOrderDepsArgs {
        ctx,
        candidate_source,
        metadata,
        use_flags,
        state,
        entry_idx,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    } = args;
    let keep_key = |key: &&str| {
        onlydeps_walk_dep_key(OnlydepsWalkDepKeyArgs {
            key,
            eapi_has_idepend,
            onlydeps_blanks_runtime,
            onlydeps_blanks_idepend,
        })
    };
    // Discovery order is per-key (`RDEPEND`, `IDEPEND`, `PDEPEND`, `DEPEND`,
    // `BDEPEND`), flattened separately so cross-key order can't leak in.
    // Portage: depgraph.py, _add_pkg_dep_string (deps tuple)
    let base_order_keys: &[&str] = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps
    {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    } else if ctx.buildpkgonly
        && candidate_source != CandidateSource::Binary
        && matches!(ctx.deep, Deep::NotRequested)
    {
        // Same runtime-key blanking as `dep_keys` above.
        &["DEPEND", "BDEPEND"]
    } else if state.entries[entry_idx].targets_running_root {
        // A running-root build entry keeps `BDEPEND`-before-`RDEPEND` insertion
        // order; single-key running entries read the same either way.
        &["DEPEND", "BDEPEND", "RDEPEND", "IDEPEND", "PDEPEND"]
    } else {
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
    };
    // Blanked keys stay out of the merge-order digraph too.
    let filtered_order_keys: Vec<&str>;
    let portage_order_keys: &[&str] = if onlydeps_blanks_runtime || !eapi_has_idepend {
        filtered_order_keys = base_order_keys.iter().copied().filter(keep_key).collect();
        &filtered_order_keys
    } else {
        base_order_keys
    };
    state.entries[entry_idx].deps = merge_order::dep_edges_from_metadata(
        metadata,
        use_flags,
        portage_order_keys,
        candidate_source == CandidateSource::Binary,
    );
    // The running-root subgraph's own cycle scope, for the merge-order digraph
    // too: a running `BDEPEND` edge back into a visited package is dropped,
    // like the old sidecar `seen` guard. Running `DEPEND` edges back are kept
    // so the cross-root cycle still reports, as is every first-visit edge.
    // Key 4 is `BDEPEND`; visited compares by `cat/pkg`, like `required_by_map`.
    if state.entries[entry_idx].targets_running_root {
        let mut visited_cps: HashSet<(String, String)> = HashSet::new();
        for v in state.visited_atoms.iter() {
            if let Some(a) = portage_dep::parse_atom(v) {
                visited_cps.insert((a.category, a.package));
            }
        }
        // Portage drops a direct self-edge unless it is an unsatisfied build-time
        // dependency; without the exemption the cut deletes the true self-loop
        // with the bootstrap-ring closers. Satisfied self-edges still drop
        // downstream. Portage: depgraph.py, _add_pkg; _serialize_tasks
        let own_category = state.entries[entry_idx].category.clone();
        let own_package = state.entries[entry_idx].package.clone();
        state.entries[entry_idx].deps.retain(|d| {
            !(d.key == 4
                && !(d.category == own_category && d.package == own_package)
                && visited_cps.contains(&(d.category.clone(), d.package.clone())))
        });
    }
}

/// What [`classify_dep_atoms`] hands back to its caller.
pub(crate) struct ClassifyDepAtomsOut {
    pub(crate) buildtime_atoms: HashSet<String>,
    pub(crate) runtime_atoms: HashSet<String>,
    pub(crate) bdepend_atoms: HashSet<String>,
}

/// The values [`classify_dep_atoms`] reads from its caller.
pub(crate) struct ClassifyDepAtomsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) candidate_source: CandidateSource,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) onlydeps_blanks_runtime: bool,
    pub(crate) onlydeps_blanks_idepend: bool,
    pub(crate) eapi_has_idepend: bool,
}

/// Per-edge build-time/run-time classification for the merge-order sort
/// and cycle detection: an atom in the build-time keys but not the run-time
/// ones tags `buildtime_hard`. Portage: DepPriority
pub(crate) fn classify_dep_atoms(args: ClassifyDepAtomsArgs<'_>) -> ClassifyDepAtomsOut {
    let ClassifyDepAtomsArgs {
        ctx,
        candidate_source,
        metadata,
        use_flags,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    } = args;
    let flatten_keys = |keys: &[&str]| -> HashSet<String> {
        let joined: String = keys
            .iter()
            .filter_map(|k| metadata.get(*k))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let toks: Vec<String> = joined.split_whitespace().map(String::from).collect();
        portage_use_reduce::use_reduce_flat(&toks, use_flags, portage_use_reduce::MatchMode::Normal)
            .map(|v| v.into_iter().filter(|t| t != "||").collect())
            .unwrap_or_default()
    };
    let buildtime_atoms = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        // Dropped from the walk above, so nothing can classify from them.
        HashSet::new()
    } else {
        flatten_keys(&["DEPEND", "BDEPEND"])
    };
    // A blanked runtime key classifies nothing, so a build-time-only atom is
    // `buildtime_hard`; a walked `IDEPEND` still classifies.
    let runtime_keys: &[&str] = onlydeps_runtime_keys(OnlydepsRuntimeKeysArgs {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    });
    let runtime_atoms = flatten_keys(runtime_keys);
    // A running `BDEPEND` edge is soft (bootstrap cycles terminate); a running
    // `DEPEND` edge stays hard (the cross-root cycle still reports).
    let bdepend_atoms = if candidate_source == CandidateSource::Binary && !ctx.with_bdeps {
        HashSet::new()
    } else {
        flatten_keys(&["BDEPEND"])
    };
    ClassifyDepAtomsOut {
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
    }
}

/// What [`prepare_disjunction_scope`] hands back to its caller.
pub(crate) struct DisjunctionScopeOut<'a> {
    pub(crate) cross_running_root: Option<&'a Path>,
    pub(crate) running_tags: HashSet<String>,
    pub(crate) self_node: RootCp,
    pub(crate) queued: Vec<QueueItem>,
    pub(crate) circ_with_pullers: Option<HashMap<RootCp, Vec<CircularDepChild>>>,
}

/// The values [`prepare_disjunction_scope`] reads from its caller.
pub(crate) struct PrepareDisjunctionScopeArgs<'a, 's> {
    pub(crate) ctx: &'s ResolveCtx<'a>,
    pub(crate) metadata: &'s Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'s HashSet<String>,
    pub(crate) state: &'s mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) bp: &'s BacktrackParams,
}

/// The `||`-resolution scope for this expansion: per-group roots, the queued
/// snapshot (queued-but-unresolved deps count as will-be-in-graph) and the
/// cycle view with the pullers' children joined in.
pub(crate) fn prepare_disjunction_scope<'a>(
    args: PrepareDisjunctionScopeArgs<'a, '_>,
) -> DisjunctionScopeOut<'a> {
    let PrepareDisjunctionScopeArgs {
        ctx,
        metadata,
        use_flags,
        state,
        entry_idx,
        bp,
    } = args;
    // This expansion's per-group roots (Portage `_add_pkg_deps`' `depend_root`
    // plus the running-root groups). `None` on same-root runs: a no-op.
    let cross_running_root: Option<&Path> = ctx.cross_root();
    let running_tags: HashSet<String> = cross_running_root
        .map(|_| {
            dep_running_tags(
                metadata,
                use_flags,
                state.entries[entry_idx].targets_running_root,
            )
        })
        .unwrap_or_default();
    // No self-exclusion on the first pass: an in-graph self atom ranks
    // `preferred_in_graph` and the cycle map re-resolves it; only a self atom
    // matching nothing refuses. Portage: dep_zapdeps.
    let self_cp = (
        state.entries[entry_idx].category.clone(),
        state.entries[entry_idx].package.clone(),
    );
    // The node key for the resolver map: the entry's own root.
    let self_node: RootCp = (
        state.entries[entry_idx].targets_running_root,
        state.entries[entry_idx].category.clone(),
        state.entries[entry_idx].package.clone(),
    );
    // Choice bins live in `disjunction_preference` (single shared copy).
    // The queued snapshot lets `atoms_all_in_graph` count queued-but-unresolved
    // deps as will-be-in-graph (Portage resolves `||` after the stack drains).
    let queued: Vec<QueueItem> = state.queue.iter().cloned().collect();
    // A new-style virtual expands inline in its puller's dep_check; portuale walks
    // it as its own node, so the pullers' recorded cycle children join the
    // lookup (union over this pass's pullers). Portage: dep_check.py, dep_zapdeps.
    let circ_with_pullers: Option<HashMap<RootCp, Vec<CircularDepChild>>> =
        if self_node.1 == "virtual" && !bp.circular_dependency.is_empty() {
            let mut extra: Vec<CircularDepChild> = Vec::new();
            if let Some(pullers) = state.required_by_map.get(&self_cp) {
                let mut pullers: Vec<&(String, String)> = pullers.iter().collect();
                pullers.sort();
                for p in pullers {
                    // Pullers are cp-keyed (root-blind); the cycle map is
                    // node-keyed, so consult both roots.
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
    DisjunctionScopeOut {
        cross_running_root,
        running_tags,
        self_node,
        queued,
        circ_with_pullers,
    }
}

/// The values [`flatten_dep_string`] reads from its caller.
pub(crate) struct FlattenDepStringArgs<'a> {
    pub(crate) tokens: &'a [String],
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) cross_running_root: Option<&'a Path>,
    pub(crate) running_tags: &'a HashSet<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) self_node: &'a RootCp,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) circular_view: &'a HashMap<RootCp, Vec<CircularDepChild>>,
    pub(crate) queued: &'a [QueueItem],
}

/// Flatten the dep string with `||` branch selection.
///
/// Branch selection sees the group's own root; a `||` group with no
/// tree-visible branch still selects one here. `Break` when the string does
/// not flatten.
pub(crate) fn flatten_dep_string(args: FlattenDepStringArgs<'_>) -> ControlFlow<(), Vec<String>> {
    let FlattenDepStringArgs {
        tokens,
        use_flags,
        metadata,
        cross_running_root,
        running_tags,
        ctx,
        config,
        entries,
        self_node,
        union_constraints,
        circular_view,
        queued,
    } = args;
    let Ok(flat_deps) = portage_use_reduce::use_reduce_flat_disjunctive(
        portage_use_reduce::UseReduceFlatDisjunctiveArgs {
            tokens,
            uselist: use_flags,
            mode: portage_use_reduce::MatchMode::Normal,
            empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
            ),
            tie_break: &mut |alts: &[Vec<String>]| {
                // Same group-root scoping for the tied-alternative promotion.
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
                    entries
                        .iter()
                        .filter(|e| e.targets_running_root == grp_running)
                        .cloned()
                        .collect()
                });
                let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
                let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
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
                    constraints: union_constraints,
                    alts,
                    queued: queued_view,
                })
            },
        },
        &mut |atoms: &[String]| {
            // Branch selection sees the group's own root (installed db and
            // same-root graph only). Pass-through unless cross-root + running.
            // Whether one `||` alternative resolves running-rooted: every member
            // tagged running (mixed-key same text falls target). Same-root runs
            // keep the pass-through below.
            let grp_running =
                cross_running_root.is_some() && atoms.iter().all(|a| running_tags.contains(a));
            // A `||` group with no tree-visible branch still needs one selected here,
            // or the unsatisfiable branch queues as an ordinary dep. Applies to all
            // keys (the merged string hides which key an atom came from); harmless,
            // since those atoms resolve via tree visibility anyway.
            // Portage: --root-deps branch selection; see `root_deps_satisfied_atoms`
            let grp_root: &Path = if grp_running {
                cross_running_root.expect("running-root group without a cross root")
            } else {
                ctx.root
            };
            let scoped_entries: Option<Vec<GraphEntry>> = cross_running_root.map(|_| {
                entries
                    .iter()
                    .filter(|e| e.targets_running_root == grp_running)
                    .cloned()
                    .collect()
            });
            let entries_view: &[GraphEntry] = scoped_entries.as_deref().unwrap_or(entries);
            let scoped_queued: Option<Vec<QueueItem>> = cross_running_root.map(|_| {
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
                self_cp: self_node,
                constraints: union_constraints,
                circular_dependency: circular_view,
                root_deps_running_root: ctx.root_deps_running_root,
                atoms,
                queued: queued_view,
                update: ctx.update,
                gate_entries: entries,
                gate_queued: queued,
            })
        },
    ) else {
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(flat_deps)
}

/// The values [`filter_root_deps`] reads from its caller.
pub(crate) struct FilterRootDepsArgs<'a> {
    pub(crate) flat_deps: Vec<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) key: &'a (String, String),
    pub(crate) state: &'a mut PassState,
    pub(crate) cross_running_root: Option<&'a Path>,
}

/// Apply the `--root-deps` existence split: atoms one root satisfies never
/// reach the ordinary queue, atoms neither satisfies become their own
/// running-root entries. No-op unless cross-root with a split root.
pub(crate) fn filter_root_deps(args: FilterRootDepsArgs<'_>) -> Vec<String> {
    let FilterRootDepsArgs {
        flat_deps,
        ctx,
        config,
        metadata,
        use_flags,
        key,
        state,
        cross_running_root,
    } = args;
    // The existence-check split only runs for flag-on coinciding-roots builds;
    // on a cross-root run the per-group walk above already resolved every build
    // atom against its own root, so filtering here would double-resolve them.
    let root_deps_split: Option<&Path> = ctx
        .root_deps_running_root
        .filter(|_| cross_running_root.is_none());
    // The `ESYSROOT`-vs-`ROOT` distinction; a strict no-op when
    // `root_deps_split` is `None`. See `root_deps_satisfied_atoms`.
    let root_deps_satisfied: HashSet<String> = root_deps_split
        .map(|root| {
            root_deps_satisfied_atoms(RootDepsSatisfiedAtomsArgs {
                metadata,
                use_flags,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
        })
        .unwrap_or_default();
    // Atoms neither root satisfies become their own running-root entries (resolved
    // against the running root directly, in order). Never falls through to the
    // ordinary queue: Portage `DEPEND`/`BDEPEND` never targets `ROOT`/`ESYSROOT`.
    // See `resolve_root_deps_build_entries`. No-op when `root_deps_split` is `None`.
    let root_deps_unsatisfied: Vec<String> = root_deps_split
        .map(|root| {
            unsatisfied_root_deps_atoms(UnsatisfiedRootDepsAtomsArgs {
                metadata,
                use_flags,
                repos: &ctx.repos,
                config,
                running_root: root,
                dep_keys: &["DEPEND", "BDEPEND", "IDEPEND"],
                empty_groups_always_true: md5_dict::eapi_empty_groups_always_true(
                    metadata.get("EAPI").map(String::as_str).unwrap_or("0"),
                ),
            })
        })
        .unwrap_or_default();
    // The two sets above are evaluated-keyed while `flat_deps` is still raw
    // (Portage evaluates at every producer; `enqueue_flat_deps` evaluates at
    // queue time), so membership compares evaluated forms; survivors stay raw.
    let flat_deps: Vec<String> = flat_deps
        .into_iter()
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, use_flags)
                .unwrap_or_else(|| tok.clone());
            !root_deps_satisfied.contains(&evaluated)
        })
        .filter(|tok| {
            let evaluated = portage_dep::evaluate_atom_conditionals(tok, use_flags)
                .unwrap_or_else(|| tok.clone());
            !root_deps_unsatisfied.contains(&evaluated)
        })
        .collect();
    if let Some(running_root) = root_deps_split {
        for atom_str in &root_deps_unsatisfied {
            state.entries.extend(resolve_root_deps_build_entries(
                ResolveRootDepsBuildEntriesArgs {
                    repos: &ctx.repos,
                    running_root,
                    atom_str,
                    config,
                    owner: (*key).clone(),
                    seen: &mut state.root_deps_build_seen,
                    local_binpkg: &ctx.local_binpkg,
                },
            ));
        }
    }
    flat_deps
}

/// The values [`record_slot_pullers_and_enqueue`] reads from its caller.
pub(crate) struct RecordSlotPullersAndEnqueueArgs<'a> {
    pub(crate) flat_deps: Vec<String>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) depth: u32,
    pub(crate) parent_use: &'a HashSet<String>,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
    pub(crate) buildtime_atoms: &'a HashSet<String>,
    pub(crate) runtime_atoms: &'a HashSet<String>,
    pub(crate) bdepend_atoms: &'a HashSet<String>,
    pub(crate) tokens: &'a Vec<String>,
    pub(crate) running_tags: &'a HashSet<String>,
}

/// Record this package as a puller of every non-blocker `cat/pkg` its dep
/// string names, then queue the flattened deps.
pub(crate) fn record_slot_pullers_and_enqueue(args: RecordSlotPullersAndEnqueueArgs<'_>) {
    let RecordSlotPullersAndEnqueueArgs {
        flat_deps,
        key,
        version,
        depth,
        parent_use,
        state,
        entry_idx,
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
        tokens,
        running_tags,
    } = args;
    // Record this package as a puller of every non-blocker `cat/pkg` its
    // dep string names (atom text kept for the `pulled in by` lines).
    for tok in &flat_deps {
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
                    version.to_string(),
                    tok.clone(),
                ));
        }
    }
    enqueue_flat_deps(EnqueueFlatDepsArgs {
        flat_deps,
        key,
        version,
        depth,
        parent_use,
        queue: &mut state.queue,
        pending_blockers: &mut state.pending_blockers,
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
        or_universe: &or_group_universe(tokens),
        running_atoms: running_tags,
        owner_targets_running_root: state.entries[entry_idx].targets_running_root,
    });
}

/// The values [`enqueue_test_deps`] reads from its caller.
pub(crate) struct EnqueueTestDepsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) depth: u32,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) config: &'a Config,
    pub(crate) candidate_str: &'a str,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) tokens: &'a Vec<String>,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
}

/// Queue the `test?`-gated deps too under `--with-test-deps`.
///
/// Additive, never a replacement. A `test?` dep is never hard (optional),
/// never from `BDEPEND`, never a `||` choice, never running.
pub(crate) fn enqueue_test_deps(args: EnqueueTestDepsArgs<'_>) {
    let EnqueueTestDepsArgs {
        ctx,
        depth,
        use_flags,
        metadata,
        config,
        candidate_str,
        key,
        version,
        tokens,
        state,
        entry_idx,
    } = args;
    // `--with-test-deps` is additive, never a replacement; see
    // `resolve_pretend_graph` for the gating. A `test?` dep is never hard
    // (optional), never from `BDEPEND`, never a `||` choice, never running.
    if ctx.with_test_deps && depth == 0 && !use_flags.contains("test") {
        let iuse_flags: HashSet<String> = metadata
            .get("IUSE")
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        let test_masked = config.use_mask.contains("test")
            || specificity_ordered_flags(
                &config.package_use_mask,
                candidate_str,
                &key.0,
                &key.1,
                HashSet::new(),
            )
            .contains("test");
        if iuse_flags.contains("test") && !test_masked {
            let mut test_uselist = use_flags.clone();
            test_uselist.insert("test".to_string());
            let subset: HashSet<String> = ["test".to_string()].into_iter().collect();
            if let Ok(test_deps) = portage_use_reduce::use_reduce_flat_subset(
                tokens,
                &test_uselist,
                portage_use_reduce::MatchMode::Normal,
                &subset,
            ) {
                enqueue_flat_deps(EnqueueFlatDepsArgs {
                    flat_deps: test_deps,
                    key,
                    version,
                    depth,
                    parent_use: &test_uselist,
                    queue: &mut state.queue,
                    pending_blockers: &mut state.pending_blockers,
                    buildtime_atoms: &HashSet::new(),
                    runtime_atoms: &HashSet::new(),
                    bdepend_atoms: &HashSet::new(),
                    or_universe: &HashSet::new(),
                    running_atoms: &HashSet::new(),
                    owner_targets_running_root: state.entries[entry_idx].targets_running_root,
                });
            }
        }
    }
}

/// Fold the entry's dependency keys and queue the atoms they name.
pub(crate) fn enqueue_entry_deps(args: EnqueueEntryDepsArgs<'_>) {
    let EnqueueEntryDepsArgs {
        ctx,
        depth,
        metadata,
        candidate_source,
        state,
        entry_idx,
        use_flags,
        bp,
        config,
        union_constraints,
        key,
        version,
        candidate_str,
    } = args;
    let SelectDepTokensOut {
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
        tokens,
    } = select_dep_tokens(ctx, depth, metadata, candidate_source);
    record_merge_order_deps(RecordMergeOrderDepsArgs {
        ctx,
        candidate_source,
        metadata,
        use_flags,
        state: &mut *state,
        entry_idx,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    });
    let ClassifyDepAtomsOut {
        buildtime_atoms,
        runtime_atoms,
        bdepend_atoms,
    } = classify_dep_atoms(ClassifyDepAtomsArgs {
        ctx,
        candidate_source,
        metadata,
        use_flags,
        onlydeps_blanks_runtime,
        onlydeps_blanks_idepend,
        eapi_has_idepend,
    });
    let scope = prepare_disjunction_scope(PrepareDisjunctionScopeArgs {
        ctx,
        metadata,
        use_flags,
        state: &mut *state,
        entry_idx,
        bp,
    });
    let circular_view = scope
        .circ_with_pullers
        .as_ref()
        .unwrap_or(&bp.circular_dependency);
    let ControlFlow::Continue(flat_deps) = flatten_dep_string(FlattenDepStringArgs {
        tokens: &tokens,
        use_flags,
        metadata,
        cross_running_root: scope.cross_running_root,
        running_tags: &scope.running_tags,
        ctx,
        config,
        entries: &state.entries,
        self_node: &scope.self_node,
        union_constraints,
        circular_view,
        queued: &scope.queued,
    }) else {
        return;
    };
    let flat_deps = filter_root_deps(FilterRootDepsArgs {
        flat_deps,
        ctx,
        config,
        metadata,
        use_flags,
        key,
        state: &mut *state,
        cross_running_root: scope.cross_running_root,
    });
    record_slot_pullers_and_enqueue(RecordSlotPullersAndEnqueueArgs {
        flat_deps,
        key,
        version,
        depth,
        parent_use: use_flags,
        state: &mut *state,
        entry_idx,
        buildtime_atoms: &buildtime_atoms,
        runtime_atoms: &runtime_atoms,
        bdepend_atoms: &bdepend_atoms,
        tokens: &tokens,
        running_tags: &scope.running_tags,
    });
    enqueue_test_deps(EnqueueTestDepsArgs {
        ctx,
        depth,
        use_flags,
        metadata,
        config,
        candidate_str,
        key,
        version,
        tokens: &tokens,
        state: &mut *state,
        entry_idx,
    });
}
