//! Alternate `--solver=` backends: PubGrub and resolvo over the same repo facts.
//!
//! Portuale-only (Portage's `emerge` has no `--solver`): [`super::SolverKind`]'s
//! `PubGrub`/`Resolvo` variants land here. Both backends drive lu-zero's
//! `portage-atom-pubgrub` / `portage-atom-resolvo` bridges instead of the
//! backtracking walk -- the same md5-cache facts in, a [`super::GraphResult`]
//! out, so `pretend.rs` never knows which algorithm resolved the graph.
//!
//! Portuale computes all policy (resolved USE, target atoms, installed set) and
//! hands it in; the engines only pick versions. Facts load on demand per
//! `cat/pkg` over a reachability-closure BFS, so cost scales with the request,
//! not the tree.
//!
//! Deliberate v1 cuts (each would be its own slice):
//! - No autounmask relaxation levels: invisible versions are never offered, but
//!   a candidate needing a flip fails instead of suggesting one.
//! - `slot_conflicts` and the `autounmask_*` lists stay empty; a bridge failure
//!   is one `Error::Detail` line of engine-native text, not a Portage notice.
//! - Merge order is the walk's own `topological_merge_order`; engine install
//!   order only seeds positions.
//! - Unparsable dep-string classes feed empty deps; unparsable versions and
//!   top-level blocker atoms are skipped.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::{
    EffectiveUseFlagsArgs, GraphEntry, GraphResult, InstalledRef, PretendOutcome, RepoConfig,
    ResolveRequest, Resolver as PortualeResolver, all_installed_packages, effective_use_flags,
    find_repos, installed_pkg_iuse_and_use, is_visible, list_candidates, repo_aux_metadata,
};

// --- Lazy fact loading -----------------------------------------------------

/// One repo version with everything a bridge needs except resolved USE
/// (computed on demand by [`LazyRepo::use_of`]).
#[derive(Clone)]
struct LoadedVersion {
    category: String,
    package: String,
    version: String,
    slot: String,
    sub_slot: String,
    repo_name: String,
    iuse: String,
    keywords: Vec<String>,
    /// DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND in that order.
    deps: [Vec<portage_solver::DepEntry>; 5],
    /// The raw md5-cache strings the trees parsed from, for merge-order edge
    /// building over the exact walk-path helper.
    raw_deps: [String; 5],
}

impl LoadedVersion {
    /// `category/package-version` spelling (the plan/map join key).
    fn cpv(&self) -> String {
        format!("{}/{}-{}", self.category, self.package, self.version)
    }
}

/// Every CPN referenced by parsed dep trees (both conditional branches): the
/// over-approximating closure edge.
fn referenced_cpns(entries: &[portage_solver::DepEntry], out: &mut Vec<String>) {
    for entry in entries {
        match entry {
            portage_solver::DepEntry::Atom(dep) => out.push(format!("{}", dep.cpn)),
            portage_solver::DepEntry::UseConditional { children, .. } => {
                referenced_cpns(children, out);
            }
            portage_solver::DepEntry::AllOf(children)
            | portage_solver::DepEntry::AnyOf(children)
            | portage_solver::DepEntry::ExactlyOneOf(children)
            | portage_solver::DepEntry::AtMostOneOf(children) => {
                referenced_cpns(children, out);
            }
        }
    }
}

/// Whether `cat/pkg` names a repo directory: the closure admits only resolvable
/// names, so a dropped dependency stays genuinely absent downstream.
fn cp_exists(repos: &[RepoConfig], cp: &str) -> bool {
    let Some((category, package)) = cp.split_once('/') else {
        return false;
    };
    repos
        .iter()
        .any(|repo| repo.location.join(category).join(package).is_dir())
}

/// Repo facts loaded on demand per `cat/pkg` and memoized, over a reachability
/// closure BFS-seeded from the target + installed CPNs.
///
/// The closure follows both conditional branches USE-unevaluated, so it is a
/// superset of whatever the solver can reach; `effective_use_flags` runs only
/// for versions the solver actually queries.
#[derive(Clone)]
struct LazyRepo {
    repos: Vec<RepoConfig>,
    config: portage_profile::Config,
    /// The closure: every CPN the solver may see (`all_packages`).
    closure: HashSet<String>,
    loaded: RefCell<HashMap<String, Vec<LoadedVersion>>>,
    uses: RefCell<HashMap<String, HashSet<String>>>,
}

impl LazyRepo {
    /// Build the closure from `seeds` (target CPNs, fully expanded) and `shallow`
    /// (installed CPNs: loaded but not expanded): BFS over parsed dep trees.
    fn build(
        req: &ResolveRequest,
        repos: Vec<RepoConfig>,
        seeds: &[String],
        shallow: &[String],
    ) -> Result<Self, super::Error> {
        let mut repo = Self {
            repos,
            config: req.config.clone(),
            closure: HashSet::new(),
            loaded: RefCell::new(HashMap::new()),
            uses: RefCell::new(HashMap::new()),
        };
        // `(cp, expand)`: targets expand, installed only load.
        let mut queue: Vec<(String, bool)> = seeds
            .iter()
            .map(|cp| (cp.clone(), true))
            .chain(shallow.iter().map(|cp| (cp.clone(), false)))
            .collect();
        while let Some((cp, expand)) = queue.pop() {
            if !repo.closure.insert(cp.clone()) {
                continue;
            }
            if !expand {
                repo.load_versions(&cp)?;
                continue;
            }
            let versions = repo.load_versions(&cp)?;
            let mut refs = Vec::new();
            for version in &versions {
                for class in &version.deps {
                    referenced_cpns(class, &mut refs);
                }
            }
            for dep_cp in refs {
                if !repo.closure.contains(&dep_cp) && cp_exists(&repo.repos, &dep_cp) {
                    queue.push((dep_cp, true));
                }
            }
        }
        Ok(repo)
    }

    /// Read + parse every version of one `cat/pkg` (no USE), memoized.
    fn load_versions(&self, cp: &str) -> Result<Vec<LoadedVersion>, super::Error> {
        if let Some(cached) = self.loaded.borrow().get(cp) {
            return Ok(cached.clone());
        }
        let mut versions = Vec::new();
        if let Some((category, package)) = cp.split_once('/') {
            let candidates = list_candidates(&self.repos, category, package)?;
            for candidate in candidates.iter() {
                // Same visibility-filtered pool as the walk: invisible versions are
                // never offered to either engine.
                if !is_visible(candidate, category, package, &self.config) {
                    continue;
                }
                let pf = format!("{package}-{}", candidate.version);
                let Ok(metadata) = repo_aux_metadata(&candidate.repo_location, category, &pf)
                else {
                    continue;
                };
                // Skip versions outside `portage_atom`'s version grammar.
                if portage_solver::Cpv::parse(&format!("{cp}-{}", candidate.version)).is_err() {
                    continue;
                }
                let mut deps: [Vec<portage_solver::DepEntry>; 5] = Default::default();
                let mut raw_deps: [String; 5] = Default::default();
                for (i, key) in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
                    .iter()
                    .enumerate()
                {
                    if let Some(text) = metadata.get(*key) {
                        raw_deps[i] = text.clone();
                        if let Ok(parsed) = portage_solver::DepEntry::parse(text) {
                            deps[i] = parsed;
                        }
                    }
                }
                versions.push(LoadedVersion {
                    category: category.to_string(),
                    package: package.to_string(),
                    version: candidate.version.clone(),
                    slot: candidate.slot.clone(),
                    sub_slot: candidate.sub_slot.clone(),
                    repo_name: candidate.repo_name.clone(),
                    iuse: metadata.get("IUSE").cloned().unwrap_or_default(),
                    keywords: candidate.keywords.clone(),
                    deps,
                    raw_deps,
                });
            }
        }
        self.loaded
            .borrow_mut()
            .insert(cp.to_string(), versions.clone());
        Ok(versions)
    }

    /// One memoized record by `cat/pkg-version`.
    fn record(&self, cp: &str, version: &str) -> Option<LoadedVersion> {
        let versions = self.load_versions(cp).ok()?;
        versions.into_iter().find(|v| v.version == version)
    }

    /// Resolved USE for one loaded version, memoized per cpv.
    fn use_of(&self, version: &LoadedVersion) -> HashSet<String> {
        let cpv = version.cpv();
        if let Some(cached) = self.uses.borrow().get(&cpv) {
            return cached.clone();
        }
        let candidate_str = format!("{cpv}::{}", version.repo_name);
        let use_set = effective_use_flags(EffectiveUseFlagsArgs {
            config: &self.config,
            iuse: &version.iuse,
            keywords: &version.keywords,
            candidate_str: &candidate_str,
            category: &version.category,
            package: &version.package,
        });
        // One clone per miss; the memo keeps its own copy.
        self.uses.borrow_mut().insert(cpv, use_set.as_ref().clone());
        use_set.as_ref().clone()
    }

    /// The closure as parsed CPNs (unparsable names dropped).
    fn closure_cpns(&self) -> Vec<portage_solver::Cpn> {
        let mut out: Vec<portage_solver::Cpn> = self
            .closure
            .iter()
            .filter_map(|cp| portage_solver::Cpn::parse(cp).ok())
            .collect();
        out.sort_by(|a, b| format!("{a}").cmp(&format!("{b}")));
        out
    }
}

/// Target + installed CPN seeds: target atoms' CPNs expand fully (blockers
/// excluded: an unmerge action, not a merge target); installed CPNs load
/// shallowly for favored-version registration and upgrade comparison.
fn closure_seeds(req: &ResolveRequest) -> Result<(Vec<String>, Vec<String>), String> {
    let mut seeds = Vec::new();
    for atom in &req.atoms {
        let dep = portage_solver::Dep::parse(atom)
            .map_err(|e| format!("invalid atom {atom:?}: {e:?}"))?;
        if dep.blocker.is_none() {
            seeds.push(format!("{}", dep.cpn));
        }
    }
    let mut shallow = Vec::new();
    for inst in all_installed_packages(&req.root) {
        shallow.push(format!("{}/{}", inst.category, inst.package));
    }
    seeds.sort();
    seeds.dedup();
    shallow.sort();
    shallow.dedup();
    Ok((seeds, shallow))
}

/// Intern a flag/slot/repo name in the bridges' shared interner.
fn interned(
    value: &str,
) -> portage_solver::interner::Interned<portage_solver::interner::DefaultInterner> {
    portage_solver::interner::Interned::intern(value)
}

/// Split an `IUSE` string into flag names, stripping `+`/`-` default markers.
fn parse_iuse(iuse: &str) -> Vec<String> {
    let mut out: Vec<String> = iuse
        .split_whitespace()
        .map(|token| {
            token
                .strip_prefix('+')
                .or_else(|| token.strip_prefix('-'))
                .unwrap_or(token)
                .to_string()
        })
        .filter(|flag| !flag.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `(category, package, slot) -> installed versions`, plus the
/// `(category, package)` occupancy check behind `new_slot`.
struct InstalledView {
    by_slot: HashMap<(String, String, String), Vec<String>>,
    occupied: HashSet<(String, String)>,
}

impl InstalledView {
    fn of(req: &ResolveRequest) -> Self {
        let mut by_slot: HashMap<(String, String, String), Vec<String>> = HashMap::new();
        let mut occupied = HashSet::new();
        for inst in all_installed_packages(&req.root) {
            by_slot
                .entry((
                    inst.category.clone(),
                    inst.package.clone(),
                    inst.slot.clone(),
                ))
                .or_default()
                .push(inst.version.clone());
            occupied.insert((inst.category, inst.package));
        }
        Self { by_slot, occupied }
    }
}

/// Newest installed version in a slot by `vercmp` ordering.
fn newest_installed(versions: &[String]) -> &str {
    let mut best = &versions[0];
    for candidate in &versions[1..] {
        match portage_versions::vercmp(candidate, best) {
            Some(ordering) if ordering > 0 => best = candidate,
            // Uncomparable versions keep the first (stable, deterministic).
            _ => {}
        }
    }
    best
}

/// Shared `Plan -> GraphResult` mapping: ordered selections plus parent edges
/// become entries with walk-derived `deps`, re-sorted through the walk's own
/// merge order.
fn graph_result_from_order(
    req: &ResolveRequest,
    repos: &[RepoConfig],
    repo: &LazyRepo,
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
) -> GraphResult {
    let installed = InstalledView::of(req);
    let mut entries = Vec::new();
    let mut pending_blockers: Vec<super::PendingBlocker> = Vec::new();
    let mut seen_blockers: HashSet<(String, String, String)> = HashSet::new();
    for (cp, version) in order {
        let Some(record) = repo.record(cp, version) else {
            continue;
        };
        let use_set = repo.use_of(&record);
        let slot_versions = installed.by_slot.get(&(
            record.category.clone(),
            record.package.clone(),
            record.slot.clone(),
        ));
        let (outcome, oldbest, new_slot) = match slot_versions {
            Some(versions) if versions.iter().any(|v| v == version) => (
                PretendOutcome::AlreadyInstalled {
                    version: version.clone(),
                },
                Vec::new(),
                false,
            ),
            Some(versions) => {
                let from = newest_installed(versions).to_string();
                let outcome = match portage_versions::vercmp(version, &from) {
                    Some(ordering) if ordering > 0 => PretendOutcome::Upgrade {
                        from: from.clone(),
                        to: version.clone(),
                    },
                    _ => PretendOutcome::Downgrade {
                        from: from.clone(),
                        to: version.clone(),
                    },
                };
                (
                    outcome,
                    vec![InstalledRef {
                        version: from,
                        slot: record.slot.clone(),
                        sub_slot: record.sub_slot.clone(),
                        repo: record.repo_name.clone(),
                    }],
                    false,
                )
            }
            None => {
                let occupied = installed
                    .occupied
                    .contains(&(record.category.clone(), record.package.clone()));
                let mut oldbest: Vec<InstalledRef> = installed
                    .by_slot
                    .iter()
                    .filter(|((cat, pkg, _), _)| cat == &record.category && pkg == &record.package)
                    .flat_map(|((_, _, slot), versions)| {
                        versions.iter().map(|v| InstalledRef {
                            version: v.clone(),
                            slot: slot.clone(),
                            sub_slot: String::new(),
                            repo: String::new(),
                        })
                    })
                    .collect();
                oldbest.sort_by(|a, b| (&a.slot, &a.version).cmp(&(&b.slot, &b.version)));
                (
                    PretendOutcome::New {
                        version: version.clone(),
                    },
                    oldbest,
                    occupied,
                )
            }
        };
        let use_flags_display: Vec<(String, bool)> = parse_iuse(&record.iuse)
            .into_iter()
            .map(|flag| {
                let on = use_set.contains(&flag);
                (flag, on)
            })
            .collect();
        let empty: HashSet<String> = HashSet::new();
        // Forced/masked-flag `( )` markers via the walk's own helper, with the same
        // candidate spelling the walk resolves under.
        let candidate_str = format!(
            "{cp}-{version}:{}/{sub}::{repo}",
            record.slot,
            sub = record.sub_slot,
            repo = record.repo_name,
        );
        let forced = super::forced_or_masked_flags(super::ForcedOrMaskedFlagsArgs {
            iuse: &record.iuse,
            keywords: &record.keywords,
            candidate_str: &candidate_str,
            category: &record.category,
            package: &record.package,
            config: &req.config,
        });
        // Same `all_flags` split `pretend.rs` uses for `-pv` vs `-p`.
        let use_expand_display =
            super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
                use_flags_display: &use_flags_display,
                config: &req.config,
                installed: None,
                forced: &forced,
                all_flags: true,
                reinst_flags: &empty,
            });
        let use_expand_display_p =
            super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
                use_flags_display: &use_flags_display,
                config: &req.config,
                installed: None,
                forced: &forced,
                all_flags: false,
                reinst_flags: &empty,
            });
        let cpv = format!("{cp}-{version}");
        let mut required_by: Vec<(String, String)> = parents.get(&cpv).cloned().unwrap_or_default();
        required_by.sort();
        required_by.dedup();
        // Entry `deps` from the raw dep strings plus resolved USE, exactly the
        // walk's own call for a source candidate.
        let dep_metadata: HashMap<String, String> = [
            ("DEPEND", record.raw_deps[0].as_str()),
            ("RDEPEND", record.raw_deps[1].as_str()),
            ("BDEPEND", record.raw_deps[2].as_str()),
            ("PDEPEND", record.raw_deps[3].as_str()),
            ("IDEPEND", record.raw_deps[4].as_str()),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let deps = super::merge_order::dep_edges_from_metadata(
            &dep_metadata,
            &use_set,
            &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"],
            false,
        );
        // Blocker atoms in the same raw strings, flattened with USE applied and
        // recorded per owner (deduped); the key index recovers buildtime
        // provenance. Portage: `_add_pkg_dep_string` diverts blockers the same way.
        for (key_idx, raw) in record.raw_deps.iter().enumerate() {
            let toks: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
            let Ok(flat) = portage_use_reduce::use_reduce_flat(
                &toks,
                &use_set,
                portage_use_reduce::MatchMode::Normal,
            ) else {
                continue;
            };
            for tok in flat {
                let evaluated = portage_dep::evaluate_atom_conditionals(&tok, &use_set)
                    .unwrap_or_else(|| tok.clone());
                let Some(dep_atom) = portage_dep::parse_atom(&evaluated) else {
                    continue;
                };
                if dep_atom.blocker == portage_dep::Blocker::None {
                    continue;
                }
                if seen_blockers.insert((
                    record.category.clone(),
                    record.package.clone(),
                    evaluated.clone(),
                )) {
                    pending_blockers.push(super::PendingBlocker {
                        atom_str: evaluated,
                        strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                        // Key-ordered `raw_deps`, so the index is the buildtime provenance.
                        buildtime: key_idx == 0 || key_idx == 2,
                        target_category: dep_atom.category,
                        target_package: dep_atom.package,
                        owner_key: (record.category.clone(), record.package.clone()),
                        owner_version: version.clone(),
                        owner_merging: true,
                        owner_installed: false,
                    });
                }
            }
        }
        entries.push(GraphEntry {
            discovery: 0,
            category: record.category.clone(),
            package: record.package.clone(),
            outcome,
            blockers: Vec::new(),
            slot: Some(record.slot.clone()),
            sub_slot: Some(record.sub_slot.clone()),
            repo_name: Some(record.repo_name.clone()),
            oldbest,
            use_flags_display,
            use_expand_display,
            use_expand_display_p,
            keyword_mask: None,
            new_slot,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by,
            source: super::CandidateSource::Ebuild,
            provenance: super::VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps,
        });
    }
    // Blocker reporting via the shared walk-path helper, filed on owner entries.
    let conflicts = super::resolve_blockers(
        &req.root,
        &pending_blockers,
        &entries,
        // The bridge has no required-set closure of its own.
        &std::collections::HashSet::new(),
        // Never runs under `--buildpkgonly`, so the gate is inert here.
        false,
    );
    let mut orphan_blockers = Vec::new();
    super::file_blocker_conflicts(
        &mut entries,
        &req.root,
        conflicts,
        // Orphans ride the same field rather than being dropped.
        &mut orphan_blockers,
    );
    // ABI rebuilds via the walk's own fixpoint, extended before the merge-order
    // sort so rebuilds land dependency-first.
    let slot_op_reachable: HashSet<(String, String)> = if req.config.complete_seed_atoms.is_empty()
    {
        HashSet::new()
    } else {
        super::required_set_reachable_cps(&req.root, &req.config.complete_seed_atoms, &[])
    };
    let (slot_op_rebuilds, abi_rebuilds) =
        if req.ignore_built_slot_operator_deps || !req.rebuild_if_new_slot {
            (Vec::new(), Vec::new())
        } else {
            {
                // Same `update` flag and top-level atoms the default path resolves
                // under, so the bridge fixpoint runs the new-slot arm too.
                let top_level_cps: HashSet<(String, String)> = req
                    .atoms
                    .iter()
                    .filter_map(|a| portage_dep::parse_atom(a))
                    .map(|atom| (atom.category, atom.package))
                    .collect();
                // The raw request atoms, for the greedy-eligibility half of the gate.
                let top_level_atoms: HashSet<String> = req.atoms.iter().cloned().collect();
                super::slot_operator_rebuild_entries(super::SlotOperatorRebuildEntriesArgs {
                    root: &req.root,
                    repos,
                    entries: &entries,
                    reachable: &slot_op_reachable,
                    with_bdeps: req.with_bdeps,
                    excluded: &req.excluded,
                    update: req.update,
                    top_level_cps: &top_level_cps,
                    top_level_atoms: &top_level_atoms,
                    config: &req.config,
                })
            }
        };
    entries.extend(slot_op_rebuilds);
    // Same merge-order sort the walk applies: engine order only seeds positions.
    let (entries, insertion_rank) =
        super::topological_merge_order(super::TopologicalMergeOrderArgs {
            entries,
            top_level_atoms: &req.atoms,
            config: &req.config,
            root: &req.root,
            implicit_system_deps: req.implicit_system_deps,
            repos,
            dynamic_deps: req.dynamic_deps,
            // No backtrack restart runs here, so no cycle map to demote by.
            circular: &HashMap::new(),
        });
    // Circular-dep notice: rebuild the walk's hard/soft edge-kind map over the
    // entry `deps` edges and report via `find_hard_cycles`, like the walk.
    let mut edge_kinds: super::EdgeKindMap = HashMap::new();
    for e in &entries {
        let owner = (
            e.targets_running_root,
            e.category.clone(),
            e.package.clone(),
        );
        for dep in &e.deps {
            let kinds = edge_kinds
                .entry((
                    (
                        super::dep_child_root(e.targets_running_root, dep.key),
                        dep.category.clone(),
                        dep.package.clone(),
                    ),
                    owner.clone(),
                ))
                .or_insert((false, false));
            if dep.priority.buildtime
                && super::best_installed_for_atom(&req.root, &dep.atom, &dep.category, &dep.package)
                    .is_none()
            {
                kinds.0 = true;
            } else {
                kinds.1 = true;
            }
        }
    }
    let circular_deps = super::find_hard_cycles(
        &entries,
        &edge_kinds,
        &req.root,
        &HashMap::new(),
        Some(&insertion_rank),
    );
    // Cycle report only when a hard cycle was reported.
    let (large_cycle_count, cycle_display) = if circular_deps.is_empty() {
        (false, Vec::new())
    } else {
        let (cycles, display) =
            super::merge_order::cycle_report(&entries, &req.atoms, &req.root, &HashMap::new());
        let display = display
            .into_iter()
            .filter_map(|i| super::merge_bound_cpv(&entries[i]))
            .collect();
        (cycles.len() > 3, display)
    };
    GraphResult {
        entries,
        // No `Backtracker` loop: no restarts, and `--backtrack` is not consumed.
        backtrack_restarts: 0,
        backtrack_max: req.backtrack_max,
        // No restart runs, so no cycle map is ever recorded.
        circular_dependency: HashMap::new(),
        // Always `Complete`: an engine plan cannot abort the way the walk can.
        outcome: super::ResolveOutcome::Complete,
        slot_conflicts: Vec::new(),
        skipped_updates: Vec::new(),
        // No backtrack loop: no trial masks, nothing for either notice.
        skipped_missing_deps: Vec::new(),
        skipped_missing_dep_full: Vec::new(),
        orphan_blockers,
        changed_deps_report: Vec::new(),
        buildpkgonly_deps_unsatisfied: false,
        prune_unresolved: false,
        pprovided_atoms: Vec::new(),
        // Never drops a set member (tests resolve explicit atoms).
        missing_args: Vec::new(),
        autounmask_keyword_changes: Vec::new(),
        autounmask_use_changes: Vec::new(),
        autounmask_license_changes: Vec::new(),
        autounmask_mask_changes: Vec::new(),
        // No relaxation loop ran, so no early-termination gate can hinge on flips.
        autounmask_no_clean_tail: false,
        settled_with_backtracking: false,
        abi_rebuilds,
        circular_deps,
        large_cycle_count,
        cycle_display,
        // A solved plan admits no divergence and ran no relaxation loop.
        masked_deps: Vec::new(),
        // No BFS walk: no `[use]`-unsatisfied disclosure.
        use_unsat_deps: Vec::new(),
        // No BFS walk: no plain-miss disclosure.
        plain_miss_deps: Vec::new(),
        // No probe walk runs here, and the outcome is always `Complete`.
        virtual_cycle: Vec::new(),
    }
}

/// Named slot from a target atom's `:slot` part, if it names one
/// (`SlotDep`'s fields are private, so this reads its `Display`).
fn target_slot(dep: &portage_solver::Dep) -> Option<String> {
    let text = dep.slot_dep.as_ref().map(|slot| format!("{slot}"))?;
    if text.starts_with('=') || text.starts_with('*') {
        return None;
    }
    let named = text.split('/').next().unwrap_or("");
    let named = named.strip_suffix('=').unwrap_or(named);
    if named.is_empty() {
        None
    } else {
        Some(named.to_string())
    }
}

// --- PubGrub backend -------------------------------------------------------

/// `portage-atom-pubgrub` over portuale's facts: the [`super::Resolver`]
/// behind [`super::SolverKind::PubGrub`].
pub struct PubGrubResolver;

impl PortualeResolver for PubGrubResolver {
    fn resolve(&self, req: &ResolveRequest) -> Result<GraphResult, super::Error> {
        resolve_pubgrub(req).map_err(super::Error::Detail)
    }
}

/// Facts over a [`LazyRepo`]: answers from memoized per-CPN loads, so the
/// provider only ever pays for the closure.
struct BridgeRepo {
    lazy: LazyRepo,
}

impl portage_atom_pubgrub::PackageRepository for BridgeRepo {
    fn all_packages(&self) -> Vec<portage_solver::Cpn> {
        self.lazy.closure_cpns()
    }

    fn versions_for(
        &self,
        cpn: &portage_solver::Cpn,
    ) -> Vec<(portage_solver::Cpv, portage_atom_pubgrub::PackageVersions)> {
        use portage_atom_pubgrub::{IUseDefault, PackageDeps, PackageVersions};
        let cp = format!("{cpn}");
        let versions = self.lazy.load_versions(&cp).unwrap_or_default();
        let mut out = Vec::new();
        for record in &versions {
            let Ok(cpv) = portage_solver::Cpv::parse(&record.cpv()) else {
                continue;
            };
            let slot = if record.slot.is_empty() {
                None
            } else {
                Some(interned(&record.slot))
            };
            let subslot = if record.sub_slot.is_empty() || record.sub_slot == record.slot {
                None
            } else {
                Some(interned(&record.sub_slot))
            };
            let iuse: Vec<_> = parse_iuse(&record.iuse)
                .into_iter()
                .map(|flag| interned(&flag))
                .collect();
            let mut iuse_defaults = HashMap::new();
            for token in record.iuse.split_whitespace() {
                if let Some(flag) = token.strip_prefix('+') {
                    iuse_defaults.insert(interned(flag), IUseDefault::Enabled);
                } else if let Some(flag) = token.strip_prefix('-') {
                    iuse_defaults.insert(interned(flag), IUseDefault::Disabled);
                }
            }
            out.push((
                cpv,
                PackageVersions {
                    slot,
                    subslot,
                    repo: Some(interned(&record.repo_name)),
                    iuse,
                    iuse_defaults,
                    deps: PackageDeps::new(
                        record.deps[0].clone(),
                        record.deps[1].clone(),
                        record.deps[2].clone(),
                        record.deps[3].clone(),
                        record.deps[4].clone(),
                    ),
                    required_use: None,
                },
            ));
        }
        out
    }

    fn desired_use(&self, cpv: &portage_solver::Cpv) -> portage_atom_pubgrub::UseConfig {
        use portage_atom_pubgrub::UseConfig;
        let cp_text = format!("{}", cpv.cpn);
        let version_text = format!("{}", cpv.version);
        let Some(record) = self.lazy.record(&cp_text, &version_text) else {
            return UseConfig::new();
        };
        let mut use_config = UseConfig::new();
        for flag in self.lazy.use_of(&record) {
            use_config.enable(interned(&flag));
        }
        use_config
    }
}

fn resolve_pubgrub(req: &ResolveRequest) -> Result<GraphResult, String> {
    use portage_atom_pubgrub::{
        InstalledPackage as BridgeInstalled, InstalledPolicy, PortageDependencyProvider,
        PortagePackage, PortageVersionSet,
    };
    let repos = find_repos(&req.config_root).map_err(|e| format!("finding repos: {e}"))?;
    let (seeds, shallow) = closure_seeds(req)?;
    let seed_cpns: Vec<portage_solver::Cpn> = seeds
        .iter()
        .chain(shallow.iter())
        .filter_map(|cp| portage_solver::Cpn::parse(cp).ok())
        .collect();
    let lazy = LazyRepo::build(req, repos, &seeds, &shallow)
        .map_err(|e| format!("reading repo facts: {e}"))?;
    // Reachable conversion, not whole-tree; the provider takes a clone while the
    // mapping keeps the original (memoized maps make the copy cheap).
    let mut provider =
        PortageDependencyProvider::new_for_targets(BridgeRepo { lazy: lazy.clone() }, seed_cpns);
    for inst in all_installed_packages(&req.root) {
        let cpn = match portage_solver::Cpn::parse(&format!("{}/{}", inst.category, inst.package)) {
            Ok(cpn) => cpn,
            Err(_) => continue,
        };
        let version = match portage_solver::Version::parse(&inst.version) {
            Ok(version) => version,
            Err(_) => continue,
        };
        let package = if inst.slot.is_empty() {
            PortagePackage::unslotted(cpn)
        } else {
            PortagePackage::slotted(cpn, interned(&inst.slot))
        };
        let (iuse_set, use_set) =
            installed_pkg_iuse_and_use(&req.root, &inst.category, &inst.package, &inst.version);
        provider.add_installed(BridgeInstalled {
            package,
            version,
            policy: InstalledPolicy::Favor,
            active_use: use_set.into_iter().map(|f| interned(&f)).collect(),
            iuse: iuse_set.into_iter().map(|f| interned(&f)).collect(),
        });
    }
    let mut targets = Vec::new();
    for atom in &req.atoms {
        let dep = portage_solver::Dep::parse(atom)
            .map_err(|e| format!("invalid atom {atom:?}: {e:?}"))?;
        // Top-level blockers are an unmerge action, not a merge target.
        if dep.blocker.is_some() {
            continue;
        }
        let versions = match (&dep.op, &dep.version) {
            (Some(op), Some(version)) => {
                PortageVersionSet::from_operator(*op, dep.glob, version.clone())
            }
            _ => PortageVersionSet::any(),
        };
        // A named `:slot` pins one slotted node, otherwise every slotted node.
        match target_slot(&dep) {
            Some(slot) => {
                targets.push((PortagePackage::slotted(dep.cpn, interned(&slot)), versions))
            }
            None => {
                let nodes = provider.packages_for_cpn(&dep.cpn);
                if nodes.is_empty() {
                    targets.push((PortagePackage::unslotted(dep.cpn), versions));
                } else {
                    targets.extend(nodes.into_iter().map(|n| (n, versions.clone())));
                }
            }
        }
    }
    let solution = provider
        .resolve_targets(targets)
        .map_err(portage_atom_pubgrub::format_solve_error)?;
    let mut order = Vec::new();
    for (pkg, ver) in provider.install_order(&solution) {
        if let portage_atom_pubgrub::PortagePackage::Real { cpn, .. } = &pkg {
            order.push((format!("{cpn}"), format!("{ver}")));
        }
    }
    let mut parents: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for edge in provider.dependency_graph(&solution) {
        let portage_atom_pubgrub::PortagePackage::Real { cpn: from_cpn, .. } = &edge.from.0 else {
            continue;
        };
        let (portage_atom_pubgrub::PortagePackage::Real { cpn: to_cpn, .. }, to_ver) = &edge.to
        else {
            continue;
        };
        let (from_cat, from_pkg) = match format!("{from_cpn}").split_once('/') {
            Some(pair) => (pair.0.to_string(), pair.1.to_string()),
            None => continue,
        };
        parents
            .entry(format!("{to_cpn}-{to_ver}"))
            .or_default()
            .push((from_cat, from_pkg));
    }
    Ok(graph_result_from_order(
        req,
        &lazy.repos,
        &lazy,
        &order,
        &parents,
    ))
}

// --- resolvo backend -------------------------------------------------------

/// `portage-atom-resolvo` over portuale's facts: the [`super::Resolver`]
/// behind [`super::SolverKind::Resolvo`].
pub struct ResolvoResolver;

impl PortualeResolver for ResolvoResolver {
    fn resolve(&self, req: &ResolveRequest) -> Result<GraphResult, super::Error> {
        resolve_resolvo(req).map_err(super::Error::Detail)
    }
}

fn resolve_resolvo(req: &ResolveRequest) -> Result<GraphResult, String> {
    use portage_atom_resolvo::{
        InMemoryRepository, InstalledPolicy, InstalledSet, PackageDeps as RvDeps, PackageMetadata,
        PortageDependencyProvider, UseConfig as RvUseConfig,
    };
    let repos = find_repos(&req.config_root).map_err(|e| format!("finding repos: {e}"))?;
    let (seeds, shallow) = closure_seeds(req)?;
    let lazy = LazyRepo::build(req, repos, &seeds, &shallow)
        .map_err(|e| format!("reading repo facts: {e}"))?;
    // The pool is inherently whole-input, so the input is the closure.
    let mut repo = InMemoryRepository::new();
    for cp in lazy.closure_cpns() {
        let cp_text = format!("{cp}");
        let versions = lazy
            .load_versions(&cp_text)
            .map_err(|e| format!("reading repo facts: {e}"))?;
        for record in &versions {
            let Ok(cpv) = portage_solver::Cpv::parse(&record.cpv()) else {
                continue;
            };
            let iuse: Vec<_> = parse_iuse(&record.iuse)
                .into_iter()
                .map(|flag| interned(&flag))
                .collect();
            let use_flags: HashSet<_> = lazy
                .use_of(record)
                .into_iter()
                .map(|f| interned(&f))
                .collect();
            repo.add(PackageMetadata {
                cpv,
                slot: if record.slot.is_empty() {
                    None
                } else {
                    Some(interned(&record.slot))
                },
                subslot: if record.sub_slot.is_empty() || record.sub_slot == record.slot {
                    None
                } else {
                    Some(interned(&record.sub_slot))
                },
                iuse,
                use_flags,
                repo: Some(interned(&record.repo_name)),
                dependencies: RvDeps {
                    depend: record.deps[0].clone(),
                    rdepend: record.deps[1].clone(),
                    bdepend: record.deps[2].clone(),
                    pdepend: record.deps[3].clone(),
                    idepend: record.deps[4].clone(),
                },
            });
        }
    }
    let mut installed = InstalledSet::new();
    for inst in all_installed_packages(&req.root) {
        let Ok(cpv) = portage_solver::Cpv::parse(&format!(
            "{}/{}-{}",
            inst.category, inst.package, inst.version
        )) else {
            continue;
        };
        installed.add(
            PackageMetadata {
                cpv,
                slot: if inst.slot.is_empty() {
                    None
                } else {
                    Some(interned(&inst.slot))
                },
                subslot: None,
                iuse: Vec::new(),
                use_flags: HashSet::new(),
                repo: None,
                dependencies: RvDeps::default(),
            },
            InstalledPolicy::Favored,
        );
    }
    // Per-version USE rides in each metadata; the provider config stays default.
    let use_config = RvUseConfig::default();
    let mut provider = PortageDependencyProvider::with_installed(&repo, &use_config, &installed);
    let mut reqs = Vec::new();
    for atom in &req.atoms {
        let dep = portage_solver::Dep::parse(atom)
            .map_err(|e| format!("invalid atom {atom:?}: {e:?}"))?;
        if dep.blocker.is_some() {
            continue;
        }
        reqs.push(provider.intern_requirement(&dep));
    }
    let problem = resolvo::Problem::new().requirements(reqs);
    let mut solver = resolvo::Solver::new(provider);
    // Engine-native failure text naming the packages, never the internals dump.
    let solution = match solver.solve(problem) {
        Ok(solution) => solution,
        Err(resolvo::UnsolvableOrCancelled::Unsolvable(conflict)) => {
            return Err(format!("{}", conflict.display_user_friendly(&solver)));
        }
        Err(resolvo::UnsolvableOrCancelled::Cancelled(_)) => {
            return Err("dependency resolution was cancelled".to_string());
        }
    };
    // Items are references, so copy explicitly.
    #[allow(clippy::iter_cloned_collect)]
    let ids: Vec<resolvo::SolvableId> = solution.iter().copied().collect();
    let provider = solver.provider();
    let ordered = provider
        .install_order(&ids)
        .map_err(|left| format!("dependency cycle left unorderable: {left:?}"))?;
    let mut order = Vec::new();
    for sid in &ordered {
        let meta = provider.package_metadata(*sid);
        order.push((format!("{}", meta.cpv.cpn), format!("{}", meta.cpv.version)));
    }
    let mut parents: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for edge in provider.dependency_graph(&ids) {
        let from = provider.package_metadata(edge.from);
        let to = provider.package_metadata(edge.to);
        let (from_cat, from_pkg) = match format!("{}", from.cpv.cpn).split_once('/') {
            Some(pair) => (pair.0.to_string(), pair.1.to_string()),
            None => continue,
        };
        parents
            .entry(format!("{}", to.cpv))
            .or_default()
            .push((from_cat, from_pkg));
    }
    Ok(graph_result_from_order(
        req,
        &lazy.repos,
        &lazy,
        &order,
        &parents,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::SolverKind;
    use portage_util::TempDir;

    #[test]
    fn solver_kind_parses_the_three_accepted_values() {
        assert_eq!(SolverKind::parse("portage"), Some(SolverKind::Portage));
        assert_eq!(SolverKind::parse("pubgrub"), Some(SolverKind::PubGrub));
        assert_eq!(SolverKind::parse("resolvo"), Some(SolverKind::Resolvo));
        assert_eq!(SolverKind::parse(""), None);
        assert_eq!(SolverKind::parse("PORTAGE"), None);
        assert_eq!(SolverKind::parse("sat"), None);
    }

    #[test]
    fn solver_kind_round_trips_through_display() {
        assert_eq!(SolverKind::Portage.as_str(), "portage");
        assert_eq!(SolverKind::PubGrub.as_str(), "pubgrub");
        assert_eq!(SolverKind::Resolvo.as_str(), "resolvo");
        assert_eq!(format!("{}", SolverKind::Resolvo), "resolvo");
        assert_eq!(SolverKind::default(), SolverKind::Portage);
    }

    fn fixture_request(atoms: &[&str]) -> super::super::ResolveRequest {
        use super::super::{Deep, ResolveRequest, SolverKind};
        use std::path::PathBuf;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("fixtures/ does not resolve: it is a symlink to ../pmtest/fixtures, so the sibling pmtest repo has to be checked out next to this one");
        // Same keyword set a Portage resolve sees.
        let mut config = portage_profile::Config::default();
        config.accept_keywords.insert("amd64".to_string());
        ResolveRequest {
            config_root: root.to_path_buf(),
            root,
            atoms: atoms.iter().map(|s| s.to_string()).collect(),
            config,
            newuse: false,
            changed_use: false,
            nodeps: false,
            // The parked engines do not implement `--onlydeps` root filtering.
            onlydeps: false,
            onlydeps_with_rdeps: true,
            onlydeps_with_ideps: false,
            update: false,
            deep: Deep::NotRequested,
            excluded: Vec::new(),
            with_bdeps: false,
            changed_deps: false,
            changed_slot: false,
            with_test_deps: false,
            changed_deps_report: false,
            selective: false,
            autounmask_suggest_keywords: false,
            autounmask_suggest_use: false,
            autounmask_suggest_license: false,
            autounmask_suggest_masks: false,
            usepkg: false,
            usepkgonly: false,
            binpkg_respect_use: false,
            usepkg_exclude: Vec::new(),
            usepkg_include: Vec::new(),
            rebuilt_binaries: false,
            rebuilt_binaries_timestamp: None,
            newrepo: false,
            buildpkgonly: false,
            root_deps_running_root: None,
            distdir: PathBuf::from("/tmp"),
            empty: false,
            getbinpkg: false,
            ignore_built_slot_operator_deps: false,
            backtrack_max: 10,
            reinstall_atoms: Vec::new(),
            rebuild_if_new_slot: false,
            rebuild_if_unbuilt: false,
            rebuild_if_new_rev: false,
            rebuild_if_new_ver: false,
            rebuild_exclude: Vec::new(),
            rebuild_ignore: Vec::new(),
            dynamic_deps: false,
            implicit_system_deps: true,
            complete: false,
            set_args: std::collections::HashSet::new(),
            solver: SolverKind::Portage,
        }
    }

    /// The closure covers only the requested reachability: md5 reads, dep parses
    /// and USE runs scale with the request, never the tree.
    #[test]
    fn closure_covers_only_the_requested_reachability() {
        use super::super::find_repos;
        let req = fixture_request(&["dev-libs/newpkg"]);
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        assert!(seeds.contains(&"dev-libs/newpkg".to_string()));
        let lazy = super::LazyRepo::build(&req, repos, &seeds, &shallow).expect("closure builds");
        let closure: std::collections::HashSet<String> = lazy
            .closure_cpns()
            .into_iter()
            .map(|cpn| format!("{cpn}"))
            .collect();
        assert!(closure.contains("dev-libs/newpkg"));
        // No other fixture package is reachable from newpkg.
        assert!(
            !closure.contains("dev-libs/diamond"),
            "closure leaked: {closure:?}"
        );
        // The diamond target pulls its whole over-approximated cone.
        let req = fixture_request(&["dev-libs/diamond"]);
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy = super::LazyRepo::build(&req, repos, &seeds, &shallow).expect("closure builds");
        let closure: std::collections::HashSet<String> = lazy
            .closure_cpns()
            .into_iter()
            .map(|cpn| format!("{cpn}"))
            .collect();
        for expected in [
            "dev-libs/diamond",
            "dev-libs/shared-a",
            "dev-libs/shared-b",
            "dev-libs/common",
        ] {
            assert!(
                closure.contains(expected),
                "missing {expected}: {closure:?}"
            );
        }
        assert!(
            !closure.contains("dev-libs/newpkg"),
            "closure leaked: {closure:?}"
        );
    }

    /// The pool is visibility-filtered: `~amd64`-only offers nothing while `amd64`
    /// keeps its version.
    #[test]
    fn load_versions_filters_invisible_candidates() {
        use super::super::find_repos;
        let req = fixture_request(&["dev-libs/maskedpkg"]);
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy = super::LazyRepo::build(&req, repos, &seeds, &shallow).expect("closure builds");
        assert!(
            lazy.load_versions("dev-libs/maskedpkg")
                .expect("loads maskedpkg")
                .is_empty(),
            "the ~amd64-only version must be filtered out of the pool"
        );
        let newpkg = lazy.load_versions("dev-libs/newpkg").expect("loads newpkg");
        assert_eq!(newpkg.len(), 1);
        assert_eq!(newpkg[0].version, "1.0");
    }
}
