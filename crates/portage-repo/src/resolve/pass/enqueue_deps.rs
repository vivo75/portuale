//! Fold the entry's dependency keys and queue the atoms they name.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

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
