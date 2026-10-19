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
//!   a candidate needing a keyword/license/mask flip fails instead of
//!   suggesting one.
//! - `slot_conflicts` stays empty and a bridge failure is one `Error::Detail`
//!   line of engine-native text, not a Portage notice. Since S9 the pubgrub
//!   path replays the walk's abort predicates read-only over the solved plan,
//!   so `autounmask_use_changes` and the miss disclosures can fill.
//! - Merge order is the walk's own `topological_merge_order`; engine install
//!   order only seeds positions.
//! - Unparsable dep-string classes feed empty deps; unparsable versions and
//!   top-level blocker atoms are skipped.
//! - Binary candidates (`--usepkg`) are not offered; `--deep=N` bounds kept
//!   expansion through a pre-solve BFS allowlist (the engine cannot carry
//!   depth); no `_greedy_slots` blocker lookahead (backlog #368).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::Write;

use super::{
    AbortOutcomeArgs, AutounmaskChange, Deep, EffectiveUseFlagsArgs, GraphEntry, GraphResult,
    InstalledDepStringArgs, InstalledMetaLayer, InstalledRef, InstalledUseState,
    MaskedDepChainArgs, MaskedDepReport, PlainMissDepReport, PretendOutcome,
    ReinstallFlagsForUseChangeArgs, RenderRequiredUseBlockArgs, RepoConfig, ResolveRequest,
    Resolver as PortualeResolver, TopLevel, UseUnsatCandidatesForAtomArgs, UseUnsatDepReport,
    abort_outcome, all_installed_packages, autounmask_use_atom_form, effective_use_flags,
    fill_use_change_chains, find_repos, implicit_iuse_set, installed_candidates,
    installed_dep_string, installed_pkg_iuse_and_use, is_visible, list_candidates,
    live_metadata_for_installed, masked_candidates_for_atom, masked_dep_chain, pkg_use_display_for,
    read_vdb_flag_set, read_vdb_string, reinstall_flags_for_use_change, render_required_use_block,
    repo_aux_metadata, required_use_dep_chain, split_slot, suggested_use_flip,
    use_unsat_candidates_for_atom, valid_iuse,
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
    /// Raw `REQUIRED_USE` for the post-solve abort validator (the engine
    /// never sees it: `version_row` keeps passing `None`).
    required_use: String,
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
    root: std::path::PathBuf,
    dynamic_deps: bool,
    ignore_built_slot_operator_deps: bool,
    /// The closure: every CPN the solver may see (`all_packages`).
    closure: HashSet<String>,
    /// The expanded cone inside it: seeds plus everything reached by expanding
    /// dep trees (shallow installed loads excluded). What Portage could have
    /// traversed to (USE-unevaluated superset); read by the G7 blocker
    /// parent test, never by either engine.
    expanded: HashSet<String>,
    loaded: RefCell<HashMap<String, Vec<LoadedVersion>>>,
    uses: RefCell<HashMap<String, HashSet<String>>>,
    /// Per-run memo for the installed-deps reader below.
    installed_dep_memo: RefCell<HashMap<(String, String, String, String), String>>,
}

impl LazyRepo {
    /// Build the closure from `seeds` (target CPNs, fully expanded) and `shallow`
    /// (installed CPNs: loaded but not expanded): BFS over parsed dep trees.
    ///
    /// `seeds_first` queues the expanding seeds above the shallow installed
    /// entries, so `Vec::pop` drains the whole target cone before any installed
    /// CPN loads shallow. Pubgrub only (#34 S2: an installed target must expand,
    /// else its deps never enter `closure`); resolvo keeps the old order so its
    /// pool -- and its output -- does not move.
    fn build(
        req: &ResolveRequest,
        repos: Vec<RepoConfig>,
        seeds: &[String],
        shallow: &[String],
        seeds_first: bool,
    ) -> Result<Self, super::Error> {
        let mut repo = Self {
            repos,
            config: req.config.clone(),
            root: req.root.clone(),
            dynamic_deps: req.dynamic_deps,
            ignore_built_slot_operator_deps: req.ignore_built_slot_operator_deps,
            closure: HashSet::new(),
            expanded: HashSet::new(),
            loaded: RefCell::new(HashMap::new()),
            uses: RefCell::new(HashMap::new()),
            installed_dep_memo: RefCell::new(HashMap::new()),
        };
        // Under deep, an installed cpv's own vdb deps decide what it reaches
        // (#34 S3) -- including versions gone from the repo -- so installed
        // CPNs expand from the vdb here. Pubgrub only, like `seeds_first`:
        // resolvo keeps the old order so its pool does not move.
        let vdb_expand = seeds_first && (!matches!(req.deep, Deep::NotRequested) || req.empty);
        // `(cp, expand)`: targets expand, installed only load. Seeds queue last
        // when `seeds_first`, so LIFO `pop` expands the target cone first: a CPN
        // that is both a seed and installed expands instead of loading shallow.
        let mut queue: Vec<(String, bool)> = if seeds_first {
            shallow
                .iter()
                .map(|cp| (cp.clone(), false))
                .chain(seeds.iter().map(|cp| (cp.clone(), true)))
                .collect()
        } else {
            seeds
                .iter()
                .map(|cp| (cp.clone(), true))
                .chain(shallow.iter().map(|cp| (cp.clone(), false)))
                .collect()
        };
        while let Some((cp, expand)) = queue.pop() {
            if !repo.closure.insert(cp.clone()) {
                continue;
            }
            if expand {
                repo.expanded.insert(cp.clone());
            }
            pg_trace(
                &mut std::io::stderr(),
                format_args!(
                    "pg-closure {cp} {}",
                    if expand { "expanded" } else { "shallow" }
                ),
            );
            if !expand {
                repo.load_versions(&cp)?;
                if vdb_expand && let Some((category, package)) = cp.split_once('/') {
                    // The vdb (not the repo md5-cache) says what an installed
                    // cpv reaches under deep; referenced CPNs expand like
                    // seed-referenced ones so unslotted deps on them resolve.
                    for (version, _, _) in installed_candidates(&req.root, category, package) {
                        for text in &repo.installed_dep_strings(category, package, &version) {
                            let Ok(parsed) = portage_solver::DepEntry::parse(text) else {
                                continue;
                            };
                            let mut refs = Vec::new();
                            referenced_cpns(&parsed, &mut refs);
                            for dep_cp in refs {
                                if !repo.closure.contains(&dep_cp)
                                    && cp_exists(&repo.repos, &dep_cp)
                                {
                                    queue.push((dep_cp, true));
                                }
                            }
                        }
                    }
                }
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
                    required_use: metadata.get("REQUIRED_USE").cloned().unwrap_or_default(),
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

    /// One installed package's five dep strings through the default resolver's
    /// own dynamic-deps reader (live repo metadata for the recorded repo plus
    /// the vdb's own built `:=` atoms, moves applied), in
    /// DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND order.
    /// Portage: FakeVartree.py, _apply_dynamic_deps.
    fn installed_dep_strings(&self, category: &str, package: &str, version: &str) -> [String; 5] {
        let live = live_metadata_for_installed(&self.repos, &self.root, category, package, version);
        let mut memo = self.installed_dep_memo.borrow_mut();
        let mut one = |key: &str| {
            installed_dep_string(InstalledDepStringArgs {
                root: &self.root,
                dynamic_deps: self.dynamic_deps,
                ignore_built_slot_operator_deps: self.ignore_built_slot_operator_deps,
                category,
                package,
                version,
                live: live.as_deref(),
                key,
                layer: InstalledMetaLayer::Effective,
                memo: &mut memo,
            })
        };
        [
            one("DEPEND"),
            one("RDEPEND"),
            one("BDEPEND"),
            one("PDEPEND"),
            one("IDEPEND"),
        ]
    }

    /// The parsed form of [`LazyRepo::installed_dep_strings`]: unparsable
    /// classes stay empty, like the repo path in [`LazyRepo::load_versions`].
    fn installed_parsed_deps(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> [Vec<portage_solver::DepEntry>; 5] {
        let mut deps: [Vec<portage_solver::DepEntry>; 5] = Default::default();
        for (slot, text) in deps
            .iter_mut()
            .zip(self.installed_dep_strings(category, package, version))
        {
            if let Ok(parsed) = portage_solver::DepEntry::parse(&text) {
                *slot = parsed;
            }
        }
        deps
    }

    /// An installed version absent from the repo as a record with its vdb deps
    /// (moves applied), so the engine sees Portage's vdb expansion instead of
    /// the crate's empty-deps stub. `None` for a missing vdb entry or an
    /// unparsable version. The vdb carries no keywords; the slot splits the
    /// same way as every other installed-metadata reader.
    /// Portage: FakeVartree.py, _aux_get_wrapper; resolve/installed_meta.rs:126-130.
    fn installed_record(
        &self,
        category: &str,
        package: &str,
        version: &str,
    ) -> Option<LoadedVersion> {
        if read_vdb_string(&self.root, category, package, version, "CATEGORY").is_empty() {
            return None;
        }
        if portage_solver::Cpv::parse(&format!("{category}/{package}-{version}")).is_err() {
            return None;
        }
        let (slot, sub_slot) = split_slot(&read_vdb_string(
            &self.root, category, package, version, "SLOT",
        ));
        let raw_deps = self.installed_dep_strings(category, package, version);
        let mut deps: [Vec<portage_solver::DepEntry>; 5] = Default::default();
        for (slot, text) in deps.iter_mut().zip(raw_deps.iter()) {
            if let Ok(parsed) = portage_solver::DepEntry::parse(text) {
                *slot = parsed;
            }
        }
        Some(LoadedVersion {
            category: category.to_string(),
            package: package.to_string(),
            version: version.to_string(),
            slot,
            sub_slot,
            repo_name: read_vdb_string(&self.root, category, package, version, "repository"),
            iuse: read_vdb_string(&self.root, category, package, version, "IUSE"),
            required_use: read_vdb_string(&self.root, category, package, version, "REQUIRED_USE"),
            keywords: Vec::new(),
            deps,
            raw_deps,
        })
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
            build_id: None,
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

    /// Whether `category/package` was reached by expanding dep trees (not a
    /// shallow installed load): the G7 blocker parent test's traversal model.
    fn expanded_contains(&self, category: &str, package: &str) -> bool {
        self.expanded.contains(&format!("{category}/{package}"))
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

/// One `(Cpv, PackageVersions)` row for a record whose dep classes are already
/// chosen (repo md5-cache or the vdb reader): slot/sub-slot, repo, IUSE with
/// defaults, and the five classes in DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND
/// order. `None` for an unparsable cpv.
fn version_row(
    record: &LoadedVersion,
    deps: [Vec<portage_solver::DepEntry>; 5],
) -> Option<(portage_solver::Cpv, portage_atom_pubgrub::PackageVersions)> {
    use portage_atom_pubgrub::{IUseDefault, PackageDeps, PackageVersions};
    let cpv = portage_solver::Cpv::parse(&record.cpv()).ok()?;
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
    let [d0, d1, d2, d3, d4] = deps;
    Some((
        cpv,
        PackageVersions {
            slot,
            subslot,
            repo: Some(interned(&record.repo_name)),
            iuse,
            iuse_defaults,
            deps: PackageDeps::new(d0, d1, d2, d3, d4),
            required_use: None,
        },
    ))
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
///
/// `kept` is the pubgrub bridge's kept/rebuild set: a same-slot same-version pick
/// in `kept.rebuild` is a `Reinstall` (Portage's bare, reasonless `[ebuild R]`),
/// resolvo passes `None` and its output does not move.
// Six inputs ride an explicit allow instead of a one-use args struct.
#[allow(clippy::too_many_arguments)]
fn graph_result_from_order(
    req: &ResolveRequest,
    repos: &[RepoConfig],
    repo: &LazyRepo,
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
    kept: Option<&KeptInstalled>,
) -> GraphResult {
    let installed = InstalledView::of(req);
    let mut entries = Vec::new();
    let mut pending_blockers: Vec<super::PendingBlocker> = Vec::new();
    let mut seen_blockers: HashSet<(String, String, String)> = HashSet::new();
    for (cp, version) in order {
        let Some(entry) = bridge_entry_for(
            cp,
            version,
            repo,
            &installed,
            kept,
            req,
            parents,
            &mut pending_blockers,
            &mut seen_blockers,
        ) else {
            continue;
        };
        entries.push(entry);
    }
    let orphan_blockers =
        file_bridge_blocker_conflicts(req, repo, &mut entries, kept, &pending_blockers);
    let (slot_op_rebuilds, abi_rebuilds) = bridge_slot_operator_rebuilds(req, repos, &entries);
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
    let (circular_deps, large_cycle_count, cycle_display) =
        bridge_circular_display(req, &entries, &insertion_rank);

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
        // No restart runs, so no rebuild-if reinstall is ever recorded.
        rebuild_if_reinstalls: Vec::new(),
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

/// One planned pick against the installed db: `Reinstall` for a rebuild
/// member, `Upgrade`/`Downgrade` across slots, else `New` or `AlreadyInstalled`.
fn bridge_outcome_for(
    cp: &str,
    record: &LoadedVersion,
    version: &str,
    installed: &InstalledView,
    kept: Option<&KeptInstalled>,
) -> (PretendOutcome, Vec<InstalledRef>, bool) {
    let slot_versions = installed.by_slot.get(&(
        record.category.clone(),
        record.package.clone(),
        record.slot.clone(),
    ));
    match slot_versions {
        Some(versions) if versions.iter().any(|v| v == version) => {
            // #34 S2: a same-version pick the bridge asked to rebuild is a
            // bare, reasonless `Reinstall` (precedent `resolve/pretend.rs:170-179`).
            // #34 S4: a USE-drift rebuild carries its changed flags.
            let cpv = format!("{cp}-{version}");
            let outcome = match kept {
                Some(kept) if kept.rebuild.contains(&cpv) => PretendOutcome::Reinstall {
                    version: version.to_string(),
                    changed_flags: kept.use_drift.get(&cpv).cloned().unwrap_or_default(),
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: false,
                },
                _ => PretendOutcome::AlreadyInstalled {
                    version: version.to_string(),
                },
            };
            (outcome, Vec::new(), false)
        }
        Some(versions) => {
            let from = newest_installed(versions).to_string();
            let outcome = match portage_versions::vercmp(version, &from) {
                Some(ordering) if ordering > 0 => PretendOutcome::Upgrade {
                    from: from.clone(),
                    to: version.to_string(),
                },
                _ => PretendOutcome::Downgrade {
                    from: from.clone(),
                    to: version.to_string(),
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
                    version: version.to_string(),
                },
                oldbest,
                occupied,
            )
        }
    }
}

/// USE-display columns for one bridge entry: the flag table plus the
/// `-pv` and `-p` expand forms.
type BridgeUseColumns = (
    Vec<(String, bool)>,
    Vec<(String, String)>,
    Vec<(String, String)>,
);

#[allow(clippy::too_many_arguments)]
fn bridge_use_columns(
    req: &ResolveRequest,
    cp: &str,
    record: &LoadedVersion,
    version: &str,
    use_set: &HashSet<String>,
    outcome: &PretendOutcome,
) -> BridgeUseColumns {
    let use_flags_display: Vec<(String, bool)> = parse_iuse(&record.iuse)
        .into_iter()
        .map(|flag| {
            let on = use_set.contains(&flag);
            (flag, on)
        })
        .collect();
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
        build_id: None,
        category: &record.category,
        package: &record.package,
        config: &req.config,
    });
    // Same `all_flags` split `pretend.rs` uses for `-pv` vs `-p`.
    // #34 S4: an `R` row diffs against the vdb USE like the walk's own
    // display (`InstalledUseState`, `USE` intersected with `IUSE`), so a
    // drifted flag renders `x*`; anything else keeps no markers. Gated
    // on the outcome, so resolvo (which never yields `Reinstall`) and
    // every non-`R` row do not move.
    let (installed_state, reinst_flags): (Option<InstalledUseState>, HashSet<String>) =
        match outcome {
            PretendOutcome::Reinstall { changed_flags, .. } => {
                let old_iuse = read_vdb_flag_set(
                    &req.root,
                    &record.category,
                    &record.package,
                    version,
                    "IUSE",
                );
                let mut old_use =
                    read_vdb_flag_set(&req.root, &record.category, &record.package, version, "USE");
                old_use.retain(|flag| old_iuse.contains(flag));
                (
                    Some(InstalledUseState { old_use, old_iuse }),
                    changed_flags.iter().cloned().collect(),
                )
            }
            _ => (None, HashSet::new()),
        };
    let use_expand_display = super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
        use_flags_display: &use_flags_display,
        config: &req.config,
        installed: installed_state.as_ref(),
        forced: &forced,
        all_flags: true,
        reinst_flags: &reinst_flags,
    });
    let use_expand_display_p = super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
        use_flags_display: &use_flags_display,
        config: &req.config,
        installed: installed_state.as_ref(),
        forced: &forced,
        all_flags: false,
        reinst_flags: &reinst_flags,
    });
    (use_flags_display, use_expand_display, use_expand_display_p)
}

/// Entry `deps` from the raw dep strings plus resolved USE, exactly the
/// walk's own call for a source candidate.
fn bridge_entry_deps(record: &LoadedVersion, use_set: &HashSet<String>) -> Vec<super::DepEdge> {
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
    super::merge_order::dep_edges_from_metadata(
        &dep_metadata,
        use_set,
        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"],
        false,
    )
}

/// Blocker atoms in one record's raw strings, flattened with USE applied
/// and recorded per owner (deduped); the key index recovers buildtime
/// provenance. Portage: `_add_pkg_dep_string` diverts blockers the same way.
fn collect_bridge_blockers(
    record: &LoadedVersion,
    use_set: &HashSet<String>,
    version: &str,
    pending_blockers: &mut Vec<super::PendingBlocker>,
    seen_blockers: &mut HashSet<(String, String, String)>,
) {
    for (key_idx, raw) in record.raw_deps.iter().enumerate() {
        let toks: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
        let Ok(flat) = portage_use_reduce::use_reduce_flat(
            &toks,
            use_set,
            portage_use_reduce::MatchMode::Normal,
        ) else {
            continue;
        };
        for tok in flat {
            let evaluated = portage_dep::evaluate_atom_conditionals(&tok, use_set)
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
                    owner_version: version.to_string(),
                    owner_merging: true,
                    owner_installed: false,
                });
            }
        }
    }
}

/// One ordered `(cp, version)` pick mapped to its `GraphEntry`: `None`
/// when the pick has no record (an unoffered version the engines never
/// emit, or an installed-only pick outside the deep bridge).
#[allow(clippy::too_many_arguments)]
fn bridge_entry_for(
    cp: &str,
    version: &str,
    repo: &LazyRepo,
    installed: &InstalledView,
    kept: Option<&KeptInstalled>,
    req: &ResolveRequest,
    parents: &HashMap<String, Vec<(String, String)>>,
    pending_blockers: &mut Vec<super::PendingBlocker>,
    seen_blockers: &mut HashSet<(String, String, String)>,
) -> Option<GraphEntry> {
    // #34 S3: an installed-only pick has no repo record; map it from the
    // vdb instead of dropping it (its entry renders `AlreadyInstalled`,
    // hidden like any other kept package). Gated on the deep bridge: the
    // stub path never offers these versions without deep, and resolvo
    // (`kept: None`) keeps its skip.
    let record = repo.record(cp, version).or_else(|| {
        if kept.is_some_and(|kept| kept.deep) {
            let (category, package) = cp.split_once('/')?;
            repo.installed_record(category, package, version)
        } else {
            None
        }
    });
    let record = record?;
    let use_set = repo.use_of(&record);
    let (outcome, oldbest, new_slot) = bridge_outcome_for(cp, &record, version, installed, kept);
    let (use_flags_display, use_expand_display, use_expand_display_p) =
        bridge_use_columns(req, cp, &record, version, &use_set, &outcome);
    let cpv = format!("{cp}-{version}");
    let mut required_by: Vec<(String, String)> = parents.get(&cpv).cloned().unwrap_or_default();
    required_by.sort();
    required_by.dedup();
    let deps = bridge_entry_deps(&record, &use_set);
    collect_bridge_blockers(&record, &use_set, version, pending_blockers, seen_blockers);
    Some(GraphEntry {
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
    })
}

/// Blocker reporting via the shared walk-path helper, filed on owner entries.
fn file_bridge_blocker_conflicts(
    req: &ResolveRequest,
    repo: &LazyRepo,
    entries: &mut Vec<GraphEntry>,
    kept: Option<&KeptInstalled>,
    pending_blockers: &[super::PendingBlocker],
) -> Vec<super::OrphanBlocker> {
    // #34 S5 G7: the pubgrub path passes the walk-identical required-set
    // closure (blocker_retry_seed_atoms + request atoms + system, over
    // installed deps) instead of an empty set, so a blocked installed
    // package the world/system cone still needs stays unresolved like
    // Portage's. Resolvo (`kept: None`) keeps the empty set: its output must
    // not move. A satisfied uninstall with an installed parent in the
    // expanded cone flips to unresolved next (Portage's digraph-parents
    // veto, which the solution-member-only entries cannot see).
    let blocker_retry_closure: std::collections::HashSet<(String, String)> = if kept.is_some() {
        let mut seeds = req.config.blocker_retry_seed_atoms.clone();
        seeds.extend(req.atoms.iter().cloned());
        seeds.sort();
        seeds.dedup();
        super::required_set_reachable_cps(&req.root, &seeds, &req.config.system_packages)
    } else {
        // The bridge has no required-set closure of its own.
        std::collections::HashSet::new()
    };
    let mut conflicts = super::resolve_blockers(
        &req.root,
        pending_blockers,
        entries,
        &blocker_retry_closure,
        // Never runs under `--buildpkgonly`, so the gate is inert here.
        false,
    );
    if kept.is_some() {
        unsolvable_with_installed_parent(&req.root, repo, entries, &mut conflicts);
    }
    let mut orphan_blockers = Vec::new();
    super::file_blocker_conflicts(
        entries,
        &req.root,
        conflicts,
        // Orphans ride the same field rather than being dropped.
        &mut orphan_blockers,
    );
    orphan_blockers
}

/// ABI rebuilds via the walk's own fixpoint, extended before the merge-order
/// sort so rebuilds land dependency-first.
fn bridge_slot_operator_rebuilds(
    req: &ResolveRequest,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
) -> (Vec<GraphEntry>, Vec<(String, String)>) {
    let slot_op_reachable: HashSet<(String, String)> = if req.config.complete_seed_atoms.is_empty()
    {
        HashSet::new()
    } else {
        super::required_set_reachable_cps(&req.root, &req.config.complete_seed_atoms, &[])
    };
    if req.ignore_built_slot_operator_deps || !req.rebuild_if_new_slot {
        (Vec::new(), Vec::new())
    } else {
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
            entries,
            reachable: &slot_op_reachable,
            with_bdeps: req.with_bdeps,
            excluded: &req.excluded,
            update: req.update,
            top_level_cps: &top_level_cps,
            top_level_atoms: &top_level_atoms,
            config: &req.config,
        })
    }
}

/// Circular-dep notice: rebuild the walk's hard/soft edge-kind map over the
/// entry `deps` edges and report via `find_hard_cycles`, like the walk.
fn bridge_circular_display(
    req: &ResolveRequest,
    entries: &[GraphEntry],
    insertion_rank: &[usize],
) -> (Vec<Vec<String>>, bool, Vec<String>) {
    let mut edge_kinds: super::EdgeKindMap = HashMap::new();
    for e in entries {
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
        entries,
        &edge_kinds,
        &req.root,
        &HashMap::new(),
        Some(insertion_rank),
    );
    // Cycle report only when a hard cycle was reported.
    let (large_cycle_count, cycle_display) = if circular_deps.is_empty() {
        (false, Vec::new())
    } else {
        let (cycles, display) =
            super::merge_order::cycle_report(entries, &req.atoms, &req.root, &HashMap::new());
        let display = display
            .into_iter()
            .filter_map(|i| super::merge_bound_cpv(&entries[i]))
            .collect();
        (cycles.len() > 3, display)
    };
    (circular_deps, large_cycle_count, cycle_display)
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

/// Slots a bare target atom selects, like Portage's `_greedy_slots`: the slot
/// of the best visible version matching `versions`, plus, under `update`, each
/// installed slot whose own best match is lower. Empty when nothing matches.
// Portage: depgraph.py, _greedy_slots
fn target_slots(
    lazy: &LazyRepo,
    cp: &str,
    versions: &portage_atom_pubgrub::PortageVersionSet,
    installed_slots: &[String],
    update: bool,
) -> Vec<String> {
    let Ok(loaded) = lazy.load_versions(cp) else {
        return Vec::new();
    };
    let mut best: HashMap<&str, portage_solver::Version> = HashMap::new();
    for v in &loaded {
        let Ok(version) = portage_solver::Version::parse(&v.version) else {
            continue;
        };
        if !versions.contains(&version) {
            continue;
        }
        let slot = best
            .entry(v.slot.as_str())
            .or_insert_with(|| version.clone());
        if version > *slot {
            *slot = version;
        }
    }
    let Some((top_slot, top)) = best.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) else {
        return Vec::new();
    };
    let (top_slot, top) = (top_slot.to_string(), top.clone());
    let pairs: Vec<(&str, &portage_solver::Version)> = best.iter().map(|(s, v)| (*s, v)).collect();
    // The slot filter is shared with the default solver's greedy expansion.
    super::greedy_slots_below_best(
        super::GreedySlotsBelowBestArgs {
            top_slot: &top_slot,
            top: &top,
            per_slot_best: &pairs,
            installed_slots,
            update,
        },
        |a, b| a < b,
    )
}

/// Root-target slots for one request atom: a named `:slot` pins one, else S1's
/// Portage `_greedy_slots` choice. Shared by the target list and `KeptInstalled`
/// so the rebuild set follows the exact slot decision the solve runs under.
// Portage: depgraph.py, _greedy_slots
fn root_target_slots(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    installed: &[super::InstalledPackage],
    dep: &portage_solver::Dep,
    versions: &portage_atom_pubgrub::PortageVersionSet,
) -> Vec<String> {
    match target_slot(dep) {
        Some(slot) => vec![slot],
        None => {
            let cp = format!("{}", dep.cpn);
            let installed_slots: Vec<String> = installed
                .iter()
                .filter(|i| format!("{}/{}", i.category, i.package) == cp)
                .map(|i| i.slot.clone())
                .collect();
            target_slots(lazy, &cp, versions, &installed_slots, req.update)
        }
    }
}

/// One dep class with direct self-cycle `||` alternatives removed: a `||`
/// branch naming the candidate's own cp-slot is dropped unless an installed
/// instance satisfies it (Portage then builds against the installed self, no
/// cycle). A self atom outside `||`, in `^^`/`??`, or alone in its group
/// stays, as does anything a bare `=`/`*` slot operator pins.
// Portage: depgraph.py:778-793 (graph-added packages preferred, with the
// gwydion-dylan self-or-bin precedent), dep_check circular handling
fn without_self_alternatives(
    cp: &str,
    slot: &str,
    installed: &[super::InstalledPackage],
    classes: &[Vec<portage_solver::DepEntry>; 5],
) -> [Vec<portage_solver::DepEntry>; 5] {
    std::array::from_fn(|i| strip_self_from_entries(cp, slot, installed, &classes[i]))
}

/// The [`without_self_alternatives`] rule over one entry tree.
fn strip_self_from_entries(
    cp: &str,
    slot: &str,
    installed: &[super::InstalledPackage],
    entries: &[portage_solver::DepEntry],
) -> Vec<portage_solver::DepEntry> {
    use portage_solver::DepEntry;
    entries
        .iter()
        .map(|entry| match entry {
            DepEntry::AnyOf(children) => {
                let survivors: Vec<DepEntry> = children
                    .iter()
                    .filter(|child| !is_droppable_self(child, cp, slot, installed))
                    .map(|child| strip_self_from_entry(cp, slot, installed, child))
                    .collect();
                if survivors.is_empty() {
                    entry.clone()
                } else {
                    DepEntry::AnyOf(survivors)
                }
            }
            DepEntry::AllOf(children) => {
                DepEntry::AllOf(strip_self_from_entries(cp, slot, installed, children))
            }
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => DepEntry::UseConditional {
                flag: *flag,
                negate: *negate,
                children: strip_self_from_entries(cp, slot, installed, children),
            },
            _ => entry.clone(),
        })
        .collect()
}

/// The [`without_self_alternatives`] rule over one entry (for `||` children
/// that are groups themselves, e.g. behind a USE conditional).
fn strip_self_from_entry(
    cp: &str,
    slot: &str,
    installed: &[super::InstalledPackage],
    entry: &portage_solver::DepEntry,
) -> portage_solver::DepEntry {
    strip_self_from_entries(cp, slot, installed, std::slice::from_ref(entry))
        .into_iter()
        .next()
        .expect("a one-entry strip keeps one entry")
}

/// Whether one `||` branch is the candidate's own cp-slot with no installed
/// instance satisfying it: selecting it would build the candidate from
/// itself. Blockers are not build edges and never count.
fn is_droppable_self(
    entry: &portage_solver::DepEntry,
    cp: &str,
    slot: &str,
    installed: &[super::InstalledPackage],
) -> bool {
    let portage_solver::DepEntry::Atom(dep) = entry else {
        return false;
    };
    if dep.blocker.is_some() || format!("{}", dep.cpn) != cp {
        return false;
    }
    if !names_own_slot(dep, slot) {
        return false;
    }
    !installed_satisfies_self(dep, cp, installed)
}

/// Whether the atom's `:slot` part names `slot`: unpinned always does; a bare
/// `=`/`*` operator never does (kept conservatively); a named pin must equal
/// it. Same reading as [`target_slot`], applied to a candidate slot.
fn names_own_slot(dep: &portage_solver::Dep, slot: &str) -> bool {
    if dep.slot_dep.is_none() {
        return true;
    }
    target_slot(dep).is_some_and(|pin| pin == slot)
}

/// Whether some installed instance of `cp` satisfies the self atom, so
/// Portage could build against it without a cycle: same slot reading as the
/// drop rule, plus the version floor when the atom carries one. An
/// unparsable installed version counts as satisfying (keeps old behaviour).
fn installed_satisfies_self(
    dep: &portage_solver::Dep,
    cp: &str,
    installed: &[super::InstalledPackage],
) -> bool {
    installed.iter().any(|inst| {
        if format!("{}/{}", inst.category, inst.package) != cp {
            return false;
        }
        if dep.slot_dep.is_some() && target_slot(dep).is_some_and(|pin| pin != inst.slot) {
            return false;
        }
        match (&dep.op, &dep.version) {
            (Some(op), Some(version)) => {
                let Ok(installed_version) = portage_solver::Version::parse(&inst.version) else {
                    return true;
                };
                portage_atom_pubgrub::PortageVersionSet::from_operator(
                    *op,
                    dep.glob,
                    version.clone(),
                )
                .contains(&installed_version)
            }
            _ => true,
        }
    })
}

/// A satisfied-by-uninstall conflict the bridge must report unresolved instead:
/// the removal target is installed, nothing this run merges in its cp, and an
/// installed package the request expansion reached still depends on it. Portage's
/// `_validate_blockers` never unmerges a blocked package pulled into the
/// graph with parents; the bridge's entries only hold solution members, so an
/// installed parent past the kept leaves (installed systemd behind kept
/// util-linux) is invisible to `graph_has_parent`. The edge is read from the
/// vdb with the built USE, like [`super::required_set_reachable_cps`].
// Portage: depgraph.py, _validate_blockers (the digraph-contains-and-parents
// veto on the uninstall scheduling)
fn unsolvable_with_installed_parent(
    root: &std::path::Path,
    repo: &LazyRepo,
    entries: &[super::GraphEntry],
    conflicts: &mut [super::FiledBlocker],
) {
    use super::{BlockerSatisfiedBy, merge_bound_version};
    for conflict in conflicts.iter_mut() {
        let Some(BlockerSatisfiedBy::Uninstall { cpv, .. }) = &conflict.conflict.satisfied_by
        else {
            continue;
        };
        let Some((category, package, version)) = super::split_cpv(cpv) else {
            continue;
        };
        // A merge-bound entry for the same cp replaces rather than removes;
        // that Replacement row already stands.
        if entries.iter().any(|e| {
            e.category == category
                && e.package == package
                && merge_bound_version(&e.outcome).is_some()
        }) {
            continue;
        }
        if !has_installed_parent(root, repo, &category, &package, &version) {
            continue;
        }
        conflict.conflict.unsolvable = true;
        conflict.conflict.satisfied_by = None;
    }
}

/// Whether an installed package other than `category/package` itself, reached
/// by the request expansion, has a USE-evaluated vdb dep edge onto the
/// `category/package-version` instance. Blocker atoms are not parents.
fn has_installed_parent(
    root: &std::path::Path,
    repo: &LazyRepo,
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let (slot, sub_slot) = super::read_vdb_slot(root, category, package, version);
    let candidate = format!("{category}/{package}-{version}:{slot}/{sub_slot}");
    super::all_installed_packages(root)
        .iter()
        .filter(|inst| {
            (inst.category.as_str(), inst.package.as_str()) != (category, package)
                && repo.expanded_contains(&inst.category, &inst.package)
        })
        .any(|inst| {
            let use_flags =
                super::read_vdb_flag_set(root, &inst.category, &inst.package, &inst.version, "USE");
            ["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
                .iter()
                .any(|key| {
                    let depstr = super::read_vdb_string(
                        root,
                        &inst.category,
                        &inst.package,
                        &inst.version,
                        key,
                    );
                    let Some(atoms) = super::flat_dep_atoms(&depstr, &use_flags) else {
                        return false;
                    };
                    atoms.iter().any(|atom_str| {
                        let Some(atom) = portage_dep::parse_atom(atom_str) else {
                            return false;
                        };
                        if atom.blocker != portage_dep::Blocker::None
                            || atom.category != category
                            || atom.package != package
                        {
                            return false;
                        }
                        portage_dep::match_from_list(atom_str, &[candidate.as_str()])
                            .is_some_and(|matched| !matched.is_empty())
                    })
                })
        })
}

/// Referenced CPNs of parsed dep entries under a USE set: conditional
/// branches the flags switch off contribute nothing (the engine evaluates
/// the same way), every choice-group branch is followed (the closure's
/// over-approximation: branch choice needs the solved plan), and blockers
/// are skipped (queued separately, never expanded).
fn refs_under_use(
    entries: &[portage_solver::DepEntry],
    use_set: &HashSet<String>,
    out: &mut Vec<String>,
) {
    for entry in entries {
        match entry {
            portage_solver::DepEntry::Atom(dep) => {
                if dep.blocker.is_none() {
                    out.push(format!("{}", dep.cpn));
                }
            }
            portage_solver::DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => {
                if use_set.contains(flag.as_str()) != *negate {
                    refs_under_use(children, use_set, out);
                }
            }
            portage_solver::DepEntry::AllOf(children)
            | portage_solver::DepEntry::AnyOf(children)
            | portage_solver::DepEntry::ExactlyOneOf(children)
            | portage_solver::DepEntry::AtMostOneOf(children) => {
                refs_under_use(children, use_set, out);
            }
        }
    }
}

/// Bounded-`--deep` expansion depths: min dep depth per `cat/pkg` from the
/// top atoms (depth 0). Installed CPNs contribute their vdb deps under the
/// vdb USE, repo-visible CPNs their repo deps under the config USE (the two
/// USE sources [`BridgeRepo::desired_use`] answers the engine with);
/// DEPEND/BDEPEND ride only with `with_bdeps`, like the walk. Returns
/// `(bound, depths)`; `None` unless `Deep::Bounded(n)` without
/// `--emptytree` (which re-resolves everything, unbounded).
///
/// The PubGrub engine cannot carry depth, so this pre-solve BFS is the
/// kept-expansion filter: [`KeptInstalled::deep_expands`] admits an
/// installed cpv only below the bound. Approximations: choice-group
/// branches all count (branch choice needs the solve), and a cp that is
/// both installed and repo-visible contributes both readers' strings (an
/// in-bound upgrade re-merges fresh).
// Portage: depgraph.py:7359-7375 (`_too_deep`)
fn deep_bounded_depths(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    installed: &[super::InstalledPackage],
) -> Option<(u32, HashMap<String, u32>)> {
    let Deep::Bounded(n) = req.deep else {
        return None;
    };
    if req.empty {
        return None;
    }
    let mut installed_cps: HashSet<String> = HashSet::new();
    for inst in installed {
        installed_cps.insert(format!("{}/{}", inst.category, inst.package));
    }
    let mut depths: HashMap<String, u32> = HashMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for atom in &req.atoms {
        let Ok(dep) = portage_solver::Dep::parse(atom) else {
            continue;
        };
        if dep.blocker.is_some() {
            continue;
        }
        let cp = format!("{}", dep.cpn);
        if depths.insert(cp.clone(), 0).is_none() {
            queue.push_back(cp);
        }
    }
    while let Some(cp) = queue.pop_front() {
        let depth = depths[&cp];
        if depth >= n {
            continue;
        }
        let mut refs = Vec::new();
        if let Some((category, package)) = cp.split_once('/')
            && installed_cps.contains(&cp)
        {
            for inst in installed
                .iter()
                .filter(|inst| inst.category == category && inst.package == package)
            {
                let (_, use_set) = installed_pkg_iuse_and_use(
                    &req.root,
                    &inst.category,
                    &inst.package,
                    &inst.version,
                );
                for (class, texts) in lazy
                    .installed_parsed_deps(&inst.category, &inst.package, &inst.version)
                    .iter()
                    .enumerate()
                {
                    // DEPEND/BDEPEND (classes 0/2) ride only with bdeps, like
                    // the walk's installed-deps enqueue.
                    if (class == 0 || class == 2) && !req.with_bdeps {
                        continue;
                    }
                    refs_under_use(texts, &use_set, &mut refs);
                }
            }
        }
        if let Ok(loaded) = lazy.load_versions(&cp) {
            for record in &loaded {
                let use_set = lazy.use_of(record);
                for (class, texts) in record.deps.iter().enumerate() {
                    if (class == 0 || class == 2) && !req.with_bdeps {
                        continue;
                    }
                    refs_under_use(texts, &use_set, &mut refs);
                }
            }
        }
        for dep_cp in refs {
            if depths.contains_key(&dep_cp) {
                continue;
            }
            if portage_solver::Cpn::parse(&dep_cp).is_err() || !cp_exists(&lazy.repos, &dep_cp) {
                continue;
            }
            depths.insert(dep_cp.clone(), depth + 1);
            if depth + 1 < n {
                queue.push_back(dep_cp);
            }
        }
    }
    Some((n, depths))
}

/// Installed cpvs the bridge describes to the engine, computed before the provider
/// exists (deps are converted inside `new_for_targets`, provider/mod.rs:443-560).
#[derive(Clone)]
struct KeptInstalled {
    /// Every installed `category/package-version`.
    installed: HashSet<String>,
    /// Installed cpvs registered `InstalledPolicy::Rebuild`, rendered `R`.
    rebuild: HashSet<String>,
    /// Installed cpvs the engine must not keep: no visible same-slot version
    /// is `>=` the installed one, so Portage re-selects the best visible
    /// version (`UD`) instead of keeping the installed instance.
    downgrade: HashSet<String>,
    /// Changed USE flags per rebuilt cpv (`--newuse`/`--changed-use` drift),
    /// carried into the `R` row's `Reinstall.changed_flags`.
    use_drift: HashMap<String, Vec<String>>,
    /// Deep expansion requested (`--deep` or `--emptytree`): no leaf rule.
    deep: bool,
    /// `--deep=N` expansion allowlist: installed cpvs whose min dep depth
    /// from the top atoms is `< N` (Portage `_too_deep`). `None` for
    /// unlimited deep/emptytree (every installed cpv expands) and without
    /// deep (none does); see [`deep_bounded_depths`].
    deep_allowed: Option<HashSet<String>>,
    /// Build-time deps are traversed (`--with-bdeps`, on unless `--usepkg`
    /// or `--with-bdeps=n`): gates the kept-buildtime fold below.
    with_bdeps: bool,
}

impl KeptInstalled {
    /// Whether a kept (installed, non-rebuild) `cpv`'s deps reach the
    /// engine: unlimited/emptytree deep always expands, `--deep=N` only
    /// inside the bounded pre-solve BFS set. Fresh versions and rebuild
    /// members always expand ([`KeptInstalled::expands`]).
    // Portage: depgraph.py:7359-7375 (`_too_deep`)
    fn deep_expands(&self, cpv: &str) -> bool {
        if !self.deep {
            return false;
        }
        match &self.deep_allowed {
            None => true,
            Some(allowed) => allowed.contains(cpv),
        }
    }

    /// The full kept-leaf rule: fresh versions and rebuild members always
    /// expand; kept installed versions only when
    /// [`KeptInstalled::deep_expands`].
    // Portage: depgraph.py:3815-3824 (`_ignored_deps`), :7359-7375 (`_too_deep`)
    fn expands(&self, cpv: &str) -> bool {
        if !self.installed.contains(cpv) || self.rebuild.contains(cpv) {
            return true;
        }
        self.deep_expands(cpv)
    }
}

impl KeptInstalled {
    /// The kept/rebuild set for one request: every installed cpv when `empty`;
    /// else the installed cpv that is the best accepted version of a root target
    /// (after S1's slot choice) when the resolve is not selective. A kept named
    /// argument under `-pvu` is a leaf too: Portage sends the deps of an installed
    /// node selected at depth 0 without deep to `_ignored_deps`.
    // Portage: depgraph.py:3815-3824 (`_ignored_deps`), :7359-7375 (`_too_deep`)
    fn of(req: &ResolveRequest, lazy: &LazyRepo, installed: &[super::InstalledPackage]) -> Self {
        let mut all = HashSet::new();
        for inst in installed {
            all.insert(inst.cpv());
        }
        let deep = !matches!(req.deep, Deep::NotRequested) || req.empty;
        let mut rebuild = HashSet::new();
        if req.empty {
            rebuild.extend(all.iter().cloned());
        } else if !(req.selective && !req.empty) {
            for atom in &req.atoms {
                let Ok(dep) = portage_solver::Dep::parse(atom) else {
                    continue;
                };
                // Top-level blockers are an unmerge action, not a merge target.
                if dep.blocker.is_some() {
                    continue;
                }
                let versions = match (&dep.op, &dep.version) {
                    (Some(op), Some(version)) => {
                        portage_atom_pubgrub::PortageVersionSet::from_operator(
                            *op,
                            dep.glob,
                            version.clone(),
                        )
                    }
                    _ => portage_atom_pubgrub::PortageVersionSet::any(),
                };
                let cp = format!("{}", dep.cpn);
                let Ok(loaded) = lazy.load_versions(&cp) else {
                    continue;
                };
                for slot in root_target_slots(req, lazy, installed, &dep, &versions) {
                    let mut best: Option<portage_solver::Version> = None;
                    for record in &loaded {
                        if record.slot != slot {
                            continue;
                        }
                        let Ok(version) = portage_solver::Version::parse(&record.version) else {
                            continue;
                        };
                        if !versions.contains(&version) {
                            continue;
                        }
                        if best.as_ref().is_none_or(|top| version > *top) {
                            best = Some(version);
                        }
                    }
                    if let Some(best) = best {
                        let cpv = format!("{cp}-{best}");
                        if all.contains(&cpv) {
                            rebuild.insert(cpv);
                        }
                    }
                }
            }
        }
        // #34 S4: USE-drift `Rebuild`. Under `--newuse`/`--changed-use` an
        // installed cpv whose effective USE drifted from the vdb rebuilds
        // (Portage's `R` with the changed flags), so it joins `rebuild`
        // before the provider is built. The adapter re-lists the repo
        // candidate for that cpv (`LoadedVersion` drops it); an installed
        // version gone from the repo has no candidate to compare, so it
        // stays out. `--changed-deps`, `--changed-slot` and `--newrepo`
        // triggers are out of scope (no corpus mode sets them).
        // Portage: depgraph.py, _reinstall_for_flags.
        let mut use_drift = HashMap::new();
        if req.newuse || req.changed_use {
            for inst in installed {
                let cpv = inst.cpv();
                if rebuild.contains(&cpv) {
                    continue;
                }
                let Ok(candidates) = list_candidates(&lazy.repos, &inst.category, &inst.package)
                else {
                    continue;
                };
                let Some(candidate) = candidates.iter().find(|c| c.version == inst.version) else {
                    continue;
                };
                if let Some(flags) =
                    reinstall_flags_for_use_change(ReinstallFlagsForUseChangeArgs {
                        root: &req.root,
                        category: &inst.category,
                        package: &inst.package,
                        candidate,
                        config: &req.config,
                        newuse: req.newuse,
                    })
                {
                    rebuild.insert(cpv.clone());
                    use_drift.insert(cpv, flags);
                }
            }
        }
        // #432: bounded `--deep=N` expansion depths: the kept-expansion
        // allowlist (min depth `< N`) and the update-walk reach (min depth
        // `<= N`, Portage `want_update`). Empty without a bound.
        let bounded = deep_bounded_depths(req, lazy, installed);
        let deep_allowed: Option<HashSet<String>> = bounded.as_ref().map(|(n, depths)| {
            installed
                .iter()
                .filter(|inst| {
                    depths
                        .get(&format!("{}/{}", inst.category, inst.package))
                        .is_some_and(|depth| *depth < *n)
                })
                .map(|inst| inst.cpv())
                .collect()
        });
        let bounded_reached: HashSet<String> = bounded
            .as_ref()
            .map(|(n, depths)| {
                depths
                    .iter()
                    .filter(|(_, depth)| **depth <= *n)
                    .map(|(cp, _)| cp.clone())
                    .collect()
            })
            .unwrap_or_default();
        // #34 S5 G1: hoisted so the G2 pass below can skip children Portage
        // re-selects away from (their version changes; a USE atom against
        // the installed version no longer applies).
        let downgrade = Self::downgrade_set(req, lazy, installed, deep, &bounded_reached);
        Self {
            installed: all,
            rebuild,
            use_drift,
            deep,
            deep_allowed,
            downgrade,
            with_bdeps: req.with_bdeps,
        }
    }

    /// #34 S5 G2 (round 2): parent-driven USE reinstall, applied
    /// post-solve by `resolve_pubgrub`'s fixpoint. A USE-dep atom in a
    /// dependency of a *solution member* whose deps expand (fresh members
    /// and rebuild members; under deep also kept installed members, whose
    /// vdb deps Portage expands) that the installed instance's USE does
    /// not satisfy, while the same-version repo ebuild reaches the
    /// required flags under the current config, makes that installed cpv
    /// a rebuild member (`InstalledPolicy::Rebuild`, an `R` row with the
    /// changed flags, its deps expanded). Only unions into
    /// `rebuild`/`use_drift` (never overwrites S4's); skipped under
    /// `--nodeps` (deps never expand, so no parent USE pressure exists).
    /// Portage: `_select_atoms`/`dep_check` re-evaluation, which only ever
    /// evaluates atoms of graph members (`depgraph.py:6030-6110` region),
    /// and `_ignored_deps` (`:3815-3824`).
    fn apply_parent_use_reinstalls(
        &mut self,
        req: &ResolveRequest,
        lazy: &LazyRepo,
        installed: &[super::InstalledPackage],
        plan: &SolvedPlan,
    ) {
        if req.nodeps {
            return;
        }
        for (cpv, flags) in parent_use_reinstalls(req, lazy, installed, self, plan) {
            self.rebuild.insert(cpv.clone());
            let entry = self.use_drift.entry(cpv).or_default();
            for flag in flags {
                if !entry.contains(&flag) {
                    entry.push(flag);
                }
            }
            entry.sort();
        }
    }

    /// Installed cpvs with no visible same-slot version `>=` the installed one
    /// that Portage would re-select away from: a named target (`arg_atoms`),
    /// under emptytree (complete mode re-resolves everything from the repo),
    /// or under `--update` with deep (the deep walk reaches the node with
    /// `want_update` true). Without update/deep Portage keeps the installed
    /// instance (`_want_update_pkg` false → the installed early return), so
    /// the set stays empty there and registration/offering are unchanged.
    /// Under `--deep=N` with update only the walk's reach (min depth `<= N`,
    /// [`deep_bounded_depths`]) probes; unlimited deep keeps the historical
    /// whole-installed-world probe.
    /// Portage: depgraph.py:2946-2957 (`_downgrade_probe`), :3805-3809
    /// (`want_update`), :7304-7333 (`_want_update_pkg`).
    fn downgrade_set(
        req: &ResolveRequest,
        lazy: &LazyRepo,
        installed: &[super::InstalledPackage],
        deep: bool,
        bounded_reached: &HashSet<String>,
    ) -> HashSet<String> {
        let mut named = HashSet::new();
        for atom in &req.atoms {
            let Ok(dep) = portage_solver::Dep::parse(atom) else {
                continue;
            };
            // Top-level blockers are an unmerge action, not a merge target.
            if dep.blocker.is_some() {
                continue;
            }
            named.insert(format!("{}", dep.cpn));
        }
        let update_walk = req.empty || (req.update && deep);
        let update_walk_all = update_walk && !matches!(req.deep, Deep::Bounded(_));
        let mut downgrade = HashSet::new();
        for inst in installed {
            let cp = format!("{}/{}", inst.category, inst.package);
            if !named.contains(&cp)
                && !update_walk_all
                && !(req.update && update_walk && bounded_reached.contains(&cp))
            {
                continue;
            }
            let Ok(installed_ver) = portage_solver::Version::parse(&inst.version) else {
                continue;
            };
            let Ok(loaded) = lazy.load_versions(&cp) else {
                continue;
            };
            let mut any_visible = false;
            let mut newer_or_equal = false;
            for record in &loaded {
                if record.slot != inst.slot {
                    continue;
                }
                let Ok(visible) = portage_solver::Version::parse(&record.version) else {
                    continue;
                };
                any_visible = true;
                if visible >= installed_ver {
                    newer_or_equal = true;
                    break;
                }
            }
            // With no visible version at all there is nothing to re-select
            // to, so the installed instance stays (like `_downgrade_probe`
            // returning false when nothing is available).
            if any_visible && !newer_or_equal {
                downgrade.insert(inst.cpv());
            }
        }
        downgrade
    }
}

/// #34 S5 G2 (round 2): installed `(cpv, changed flags)` pairs a planned
/// parent reinstalls through a USE-dep atom (`dev-libs/pg34-clib[f]`
/// required by `pg34-capp` while installed `clib-1` was built `USE=""`):
/// the installed instance's USE does not satisfy the atom, but the
/// same-version repo ebuild reaches the required flags under the current
/// config, so Portage re-selects the repo ebuild (`R` with the changed
/// flags) and expands its deps. When the repo ebuild cannot satisfy the
/// atom either (a default-off flag with no `package.use` line), Portage
/// stops at the autounmask gate instead (rc 1, triage G4a) and no pair is
/// reported.
///
/// `plan` is the solved plan: only atoms from solution members whose deps
/// expand are evaluated, because Portage only re-evaluates atoms of graph
/// members. A USE atom carried by a package the solve never selects (an
/// unselected version, a reachable-but-unmerged cp such as
/// `dev-qt/qtwebengine` for an `app-editors/vim` probe) must not force a
/// reinstall: round 1 evaluated every reachable cp's strings pre-solve and
/// rebuilt `libxml2`/`systemd`/`dbus`/`xmlto`/`curl` on dozens of unrelated
/// probes (bed delta +1208 extras in `-pv`). Fresh members contribute the
/// solved version's strings (the picked version is known post-solve);
/// installed members only when Portage would expand them -- rebuilt
/// members with the config USE (repo strings), kept ones only under deep
/// with the vdb strings/USE (`installed_parsed_deps`,
/// `installed_pkg_iuse_and_use`), skipped as kept leaves without deep
/// (`_ignored_deps`, `depgraph.py:3815-3824`) -- with the same USE source
/// `desired_use` answers them with. `AllOf` groups and satisfied `flag?`
/// conditionals are transparent; `||`/`^^`/`??` choice groups resolve to
/// the taken branch (alternatives the solve edged from the parent, e.g.
/// `virtual/minizip`'s `|| ( zlib[minizip] minizip-ng )` taking installed
/// `zlib`), so a USE atom on an untaken branch (triage G5) never forces a
/// reinstall while a taken one does. A downgraded parent's version changes
/// out from under its strings, so it is skipped.
/// Portage: `depgraph.py:6030-6110` region (`_select_atoms`/`dep_check`
/// re-evaluation), `:3815-3824` (`_ignored_deps`).
/// The solved plan's shape [`parent_use_reinstalls`] evaluates USE-dep
/// atoms against: which `(cp, version)` pairs the solve selected, and
/// which `(parent cp, child cp)` edges it recorded (choice-group branch
/// resolution: an alternative the solve edged from the parent is taken).
struct SolvedPlan {
    members: HashSet<(String, String)>,
    taken: HashSet<(String, String)>,
}

/// Invert one [`resolve_pubgrub`] round's `parents` map
/// (`child cpv -> [(requiring category, package)]`) into the
/// `(parent cp, child cp)` edge set [`SolvedPlan::taken`] records.
fn solution_taken_edges(
    parents: &HashMap<String, Vec<(String, String)>>,
) -> HashSet<(String, String)> {
    let mut taken = HashSet::new();
    for (child_cpv, froms) in parents {
        let Ok(child) = portage_solver::Cpv::parse(child_cpv) else {
            continue;
        };
        let child_cp = format!("{}", child.cpn);
        for (category, package) in froms {
            taken.insert((format!("{category}/{package}"), child_cp.clone()));
        }
    }
    taken
}

fn parent_use_reinstalls(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    installed: &[super::InstalledPackage],
    kept: &KeptInstalled,
    plan: &SolvedPlan,
) -> Vec<(String, Vec<String>)> {
    // Under emptytree every installed cpv already rebuilds; with nothing
    // installed there is no child to reinstall.
    if req.empty || installed.is_empty() {
        return Vec::new();
    }
    // Installed instances by cp (a cp may hold several installed slots).
    let mut by_cp: HashMap<String, Vec<&super::InstalledPackage>> = HashMap::new();
    for inst in installed {
        by_cp
            .entry(format!("{}/{}", inst.category, inst.package))
            .or_default()
            .push(inst);
    }
    // USE-dep atoms grouped by target cp, with the requiring parent's
    // effective USE (for the `?`/`=` kinds, which read the parent's flags).
    let mut wanted: HashMap<String, Vec<(portage_solver::Dep, HashSet<String>)>> = HashMap::new();
    let mut push_entries =
        |entries: &[portage_solver::DepEntry], parent_use: &HashSet<String>, parent_cp: &str| {
            let mut stack: Vec<&portage_solver::DepEntry> = entries.iter().collect();
            while let Some(entry) = stack.pop() {
                match entry {
                    portage_solver::DepEntry::Atom(dep) => {
                        if dep.blocker.is_none()
                            && dep.use_deps.as_ref().is_some_and(|uds| !uds.is_empty())
                        {
                            wanted
                                .entry(format!("{}", dep.cpn))
                                .or_default()
                                .push((dep.clone(), parent_use.clone()));
                        }
                    }
                    portage_solver::DepEntry::UseConditional {
                        flag,
                        negate,
                        children,
                    } => {
                        if parent_use.contains(flag.as_str()) != *negate {
                            stack.extend(children.iter());
                        }
                    }
                    portage_solver::DepEntry::AllOf(children) => stack.extend(children.iter()),
                    // Choice groups resolve to the taken branch: an
                    // alternative the solve edged from this parent counts (a
                    // USE atom there is really evaluated), an untaken one
                    // stays out (triage G5). See the fn doc.
                    portage_solver::DepEntry::AnyOf(children)
                    | portage_solver::DepEntry::ExactlyOneOf(children)
                    | portage_solver::DepEntry::AtMostOneOf(children) => {
                        for child in children {
                            let mut refs = Vec::new();
                            referenced_cpns(std::slice::from_ref(child), &mut refs);
                            if refs
                                .iter()
                                .any(|cp| plan.taken.contains(&(parent_cp.to_string(), cp.clone())))
                            {
                                stack.push(child);
                            }
                        }
                    }
                }
            }
        };
    // Member-driven parent scan: only solution members whose deps
    // expand contribute USE-dep atoms. Sorted for a stable visit order
    // (`wanted`/`out` downstream are order-insensitive).
    let mut member_list: Vec<&(String, String)> = plan.members.iter().collect();
    member_list.sort();
    for (cp, version) in member_list {
        // Installed member: the version Portage expands -- vdb strings
        // under deep (like `versions_for`, including versions gone from
        // the repo), repo strings otherwise -- with the USE source
        // `desired_use` answers (config USE for rebuild members, vdb USE
        // for kept ones). A downgraded instance's version changes out
        // from under its strings, so it is skipped here (its solved
        // version scans through the fresh branch below); a kept leaf's
        // deps go to `_ignored_deps` without deep, so it contributes
        // nothing.
        if let Some(inst) = by_cp
            .get(cp)
            .and_then(|cands| cands.iter().find(|i| i.version == *version))
        {
            let cpv = inst.cpv();
            if kept.downgrade.contains(&cpv) {
                continue;
            }
            let is_rebuild = kept.rebuild.contains(&cpv);
            // A kept leaf's deps go to `_ignored_deps` without deep -- and
            // past the bound under `--deep=N` -- so it contributes nothing.
            if !is_rebuild && !kept.deep_expands(&cpv) {
                continue;
            }
            let Ok(loaded) = lazy.load_versions(cp) else {
                continue;
            };
            let repo_record = loaded
                .iter()
                .find(|r| r.version == inst.version && r.slot == inst.slot);
            if kept.deep {
                let parsed =
                    lazy.installed_parsed_deps(&inst.category, &inst.package, &inst.version);
                let vdb_use = installed_pkg_iuse_and_use(
                    &lazy.root,
                    &inst.category,
                    &inst.package,
                    &inst.version,
                )
                .1;
                let parent_use = if is_rebuild {
                    match repo_record {
                        Some(record) => lazy.use_of(record),
                        None => {
                            match lazy.installed_record(
                                &inst.category,
                                &inst.package,
                                &inst.version,
                            ) {
                                Some(record) => lazy.use_of(&record),
                                None => vdb_use.clone(),
                            }
                        }
                    }
                } else {
                    vdb_use
                };
                for classes in &parsed {
                    push_entries(
                        classes,
                        &parent_use,
                        &format!("{}/{}", inst.category, inst.package),
                    );
                }
            } else if let Some(record) = repo_record {
                let parent_use = lazy.use_of(record);
                for classes in &record.deps {
                    push_entries(
                        classes,
                        &parent_use,
                        &format!("{}/{}", inst.category, inst.package),
                    );
                }
            }
            continue;
        }
        // Fresh member (or a re-selected one, e.g. a G1 downgrade): the
        // solved version's repo strings with the config USE. A solved
        // version with no record cannot happen (offered versions come
        // from these records); skip rather than fall back to every
        // version's strings.
        let Ok(loaded) = lazy.load_versions(cp) else {
            continue;
        };
        let Some(record) = loaded.iter().find(|r| r.version == *version) else {
            continue;
        };
        // Defensive: a member version that is also installed expands from
        // the vdb above, never here.
        if by_cp
            .get(cp)
            .is_some_and(|cands| cands.iter().any(|i| i.cpv() == record.cpv()))
        {
            continue;
        }
        let parent_use = lazy.use_of(record);
        for classes in &record.deps {
            push_entries(classes, &parent_use, cp);
        }
    }
    // One load per target cp; union the changed flags across parents.
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (cp, atoms) in &wanted {
        let Some(cands) = by_cp.get(cp) else {
            continue;
        };
        let Ok(loaded) = lazy.load_versions(cp) else {
            continue;
        };
        for (atom, parent_use) in atoms {
            let Some(uds) = atom.use_deps.as_ref() else {
                continue;
            };
            for inst in cands {
                let child_cpv = inst.cpv();
                if kept.rebuild.contains(&child_cpv) || kept.downgrade.contains(&child_cpv) {
                    continue;
                }
                let Ok(cpv) = portage_solver::Cpv::parse(&child_cpv) else {
                    continue;
                };
                // Named-slot and version restrictions still bind: an atom
                // the installed version does not match is an upgrade
                // question, not a parent-USE reinstall.
                if !atom.matches_cpv(&cpv, Some(inst.slot.as_str())) {
                    continue;
                }
                let (vdb_iuse, vdb_use) = installed_pkg_iuse_and_use(
                    &lazy.root,
                    &inst.category,
                    &inst.package,
                    &inst.version,
                );
                if use_deps_satisfied(uds, parent_use, &vdb_iuse, &vdb_use) {
                    continue;
                }
                // The same-version repo ebuild under the current config.
                let Some(record) = loaded
                    .iter()
                    .find(|r| r.version == inst.version && r.slot == inst.slot)
                else {
                    continue;
                };
                let effective = lazy.use_of(record);
                let repo_iuse: HashSet<String> = parse_iuse(&record.iuse).into_iter().collect();
                if !use_deps_satisfied(uds, parent_use, &repo_iuse, &effective) {
                    continue;
                }
                let flags = changed_use_dep_flags(uds, parent_use, &vdb_use, &effective);
                let entry = out.entry(child_cpv).or_default();
                for flag in flags {
                    if !entry.contains(&flag) {
                        entry.push(flag);
                    }
                }
            }
        }
    }
    let mut out: Vec<(String, Vec<String>)> = out.into_iter().collect();
    for (_, flags) in &mut out {
        flags.sort();
    }
    out
}

/// Whether a USE-dep list holds for `child_use` (with `child_iuse` for the
/// `(+/-)` fallback when the flag is undefined), given the requiring
/// parent's effective USE. Mirrors the engine's own check
/// (`portage-atom-pubgrub-0.8.0/src/validate.rs`, `resolve_flag_state`
/// plus the per-kind match), including its PMS 8.2.6.4 `[!flag?]`
/// reading (parent off ⇒ the target's flag must be *disabled*).
fn use_deps_satisfied(
    uds: &[portage_atom::UseDep],
    parent_use: &HashSet<String>,
    child_iuse: &HashSet<String>,
    child_use: &HashSet<String>,
) -> bool {
    use portage_atom::UseDepKind;
    uds.iter().all(|ud| {
        let flag = ud.flag.as_str();
        // Portage: `vardb.match` falls back to the `(+/-)` default only
        // when the flag is not in IUSE (`installed_pkg_iuse_and_use` doc).
        let target_on = if child_iuse.contains(flag) {
            child_use.contains(flag)
        } else {
            ud.default == Some(portage_atom::UseDefault::Enabled)
        };
        let parent_on = parent_use.contains(flag);
        match ud.kind {
            UseDepKind::Enabled => target_on,
            UseDepKind::Disabled => !target_on,
            UseDepKind::Conditional => {
                if parent_on {
                    target_on
                } else {
                    true
                }
            }
            UseDepKind::ConditionalInverse => {
                if parent_on {
                    true
                } else {
                    !target_on
                }
            }
            UseDepKind::Equal => target_on == parent_on,
            UseDepKind::EqualInverse => target_on != parent_on,
        }
    })
}

/// The required flags that flip between the installed USE and the repo
/// ebuild's effective USE, for the `R` row's `*` paint (same
/// `use_drift` channel S4 fills).
fn changed_use_dep_flags(
    uds: &[portage_atom::UseDep],
    parent_use: &HashSet<String>,
    vdb_use: &HashSet<String>,
    effective_use: &HashSet<String>,
) -> Vec<String> {
    use portage_atom::UseDepKind;
    let mut flags = Vec::new();
    for ud in uds {
        let flag = ud.flag.as_str();
        let vdb_on = vdb_use.contains(flag);
        let eff_on = effective_use.contains(flag);
        let parent_on = parent_use.contains(flag);
        let flipped = match ud.kind {
            UseDepKind::Enabled => !vdb_on && eff_on,
            UseDepKind::Disabled => vdb_on && !eff_on,
            UseDepKind::Conditional => parent_on && !vdb_on && eff_on,
            UseDepKind::ConditionalInverse => !parent_on && vdb_on && !eff_on,
            UseDepKind::Equal => vdb_on != eff_on && eff_on == parent_on,
            UseDepKind::EqualInverse => vdb_on != eff_on && eff_on != parent_on,
        };
        if flipped {
            flags.push(flag.to_string());
        }
    }
    flags
}

// --- `||` in-graph preference (post-solve reorder fixpoint) ------------------

/// One `||` group's address: the member version whose dep strings hold it,
/// the dep class index (DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND = 0-4, the
/// same order [`version_row`] consumes), and the group's pre-order index
/// among the class's `||` nodes. The index is structural: reordering or
/// dropping branches inside the node itself never moves it, so an override
/// computed from one solve still addresses the same group on the next
/// pass. The one shape that does move it (a nested `||` inside a dropped
/// branch) is caught by the stored content fingerprint instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OrGroupKey {
    cpv: String,
    class: usize,
    index: usize,
}

/// The desired branch order for one [`OrGroupKey`]: `order` is the full
/// branch-key sequence the engine should see (a branch missing from it is
/// dropped from the facts), `fingerprint` the sorted branch keys the order
/// was computed from. An entry whose group content changed addresses a
/// different group and is skipped on apply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct OrGroupOrder {
    fingerprint: Vec<String>,
    order: Vec<String>,
}

/// Per-solve `||` overrides for the pubgrub facts: which `||` groups read
/// in a different branch order (or without a branch) than the ebuild lists
/// them. Pubgrub-only: [`BridgeRepo::versions_for`] applies them, and the
/// resolvo backend reads `record.deps` straight from [`LazyRepo` and never
/// sees them, so the default/resolvo output does not move.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct OrChoiceOverrides {
    groups: HashMap<OrGroupKey, OrGroupOrder>,
}

impl OrChoiceOverrides {
    fn new() -> Self {
        Self::default()
    }

    /// Deterministic rendering for the fixpoint's oscillation history
    /// (fingerprint and order both: two passes can agree on the surviving
    /// branch while disagreeing on what was dropped).
    fn signature(&self) -> String {
        let mut parts: Vec<String> = self
            .groups
            .iter()
            .map(|(key, order)| {
                format!(
                    "{}|{}|{}={}|{}",
                    key.cpv,
                    key.class,
                    key.index,
                    order.fingerprint.join(","),
                    order.order.join(",")
                )
            })
            .collect();
        parts.sort();
        parts.join(";")
    }
}

/// Structural key for one direct `||` branch: the atom's identity (cp,
/// slot pin, version floor, USE deps) for [`portage_solver::DepEntry::Atom`]
/// branches, the nested content for group branches. Two branches naming the
/// same cp in different slots (`|| ( foo:1 foo:2 )`) key differently.
fn or_branch_key(entry: &portage_solver::DepEntry) -> String {
    match entry {
        portage_solver::DepEntry::Atom(dep) => {
            let version_part = match (&dep.op, &dep.version) {
                (Some(op), Some(version)) => {
                    format!("{op:?}{version:?}{}", if dep.glob { "*" } else { "" })
                }
                _ => String::new(),
            };
            let use_part = match &dep.use_deps {
                None => String::new(),
                Some(uds) => {
                    let mut parts: Vec<String> =
                        uds.iter().map(|item| format!("{item:?}")).collect();
                    parts.sort();
                    parts.join(",")
                }
            };
            format!(
                "atom:{}|{:?}|{}|{}",
                dep.cpn, dep.slot_dep, version_part, use_part
            )
        }
        portage_solver::DepEntry::AllOf(children) => format!(
            "all({})",
            children
                .iter()
                .map(or_branch_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        portage_solver::DepEntry::UseConditional {
            flag,
            negate,
            children,
        } => format!(
            "use({}{}:{})",
            if *negate { "!" } else { "" },
            flag.as_str(),
            children
                .iter()
                .map(or_branch_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        portage_solver::DepEntry::AnyOf(children) => format!(
            "any({})",
            children
                .iter()
                .map(or_branch_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        portage_solver::DepEntry::ExactlyOneOf(children) => format!(
            "one({})",
            children
                .iter()
                .map(or_branch_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        portage_solver::DepEntry::AtMostOneOf(children) => format!(
            "maybe({})",
            children
                .iter()
                .map(or_branch_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

/// Whether any atom in `entries` (nested groups included) is a blocker:
/// groups carrying blockers are never reordered or trimmed (the task's
/// "must not alter blockers" rule).
fn or_subtree_has_blocker(entries: &[portage_solver::DepEntry]) -> bool {
    entries.iter().any(|entry| match entry {
        portage_solver::DepEntry::Atom(dep) => dep.blocker.is_some(),
        portage_solver::DepEntry::UseConditional { children, .. } => {
            or_subtree_has_blocker(children)
        }
        portage_solver::DepEntry::AllOf(children)
        | portage_solver::DepEntry::AnyOf(children)
        | portage_solver::DepEntry::ExactlyOneOf(children)
        | portage_solver::DepEntry::AtMostOneOf(children) => or_subtree_has_blocker(children),
    })
}

/// Apply `overrides` to one dep class: every `||` node is visited in
/// pre-order (the same traversal [`compute_or_overrides`] numbers groups
/// with, so `index` lines up), then the stored order is enforced --
/// branches missing from it are dropped, unknown stored keys ignored. The
/// raw md5-cache strings (`raw_deps`, the merge-order/display source) are
/// untouched: only the parsed facts the engine converts change.
fn apply_or_overrides_to_entries(
    entries: &[portage_solver::DepEntry],
    key_prefix: (&str, usize),
    overrides: &OrChoiceOverrides,
    next_index: &mut usize,
) -> Vec<portage_solver::DepEntry> {
    use portage_solver::DepEntry;
    entries
        .iter()
        .map(|entry| match entry {
            DepEntry::AnyOf(children) => {
                let index = *next_index;
                *next_index += 1;
                let reordered: Vec<DepEntry> = match overrides.groups.get(&OrGroupKey {
                    cpv: key_prefix.0.to_string(),
                    class: key_prefix.1,
                    index,
                }) {
                    Some(want) => {
                        let live: Vec<String> = children.iter().map(or_branch_key).collect();
                        // A group whose content does not contain the stored
                        // content is a different node (a nested `||` inside
                        // a dropped branch moved the index onto it) and is
                        // left alone. Otherwise the stored order is
                        // enforced for the branches it names; live branches
                        // missing from it were dropped on the analysis pass
                        // and stay dropped, unknown stored keys are ignored.
                        // (`live` is the base record content, which never
                        // changes within a resolve, so a stored entry always
                        // still addresses its own group here.)
                        if !want.fingerprint.iter().all(|key| live.contains(key)) {
                            children.clone()
                        } else {
                            let mut unused: Vec<&DepEntry> = children.iter().collect();
                            let mut out = Vec::new();
                            for key in &want.order {
                                if let Some(pos) =
                                    unused.iter().position(|c| &or_branch_key(c) == key)
                                {
                                    out.push(unused.remove(pos).clone());
                                }
                            }
                            out
                        }
                    }
                    None => children.clone(),
                };
                DepEntry::AnyOf(
                    reordered
                        .into_iter()
                        .map(|child| {
                            apply_or_overrides_to_entry(&child, key_prefix, overrides, next_index)
                        })
                        .collect(),
                )
            }
            DepEntry::AllOf(children) => DepEntry::AllOf(
                children
                    .iter()
                    .map(|child| {
                        apply_or_overrides_to_entry(child, key_prefix, overrides, next_index)
                    })
                    .collect(),
            ),
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => DepEntry::UseConditional {
                flag: *flag,
                negate: *negate,
                children: children
                    .iter()
                    .map(|child| {
                        apply_or_overrides_to_entry(child, key_prefix, overrides, next_index)
                    })
                    .collect(),
            },
            // `^^`/`??` groups are never reordered (task rule), but a
            // nested `||` inside one still takes its own override.
            DepEntry::ExactlyOneOf(children) | DepEntry::AtMostOneOf(children) => {
                let reworked: Vec<DepEntry> = children
                    .iter()
                    .map(|child| {
                        apply_or_overrides_to_entry(child, key_prefix, overrides, next_index)
                    })
                    .collect();
                match entry {
                    DepEntry::ExactlyOneOf(_) => DepEntry::ExactlyOneOf(reworked),
                    _ => DepEntry::AtMostOneOf(reworked),
                }
            }
            _ => entry.clone(),
        })
        .collect()
}

/// Whether a kept installed version's build-time classes reach the engine:
/// expanded under deep, bdeps on, and not rebuilding (a rebuild member
/// already expands every class). Portage's `_add_pkg_deps` traverses
/// DEPEND/BDEPEND for a built package when `bdeps` is `y`/`auto`, blanking
/// them only with `--with-bdeps=n`.
// Portage: _emerge/depgraph.py, _add_pkg_deps (built-package bdeps branch)
fn keeps_buildtime(kept: &KeptInstalled, cpv: &str) -> bool {
    kept.deep_expands(cpv)
        && kept.with_bdeps
        && kept.installed.contains(cpv)
        && !kept.rebuild.contains(cpv)
}

/// Fold DEPEND/BDEPEND into RDEPEND for [`keeps_buildtime`] versions: the
/// engine answers a kept installed version with its runtime classes only, so
/// its build-time edges would never solve. Display still reads the raw split.
fn fold_buildtime_for_kept(
    classes: [Vec<portage_solver::DepEntry>; 5],
) -> [Vec<portage_solver::DepEntry>; 5] {
    let [depend, rdepend, bdepend, pdepend, idepend] = classes;
    let mut runtime = rdepend;
    runtime.extend(depend);
    runtime.extend(bdepend);
    [Vec::new(), runtime, Vec::new(), pdepend, idepend]
}

/// Apply `overrides` to all five dep classes: the traversal both
/// [`BridgeRepo::versions_for`] (engine facts) and [`or_member_view`]
/// (post-solve analysis) share, so a group index computed from one solve
/// addresses the same group on the next pass. The `||` counter restarts
/// per class (the class rides in the key); `^^`/`??` nodes are traversed
/// but never reordered.
fn apply_or_overrides_to_classes(
    classes: &[Vec<portage_solver::DepEntry>; 5],
    cpv: &str,
    overrides: &OrChoiceOverrides,
) -> [Vec<portage_solver::DepEntry>; 5] {
    std::array::from_fn(|class| {
        let mut next_index = 0;
        apply_or_overrides_to_entries(&classes[class], (cpv, class), overrides, &mut next_index)
    })
}

/// The [`apply_or_overrides_to_entries`] rule over one entry (for `||`
/// children that are groups themselves).
fn apply_or_overrides_to_entry(
    entry: &portage_solver::DepEntry,
    key_prefix: (&str, usize),
    overrides: &OrChoiceOverrides,
    next_index: &mut usize,
) -> portage_solver::DepEntry {
    apply_or_overrides_to_entries(
        std::slice::from_ref(entry),
        key_prefix,
        overrides,
        next_index,
    )
    .into_iter()
    .next()
    .expect("a one-entry override keeps one entry")
}

/// The engine's view of one solution member's dep classes: the same source
/// [`BridgeRepo::versions_for`] hands the engine (repo md5-cache, or the
/// vdb reader for a deep installed member), with the G3 self-cycle rule
/// and the current `||` overrides applied, plus the member's effective USE
/// (the same source [`BridgeRepo::desired_use`] answers). `None` for a
/// kept leaf (the engine sees empty deps) or a member with no record.
#[allow(clippy::too_many_arguments)]
fn or_member_view(
    lazy: &LazyRepo,
    kept: &KeptInstalled,
    installed: &[super::InstalledPackage],
    cp: &str,
    version: &str,
    overrides: &OrChoiceOverrides,
) -> Option<([Vec<portage_solver::DepEntry>; 5], HashSet<String>)> {
    let cpv = format!("{cp}-{version}");
    // Same leaf rule as `versions_for`: a kept non-rebuild member past the
    // `--deep=N` bound contributes no edges, so none of its `||` groups is
    // evaluated.
    if !kept.expands(&cpv) {
        return None;
    }
    let loaded = lazy.load_versions(cp).ok()?;
    let repo_record = loaded.iter().find(|record| record.version == version);
    // Classes: the vdb reader for an expanding installed member (like
    // `versions_for`, including versions gone from the repo), otherwise
    // the repo record.
    let (classes, slot) = match (
        kept.installed.contains(&cpv)
            && (kept.deep_expands(&cpv) || (kept.deep && kept.rebuild.contains(&cpv))),
        cp.split_once('/'),
    ) {
        (true, Some((category, package))) => {
            let classes = lazy.installed_parsed_deps(category, package, version);
            let slot = repo_record.map_or_else(
                || {
                    lazy.installed_record(category, package, version)
                        .map(|record| record.slot)
                        .unwrap_or_default()
                },
                |record| record.slot.clone(),
            );
            (classes, slot)
        }
        _ => {
            let record = repo_record?;
            (record.deps.clone(), record.slot.clone())
        }
    };
    // Same fold as `versions_for`, so the `||` override indices and the walk
    // simulation see the classes the engine solves under.
    let classes = if keeps_buildtime(kept, &cpv) {
        fold_buildtime_for_kept(classes)
    } else {
        classes
    };
    // USE: the vdb flags for an expanding kept member, else the config USE of
    // the repo record (like `desired_use`; an installed-only version with
    // no repo record answers empty there too).
    let use_set =
        if kept.deep_expands(&cpv) && kept.installed.contains(&cpv) && !kept.rebuild.contains(&cpv)
        {
            match cp.split_once('/') {
                Some((category, package)) => {
                    installed_pkg_iuse_and_use(&lazy.root, category, package, version).1
                }
                None => HashSet::new(),
            }
        } else if let Some(record) = repo_record {
            lazy.use_of(record)
        } else {
            HashSet::new()
        };
    // What the engine converts: the G3 self-cycle strip, then the current
    // `||` overrides (the shared traversal, so group indices line up with
    // the facts `versions_for` hands the engine).
    let stripped = without_self_alternatives(cp, &slot, installed, &classes);
    Some((
        apply_or_overrides_to_classes(&stripped, &cpv, overrides),
        use_set,
    ))
}

/// Every atom under one `||` branch (direct or nested in a plain group).
fn or_branch_atoms<'a>(
    branch: &'a portage_solver::DepEntry,
    out: &mut Vec<&'a portage_solver::Dep>,
) {
    match branch {
        portage_solver::DepEntry::Atom(dep) => out.push(dep),
        portage_solver::DepEntry::AllOf(children) => {
            for child in children {
                or_branch_atoms(child, out);
            }
        }
        // A conditional or nested choice as a direct `||` branch: its atoms
        // still name the packages the branch would merge.
        portage_solver::DepEntry::UseConditional { children, .. } => {
            for child in children {
                or_branch_atoms(child, out);
            }
        }
        portage_solver::DepEntry::AnyOf(children)
        | portage_solver::DepEntry::ExactlyOneOf(children)
        | portage_solver::DepEntry::AtMostOneOf(children) => {
            for child in children {
                or_branch_atoms(child, out);
            }
        }
    }
}

/// Whether one `||` branch is the branch Portage would take: every package
/// it names is selected independently of this group (in the plan *and*
/// required from another parent or as a root target) *before* this group
/// resolves in Portage's walk (a package added later, like util-linux
/// behind the pager root, never won the group), its atoms match the
/// planned versions (slot pins still bind), and its USE deps hold under
/// the member's effective USE. `installed_blocking` names a direct atom
/// branch an installed instance satisfies while it is not the desired one:
/// the engine's installed-preference takes it regardless of branch order
/// (`provider/solve.rs`), so reordering alone cannot fix the group and the
/// branch is dropped from the facts instead.
struct OrBranchStanding {
    qualifies: bool,
    installed_blocking: bool,
}

#[allow(clippy::too_many_arguments)]
fn or_branch_standing(
    lazy: &LazyRepo,
    installed: &[super::InstalledPackage],
    branch: &portage_solver::DepEntry,
    member_cp: &str,
    member_cpv: &str,
    class: usize,
    index: usize,
    walk: &WalkOrder,
    member_use: &HashSet<String>,
    plan_ver: &HashMap<String, String>,
    taken: &HashSet<(String, String)>,
    root_cps: &HashSet<String>,
) -> OrBranchStanding {
    let not_qualifying = OrBranchStanding {
        qualifies: false,
        installed_blocking: false,
    };
    let mut atoms = Vec::new();
    or_branch_atoms(branch, &mut atoms);
    if atoms.is_empty() {
        return not_qualifying;
    }
    let direct_atom = match branch {
        portage_solver::DepEntry::Atom(dep) => Some(dep),
        _ => None,
    };
    // Group branches (`( a b )`, conditionals) reorder only: dropping one
    // of several packages is never faithful.
    let mut installed_blocking = false;
    if let Some(dep) = direct_atom {
        let cp = format!("{}", dep.cpn);
        if installed_satisfies_self(dep, &cp, installed) {
            installed_blocking = true;
        }
    }
    for atom in atoms {
        let cp = format!("{}", atom.cpn);
        // Independently selected: in the plan, and required from somewhere
        // other than this group (another parent's edge, or the probe's own
        // target). A package the solve merged only through this group
        // (gnome-keyring via the virtual) is not.
        let Some(plan_version) = plan_ver.get(&cp) else {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        };
        if !root_cps.contains(&cp)
            && !taken
                .iter()
                .any(|(parent, child)| child == &cp && parent != member_cp)
        {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        }
        // Walk-ordered: in Portage's walk the group resolves against the
        // graph as it stands then, so an alternative added only later (or
        // never — a complete-mode latecomer the walk never reaches) cannot
        // have won it. A member the walk never reaches keeps the old
        // order-blind answer: there is no walk information to refine it.
        let early = match (
            walk.added.get(&cp),
            walk.resolved.get(&(member_cpv.to_string(), class, index)),
        ) {
            (Some(added), Some(resolved)) => added < resolved,
            _ => !walk.added.contains_key(member_cp),
        };
        if !early {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        }
        // The atom must match the planned version: a slot pin naming
        // another slot (or a version floor the plan does not satisfy) is a
        // different choice, not this branch winning.
        let plan_cpv = format!("{cp}-{plan_version}");
        let Ok(cpv) = portage_solver::Cpv::parse(&plan_cpv) else {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        };
        let slot = lazy
            .load_versions(&cp)
            .ok()
            .and_then(|versions| {
                versions
                    .into_iter()
                    .find(|record| record.version == *plan_version)
            })
            .map(|record| record.slot);
        let Some(slot) = slot else {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        };
        if !atom.matches_cpv(&cpv, Some(&slot)) {
            return OrBranchStanding {
                qualifies: false,
                installed_blocking,
            };
        }
        // USE deps still bind: a branch Portage would reject (or
        // autounmask-gate) is not the faithful pick.
        if let Some(uds) = atom.use_deps.as_ref()
            && !uds.is_empty()
        {
            let record = lazy
                .load_versions(&cp)
                .ok()
                .and_then(|versions| versions.into_iter().find(|r| r.version == *plan_version));
            let Some(record) = record else {
                return OrBranchStanding {
                    qualifies: false,
                    installed_blocking,
                };
            };
            let repo_iuse: HashSet<String> = parse_iuse(&record.iuse).into_iter().collect();
            if !use_deps_satisfied(uds, member_use, &repo_iuse, &lazy.use_of(&record)) {
                return OrBranchStanding {
                    qualifies: false,
                    installed_blocking,
                };
            }
        }
    }
    OrBranchStanding {
        qualifies: true,
        installed_blocking,
    }
}

/// Visit every `||` node in one dep class in pre-order (the same traversal
/// [`apply_or_overrides_to_entries`] numbers groups with): collected as
/// `(index, children snapshot, active under the member USE)`. `^^`/`??`
/// nodes are traversed but never collected.
fn visit_or_groups(
    entries: &[portage_solver::DepEntry],
    member_use: &HashSet<String>,
    active: bool,
    next_index: &mut usize,
    out: &mut Vec<(usize, Vec<portage_solver::DepEntry>, bool)>,
) {
    use portage_solver::DepEntry;
    for entry in entries {
        match entry {
            DepEntry::AnyOf(children) => {
                let index = *next_index;
                *next_index += 1;
                out.push((index, children.clone(), active));
                for child in children {
                    visit_or_groups(
                        std::slice::from_ref(child),
                        member_use,
                        active,
                        next_index,
                        out,
                    );
                }
            }
            DepEntry::AllOf(children) => {
                visit_or_groups(children, member_use, active, next_index, out);
            }
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => {
                let child_active = active && (member_use.contains(flag.as_str()) != *negate);
                visit_or_groups(children, member_use, child_active, next_index, out);
            }
            DepEntry::ExactlyOneOf(children) | DepEntry::AtMostOneOf(children) => {
                visit_or_groups(children, member_use, active, next_index, out);
            }
            _ => {}
        }
    }
}

/// Portage's graph-fill order simulated over one solved plan: which
/// packages are already in the digraph when each `||` group resolves.
/// `added` maps `cp` to its `_add_pkg` step; `resolved` maps
/// `(member cpv, dep class, group index)` to its disjunction-pop step.
/// A member or alternative the walk never reaches (complete-mode
/// latecomers) has no entry and counts as added last.
/// // Portage: `_emerge/depgraph.py`, `_create_graph`.
struct WalkOrder {
    added: HashMap<String, usize>,
    resolved: HashMap<(String, usize, usize), usize>,
}

/// Active plain (non-`||`) atoms of one dep class in written order: what
/// `_queue_disjunctive_deps` yields for immediate `_add_dep` (`^^`/`??`
/// groups are transparent there, as in `dep_zapdeps`; `||` branches are
/// not collected — they enter at resolution time).
fn walk_plain_children(
    entries: &[portage_solver::DepEntry],
    member_use: &HashSet<String>,
    active: bool,
    out: &mut Vec<String>,
) {
    use portage_solver::DepEntry;
    for entry in entries {
        match entry {
            DepEntry::Atom(dep) => {
                if active && dep.blocker.is_none() {
                    out.push(format!("{}", dep.cpn));
                }
            }
            DepEntry::AllOf(children) => {
                walk_plain_children(children, member_use, active, out);
            }
            DepEntry::UseConditional {
                flag,
                negate,
                children,
            } => {
                walk_plain_children(
                    children,
                    member_use,
                    active && (member_use.contains(flag.as_str()) != *negate),
                    out,
                );
            }
            // A `||` branch's atoms enter only when the group resolves.
            DepEntry::AnyOf(_) => {}
            DepEntry::ExactlyOneOf(children) | DepEntry::AtMostOneOf(children) => {
                walk_plain_children(children, member_use, active, out);
            }
        }
    }
}

/// Dep classes in the order `_add_pkg_deps` processes them (build-time
/// classes queue their disjunctions last, so those pop first).
const WALK_CLASS_ORDER: [usize; 5] = [1, 4, 3, 0, 2];

/// One queued disjunction frame: a member's `||` groups of one dep class
/// in written order (frames pop LIFO across members and classes).
struct WalkFrame {
    member_cp: String,
    member_cpv: String,
    class: usize,
    groups: Vec<(usize, Vec<portage_solver::DepEntry>)>,
}

/// Append-only walk add: first add wins (`previously_added` is never
/// re-pushed), and only plan members enter the graph.
fn walk_add(
    walk: &mut WalkOrder,
    step: &mut usize,
    stack: &mut Vec<(String, String)>,
    plan_ver: &HashMap<&str, &str>,
    cp: &str,
) {
    if plan_ver.contains_key(cp) && !walk.added.contains_key(cp) {
        walk.added.insert(cp.to_string(), *step);
        *step += 1;
        stack.push((cp.to_string(), plan_ver[cp].to_string()));
    }
}

/// Simulate `_create_graph` over the solved plan: roots first (in
/// request-atom order), then a LIFO package stack for plain deps plus a
/// LIFO disjunction stack for `||` groups (drained only when the package
/// stack empties). Taken children of a group resolve after every group
/// of their frame (`_select_atoms` finishes before `_minimize_children`
/// adds anything), so they never count as in-graph for their own frame.
fn simulate_walk_order(
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
    root_order: &[String],
    views: &HashMap<String, ([Vec<portage_solver::DepEntry>; 5], HashSet<String>)>,
) -> WalkOrder {
    let plan_ver: HashMap<&str, &str> = order
        .iter()
        .map(|(cp, version)| (cp.as_str(), version.as_str()))
        .collect();
    let taken = solution_taken_edges(parents);
    let mut walk = WalkOrder {
        added: HashMap::new(),
        resolved: HashMap::new(),
    };
    let mut step: usize = 0;
    let mut stack: Vec<(String, String)> = Vec::new();
    for cp in root_order {
        walk_add(&mut walk, &mut step, &mut stack, &plan_ver, cp);
    }
    // One frame per (member, class): its groups in written order.
    let mut disj: Vec<WalkFrame> = Vec::new();
    while !stack.is_empty() || !disj.is_empty() {
        while let Some((cp, version)) = stack.pop() {
            let cpv = format!("{cp}-{version}");
            let Some((classes, use_set)) = views.get(&cpv) else {
                continue;
            };
            for class in WALK_CLASS_ORDER {
                let mut plain = Vec::new();
                walk_plain_children(&classes[class], use_set, true, &mut plain);
                for child in &plain {
                    walk_add(&mut walk, &mut step, &mut stack, &plan_ver, child);
                }
                let mut found = Vec::new();
                let mut next_index = 0;
                visit_or_groups(&classes[class], use_set, true, &mut next_index, &mut found);
                let frame: Vec<(usize, Vec<portage_solver::DepEntry>)> = found
                    .into_iter()
                    .filter(|(_, children, active)| *active && !children.is_empty())
                    .map(|(index, children, _)| (index, children))
                    .collect();
                if !frame.is_empty() {
                    disj.push(WalkFrame {
                        member_cp: cp.clone(),
                        member_cpv: cpv.clone(),
                        class,
                        groups: frame,
                    });
                }
            }
        }
        if let Some(frame) = disj.pop() {
            for (index, _) in &frame.groups {
                walk.resolved
                    .insert((frame.member_cpv.clone(), frame.class, *index), step);
                step += 1;
            }
            for (_, children) in &frame.groups {
                for branch in children {
                    let mut atoms = Vec::new();
                    or_branch_atoms(branch, &mut atoms);
                    for atom in atoms {
                        let child = format!("{}", atom.cpn);
                        if taken.contains(&(frame.member_cp.clone(), child.clone())) {
                            walk_add(&mut walk, &mut step, &mut stack, &plan_ver, &child);
                        }
                    }
                }
            }
        }
    }
    walk
}

/// The desired `||` facts for the next solve, from the solved plan: for
/// every `||` group in the deps of a selected member (as the engine saw
/// them, G3 strip and current overrides included), the first-listed
/// independently-selected branch moves first (the engine's final fallback
/// is the first-listed alternative), and an installed-satisfying direct
/// branch that is not that pick is dropped (the engine's
/// installed-preference ignores order, so keeping it would keep the wrong
/// pick). Portage: atoms for already-in-graph packages preferred
/// (`depgraph.py:778-793`), with the in-bin in-graph promotion over an
/// installed-only alternative (`dep_check.py` `dep_zapdeps`). Groups with
/// no independently-selected branch, with blockers, or behind an
/// unsatisfied USE conditional keep their facts: the engine and Portage
/// already agree there (first-listed fallback on both sides).
#[allow(clippy::too_many_arguments)]
fn compute_or_overrides(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    kept: &KeptInstalled,
    installed: &[super::InstalledPackage],
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
    root_cps: &HashSet<String>,
    current: &OrChoiceOverrides,
) -> OrChoiceOverrides {
    if req.nodeps {
        return current.clone();
    }
    let taken = solution_taken_edges(parents);
    let plan_ver: HashMap<String, String> = order
        .iter()
        .map(|(cp, version)| (cp.clone(), version.clone()))
        .collect();
    // Request-atom order: the walk appends arguments in order (popped
    // reversed), so every root is in the graph before any group resolves.
    let mut root_order = Vec::new();
    for atom in &req.atoms {
        let Ok(dep) = portage_solver::Dep::parse(atom) else {
            continue;
        };
        if dep.blocker.is_some() {
            continue;
        }
        let cp = format!("{}", dep.cpn);
        if !root_order.contains(&cp) {
            root_order.push(cp);
        }
    }
    let mut member_list: Vec<&(String, String)> = order.iter().collect();
    member_list.sort();
    // One engine-view per member, shared by the walk simulation and the
    // group analysis so both see the same classes and USE.
    let mut views: HashMap<String, ([Vec<portage_solver::DepEntry>; 5], HashSet<String>)> =
        HashMap::new();
    for (cp, version) in member_list.clone() {
        if let Some(view) = or_member_view(lazy, kept, installed, cp, version, current) {
            views.insert(format!("{cp}-{version}"), view);
        }
    }
    let walk = simulate_walk_order(order, parents, &root_order, &views);
    let mut groups = OrChoiceOverrides::new();
    for (cp, version) in member_list {
        let Some((classes, member_use)) = views.get(&format!("{cp}-{version}")) else {
            continue;
        };
        let member_cpv = format!("{cp}-{version}");
        for (class, entries) in classes.iter().enumerate() {
            let mut found = Vec::new();
            let mut next_index = 0;
            visit_or_groups(entries, member_use, true, &mut next_index, &mut found);
            for (index, children, active) in found {
                if !active || children.is_empty() {
                    continue;
                }
                if or_subtree_has_blocker(&children) {
                    continue;
                }
                let keys: Vec<String> = children.iter().map(or_branch_key).collect();
                let mut fingerprint = keys.clone();
                fingerprint.sort();
                let standings: Vec<OrBranchStanding> = children
                    .iter()
                    .map(|branch| {
                        or_branch_standing(
                            lazy,
                            installed,
                            branch,
                            cp,
                            &member_cpv,
                            class,
                            index,
                            &walk,
                            member_use,
                            &plan_ver,
                            &taken,
                            root_cps,
                        )
                    })
                    .collect();
                let desired = standings.iter().position(|standing| standing.qualifies);
                let mut ordered = Vec::new();
                if let Some(first) = desired {
                    ordered.push(keys[first].clone());
                }
                for (pos, key) in keys.iter().enumerate() {
                    if Some(pos) == desired {
                        continue;
                    }
                    // An installed-satisfying branch the faithful pick does
                    // not need: the engine would take it over any order, so
                    // it leaves the facts (like the G3 self-cycle drop; the
                    // branch stays merged when another parent needs it).
                    // Only when a desired pick exists: without one the
                    // engine and Portage already agree on the installed
                    // branch (the xapp/zlib shape), and dropping it would
                    // force the untaken alternative instead.
                    if desired.is_some() && standings[pos].installed_blocking {
                        pg_trace(
                            &mut std::io::stderr(),
                            format_args!("pg-or-drop {cp}-{version} [{class}:{index}] {key}"),
                        );
                        continue;
                    }
                    ordered.push(key.clone());
                }
                if ordered.len() != children.len() || ordered.iter().ne(keys.iter()) {
                    pg_trace(
                        &mut std::io::stderr(),
                        format_args!(
                            "pg-or-group {cp}-{version} [{class}:{index}] -> {}",
                            ordered.join(" | ")
                        ),
                    );
                }
                groups.groups.insert(
                    OrGroupKey {
                        cpv: format!("{cp}-{version}"),
                        class,
                        index,
                    },
                    OrGroupOrder {
                        fingerprint,
                        order: ordered,
                    },
                );
            }
        }
    }
    groups
}

// --- PubGrub backend -------------------------------------------------------

/// Whether the pubgrub-bridge trace is active (`PORTUALE_PG_TRACE=1`).
fn pg_trace_enabled() -> bool {
    std::env::var("PORTUALE_PG_TRACE").is_ok_and(|value| value == "1")
}

/// Write one trace `line`; no-op unless [`pg_trace_enabled`].
fn pg_trace(out: &mut dyn Write, line: fmt::Arguments) {
    if pg_trace_enabled() {
        let _ = writeln!(out, "{line}");
    }
}

/// One `pg-installed` line for an installed package.
fn pg_installed_line(
    inst: &super::InstalledPackage,
    policy: portage_atom_pubgrub::InstalledPolicy,
) -> String {
    format!(
        "pg-installed {} slot={} policy={policy:?}",
        inst.cpv(),
        inst.slot
    )
}

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
    kept: KeptInstalled,
    /// Installed cpvs for the G3 self-cycle rule (an alternative an installed
    /// instance satisfies is kept). Pubgrub only: resolvo reads
    /// `record.deps` straight from `LazyRepo` and never sees this.
    installed: Vec<super::InstalledPackage>,
    /// #34 S5 G5/G6: the current `||` reorder/drop pass. Pubgrub only, for
    /// the same reason as `installed`.
    or_overrides: OrChoiceOverrides,
}

impl portage_atom_pubgrub::PackageRepository for BridgeRepo {
    fn all_packages(&self) -> Vec<portage_solver::Cpn> {
        self.lazy.closure_cpns()
    }

    fn versions_for(
        &self,
        cpn: &portage_solver::Cpn,
    ) -> Vec<(portage_solver::Cpv, portage_atom_pubgrub::PackageVersions)> {
        let cp = format!("{cpn}");
        let versions = self.lazy.load_versions(&cp).unwrap_or_default();
        let mut out = Vec::new();
        for record in &versions {
            // #34 S2: without deep, an installed package that is not being rebuilt
            // is a leaf: Portage sends its deps to `_ignored_deps` instead of
            // expanding them. #432: under `--deep=N` only installed cpvs
            // inside the bounded pre-solve BFS set expand. The trace then
            // shows no edges for it, like the engine.
            // Portage: depgraph.py:3815-3824, :7359-7375
            let leaf = !self.kept.expands(&record.cpv());
            // #34 S3: under deep, an installed cpv's deps come from the vdb
            // (the default resolver's dynamic-deps reader), not the repo
            // md5-cache: Portage expands the installed node with the flags it
            // was built with. Portage: FakeVartree.py, _apply_dynamic_deps.
            let vdb_classes;
            let cpv = record.cpv();
            let classes: &[Vec<portage_solver::DepEntry>; 5] = match (
                self.kept.installed.contains(&cpv)
                    && (self.kept.deep_expands(&cpv)
                        || (self.kept.deep && self.kept.rebuild.contains(&cpv))),
                cp.split_once('/'),
            ) {
                (true, Some((category, package))) => {
                    vdb_classes =
                        self.lazy
                            .installed_parsed_deps(category, package, &record.version);
                    &vdb_classes
                }
                _ => &record.deps,
            };
            // One line per referenced CPN per dep class, for the S0 chain walk.
            if pg_trace_enabled() && !leaf {
                const CLASSES: [&str; 5] = ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"];
                let parent = record.cpv();
                for (class, deps) in CLASSES.iter().zip(classes.iter()) {
                    let mut refs = Vec::new();
                    referenced_cpns(deps, &mut refs);
                    for child in refs {
                        pg_trace(
                            &mut std::io::stderr(),
                            format_args!("pg-edge {parent} -> {child} ({class})"),
                        );
                    }
                }
            }
            let empty: [Vec<portage_solver::DepEntry>; 5] = Default::default();
            // #34 S5 G3: drop a `||` alternative naming this candidate's own
            // cp-slot (a direct self-cycle Portage avoids at selection), so
            // the engine takes the sibling (go picks go-bootstrap).
            // #34 S5 G5/G6: then the `||` reorder/drop pass (an
            // already-in-graph alternative first, an installed wrong pick
            // dropped), so the engine takes the in-graph branch Portage
            // prefers (`depgraph.py:778-793`).
            let deps = if leaf {
                empty
            } else {
                // #390: a kept installed version expanded under deep with bdeps
                // on folds its build-time classes into the runtime view the
                // engine reads for it.
                let folded;
                let classes: &[Vec<portage_solver::DepEntry>; 5] =
                    if keeps_buildtime(&self.kept, &record.cpv()) {
                        folded = fold_buildtime_for_kept(classes.clone());
                        &folded
                    } else {
                        classes
                    };
                let stripped =
                    without_self_alternatives(&cp, &record.slot, &self.installed, classes);
                apply_or_overrides_to_classes(&stripped, &record.cpv(), &self.or_overrides)
            };
            if let Some(row) = version_row(record, deps) {
                out.push(row);
            }
        }
        // #34 S3: installed versions absent from the repo are offered with
        // their vdb deps, so the crate's empty-deps stub
        // (`provider/mod.rs:742-753`) never stands in for them. Portage
        // expands them from the vdb with moves applied
        // (resolve/installed_meta.rs:126-130).
        if self.kept.deep
            && let Some((category, package)) = cp.split_once('/')
        {
            for (version, _, _) in installed_candidates(&self.lazy.root, category, package) {
                if versions.iter().any(|v| v.version == version) {
                    continue;
                }
                // #34 S5 G1: never offer a masked installed-only version
                // Portage downgrades away from: the crate's `max()` would
                // keep it, where Portage re-selects the best visible
                // version (`UD`) and expands that version's deps.
                if self.kept.downgrade.contains(&format!("{cp}-{version}")) {
                    continue;
                }
                let Some(record) = self.lazy.installed_record(category, package, &version) else {
                    continue;
                };
                if pg_trace_enabled() {
                    const CLASSES: [&str; 5] =
                        ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"];
                    let parent = record.cpv();
                    for (class, deps) in CLASSES.iter().zip(record.deps.iter()) {
                        let mut refs = Vec::new();
                        referenced_cpns(deps, &mut refs);
                        for child in refs {
                            pg_trace(
                                &mut std::io::stderr(),
                                format_args!("pg-edge {parent} -> {child} ({class})"),
                            );
                        }
                    }
                }
                // #390: same kept-buildtime fold as the repo path above.
                // #432: past the `--deep=N` bound the offer is a leaf (empty
                // deps), like the repo path above.
                let leaf = !self.kept.expands(&record.cpv());
                let kept_folded;
                let source: &[Vec<portage_solver::DepEntry>; 5] =
                    if keeps_buildtime(&self.kept, &record.cpv()) {
                        kept_folded = fold_buildtime_for_kept(record.deps.clone());
                        &kept_folded
                    } else {
                        &record.deps
                    };
                let empty: [Vec<portage_solver::DepEntry>; 5] = Default::default();
                if let Some(row) = version_row(
                    &record,
                    if leaf {
                        empty
                    } else {
                        // #34 S5 G3: same self-cycle rule as the repo path above.
                        // #34 S5 G5/G6: same `||` reorder/drop pass.
                        apply_or_overrides_to_classes(
                            &without_self_alternatives(&cp, &record.slot, &self.installed, source),
                            &record.cpv(),
                            &self.or_overrides,
                        )
                    },
                ) {
                    out.push(row);
                }
            }
        }
        out
    }

    fn desired_use(&self, cpv: &portage_solver::Cpv) -> portage_atom_pubgrub::UseConfig {
        use portage_atom_pubgrub::UseConfig;
        let cp_text = format!("{}", cpv.cpn);
        let version_text = format!("{}", cpv.version);
        let cpv_str = format!("{cp_text}-{version_text}");
        // #34 S3: under deep, an installed cpv keeps the USE it was built
        // with (the vdb), whether or not its version is still in the repo:
        // Portage expands the installed node with `_pkg_use_enabled`, while
        // the config USE (`package.use` included) would wrongly switch its
        // conditional deps on or off. Portage: depgraph.py, _pkg_use_enabled.
        // #34 S4: except a rebuild member, which Portage rebuilds with the
        // current config USE (its `x?` deps switch on for the rebuild, e.g.
        // `R pg34-ulib` pulling `N pg34-uextra`). #432: past the `--deep=N`
        // bound the kept leaf's deps never reach the engine, so the config
        // USE answers there too.
        if self.kept.deep_expands(&cpv_str)
            && self.kept.installed.contains(&cpv_str)
            && !self.kept.rebuild.contains(&cpv_str)
            && let Some((category, package)) = cp_text.split_once('/')
        {
            let (_iuse, use_set) =
                installed_pkg_iuse_and_use(&self.lazy.root, category, package, &version_text);
            let mut use_config = UseConfig::new();
            for flag in &use_set {
                use_config.enable(interned(flag));
            }
            if pg_trace_enabled() {
                let mut flags: Vec<&str> = use_set.iter().map(String::as_str).collect();
                flags.sort();
                pg_trace(
                    &mut std::io::stderr(),
                    format_args!("pg-use {cpv_str} source=vdb {}", flags.join(" ")),
                );
            }
            return use_config;
        }
        let Some(record) = self.lazy.record(&cp_text, &version_text) else {
            return UseConfig::new();
        };
        let mut use_config = UseConfig::new();
        let use_set = self.lazy.use_of(&record);
        for flag in &use_set {
            use_config.enable(interned(flag));
        }
        if pg_trace_enabled() {
            let mut flags: Vec<&str> = use_set.iter().map(String::as_str).collect();
            flags.sort();
            pg_trace(
                &mut std::io::stderr(),
                format_args!(
                    "pg-use {cp_text}-{version_text} source=config {}",
                    flags.join(" ")
                ),
            );
        }
        use_config
    }
}

/// The pubgrub provider over [`KeptInstalled`]: installed cpvs in
/// `kept.rebuild` register `InstalledPolicy::Rebuild` instead of `Favor` -- without
/// it the `R` row shows but the reinstalled package's DEPEND/BDEPEND never expand.
#[allow(clippy::too_many_arguments)]
fn pubgrub_provider(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    kept: &KeptInstalled,
    installed: &[super::InstalledPackage],
    seed_cpns: &[portage_solver::Cpn],
    overrides: &OrChoiceOverrides,
) -> portage_atom_pubgrub::PortageDependencyProvider {
    use portage_atom_pubgrub::{
        InstalledPackage as BridgeInstalled, InstalledPolicy, PortageDependencyProvider,
        PortagePackage,
    };
    // Reachable conversion, not whole-tree; the provider takes a clone while the
    // mapping keeps the original (memoized maps make the copy cheap).
    let mut provider = PortageDependencyProvider::new_for_targets(
        BridgeRepo {
            lazy: lazy.clone(),
            kept: kept.clone(),
            installed: installed.to_vec(),
            or_overrides: overrides.clone(),
        },
        seed_cpns.to_vec(),
    );
    // #34 S2 (moved here from S4): without it `-pve` keeps old installed versions
    // where Portage shows `U`.
    provider.set_rebuild_tree(req.empty);
    // Ported from portage-cli src/query/depgraph/round.rs (MIT, Luca Barbato)
    let deep = !matches!(req.deep, Deep::NotRequested);
    // The walk clears selective for `-e` (resolve/pretend.rs:407-408).
    let selective = req.selective && !req.empty;
    provider.set_nodeps(req.nodeps);
    provider.set_prefer_newest_slot((deep && req.update) || req.empty);
    provider.set_prefer_update(req.update && deep && !req.empty);
    provider.set_selective_no_update(selective && !req.update);
    for inst in installed.iter().cloned() {
        // #34 S5 G1: a masked installed version Portage downgrades is not
        // registered at all: Favor (or the crate's empty-deps installed
        // stub, provider/mod.rs:742-753) would keep it even for a named
        // target, where Portage re-selects the best visible version (`UD`).
        if kept.downgrade.contains(&inst.cpv()) {
            continue;
        }
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
        let policy = if kept.rebuild.contains(&inst.cpv()) {
            InstalledPolicy::Rebuild
        } else {
            InstalledPolicy::Favor
        };
        provider.add_installed(BridgeInstalled {
            package,
            version,
            policy,
            active_use: use_set.into_iter().map(|f| interned(&f)).collect(),
            iuse: iuse_set.into_iter().map(|f| interned(&f)).collect(),
        });
        pg_trace(
            &mut std::io::stderr(),
            format_args!("{}", pg_installed_line(&inst, policy)),
        );
    }
    provider
}

/// Root solve targets for one [`resolve_pubgrub`] round: a named `:slot`
/// pins one slotted node, otherwise every slotted node (falling back to
/// the whole slotted-node fan-out, D-target-all-slots, when the slot
/// decision finds nothing).
fn pubgrub_targets(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    installed: &[super::InstalledPackage],
    provider: &portage_atom_pubgrub::PortageDependencyProvider,
) -> Result<
    Vec<(
        portage_atom_pubgrub::PortagePackage,
        portage_atom_pubgrub::PortageVersionSet,
    )>,
    String,
> {
    use portage_atom_pubgrub::{PortagePackage, PortageVersionSet};
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
                pg_trace(
                    &mut std::io::stderr(),
                    format_args!("pg-target {} slot={slot}", dep.cpn),
                );
                targets.push((PortagePackage::slotted(dep.cpn, interned(&slot)), versions))
            }
            None => {
                let nodes = provider.packages_for_cpn(&dep.cpn);
                let slots = root_target_slots(req, lazy, installed, &dep, &versions);
                let picked: Vec<PortagePackage> = slots
                    .iter()
                    .map(|slot| PortagePackage::slotted(dep.cpn, interned(slot)))
                    .filter(|node| nodes.contains(node))
                    .collect();
                if !picked.is_empty() {
                    pg_trace(
                        &mut std::io::stderr(),
                        format_args!("pg-target {} slot={}", dep.cpn, slots.join(",")),
                    );
                    targets.extend(picked.into_iter().map(|n| (n, versions.clone())));
                } else if nodes.is_empty() {
                    pg_trace(
                        &mut std::io::stderr(),
                        format_args!("pg-target {} slot=-", dep.cpn),
                    );
                    targets.push((PortagePackage::unslotted(dep.cpn), versions));
                } else {
                    // `*`: the whole slotted-node fan-out (D-target-all-slots).
                    pg_trace(
                        &mut std::io::stderr(),
                        format_args!("pg-target {} slot=*", dep.cpn),
                    );
                    targets.extend(nodes.into_iter().map(|n| (n, versions.clone())));
                }
            }
        }
    }
    Ok(targets)
}

/// One [`resolve_pubgrub`] round's solved plan in bridge terms: ordered
/// `(cp, version)` members plus the child-cpv to requirers edge map.
type SolvedRound = (
    Vec<(String, String)>,
    HashMap<String, Vec<(String, String)>>,
);

/// Bound on the `||` reorder/drop passes of one [`resolve_pubgrub`] run
/// (the G2 parent-USE growth stays monotonic and unbounded, as before):
/// real plans converge in one or two passes; past this the loop falls
/// back to the previous plan instead of oscillating.
const OR_FIXPOINT_MAX_PASSES: usize = 8;

/// Absolute bound on the combined solve loop below (G2 growth plus `||`
/// passes); a pathological flap falls back to the previous plan rather
/// than solving forever.
const PUBGRUB_FIXPOINT_MAX_ROUNDS: usize = 32;

// --- S9 post-solve abort validator -------------------------------------------

/// S9 (#363/#364/#365): what the read-only post-solve validator fills in:
/// the abort channels `graph_result_from_order` leaves empty when the
/// engine solved a plan Portage would have refused.
struct BridgeAbortValidation {
    autounmask_use_changes: Vec<AutounmaskChange>,
    masked_deps: Vec<MaskedDepReport>,
    use_unsat_deps: Vec<UseUnsatDepReport>,
    plain_miss_deps: Vec<PlainMissDepReport>,
    nvc_entries: Vec<GraphEntry>,
    nvc_dep_atoms: HashMap<(String, String), String>,
}

/// S9 (#363/#364/#365): replay the default path's three abort predicates
/// over a solved pubgrub plan -- REQUIRED_USE (`entry_use_display`), miss
/// disclosure (`record_miss_disclosure`) and the USE-change gate
/// (`entry_use_flags`) -- reusing their helpers. Read-only: `order` and
/// `parents` are untouched, and `None` (no notices) leaves the result
/// exactly as `graph_result_from_order` built it. `Err` is a REQUIRED_USE
/// refusal, the same `Error::Detail` text the walk returns.
// Portage: depgraph.py need_config_change:11708, _show_unsatisfied_dep, _add_pkg
fn bridge_validate_aborts(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    parents: &HashMap<String, Vec<(String, String)>>,
    entries: &[GraphEntry],
) -> Result<Option<BridgeAbortValidation>, String> {
    // `--nodeps` never expands deps, so nothing can miss or flip.
    if req.nodeps {
        return Ok(None);
    }
    // Merge-bound members plus installed-kept solution members: the
    // engine's plan treats a kept install as the selected version, so its
    // dependents' atoms match against it (the pg34-nlib shape: the child
    // is `AlreadyInstalled` while the parent still needs the flip). Only
    // merge-bound members get the REQUIRED_USE check and edge expansion,
    // like the walk (installed outcomes skip both).
    struct Member<'a> {
        cp: String,
        version: String,
        merge_bound: bool,
        record: LoadedVersion,
        use_set: HashSet<String>,
        entry: &'a GraphEntry,
    }
    let mut members: Vec<Member<'_>> = Vec::new();
    for entry in entries {
        let (version, merge_bound) = match &entry.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                (Some(version.clone()), true)
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                (Some(to.clone()), true)
            }
            PretendOutcome::AlreadyInstalled { version } => (Some(version.clone()), false),
            _ => (None, false),
        };
        let Some(version) = version else { continue };
        let cp = format!("{}/{}", entry.category, entry.package);
        // An installed-only pick has no repo record: read its vdb deps like
        // the entry builder does, so its dependents' atoms still match
        // against it instead of misclassifying as a miss.
        // Portage: resolve/pretend.rs:787-824 (installed-only AlreadyInstalled).
        let record = lazy.record(&cp, &version).or_else(|| {
            let (category, package) = cp.split_once('/')?;
            lazy.installed_record(category, package, &version)
        });
        let Some(record) = record else {
            continue;
        };
        let use_set = lazy.use_of(&record);
        members.push(Member {
            cp,
            version,
            merge_bound,
            record,
            use_set,
            entry,
        });
    }
    let top_level = TopLevel {
        atoms: req.atoms.iter().map(String::as_str).collect(),
        sets: &req.argument_sets,
    };
    // (c) REQUIRED_USE first: like `assemble_pass_result`, a violation fails
    // the whole call, so edge misses never report alongside one.
    let mut violations: Vec<((bool, usize), String)> = Vec::new();
    for member in &members {
        if !member.merge_bound {
            continue;
        }
        let required_use = member.record.required_use.trim();
        if required_use.is_empty() {
            continue;
        }
        let (category, package) = (
            member.entry.category.as_str(),
            member.entry.package.as_str(),
        );
        let iuse_set = implicit_iuse_set(&member.record.iuse, &req.config);
        match portage_required_use::check_required_use(required_use, &member.use_set, &iuse_set) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => return Err(e.to_string()),
        }
        let normalized = required_use
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let reduced = match portage_required_use::unsatisfied_reduced(
            required_use,
            &member.use_set,
            &iuse_set,
        ) {
            Ok(Some(reduced)) => portage_required_use::human_readable(&reduced),
            _ => portage_required_use::human_readable(&normalized),
        };
        let full = portage_required_use::human_readable(&normalized);
        let use_display =
            pkg_use_display_for(&lazy.repos, &req.config, category, package, &member.version);
        let cpv_repo = if member.record.repo_name.is_empty() {
            format!("{}/{package}-{}", category, member.version)
        } else {
            format!(
                "{}/{package}-{}::{}",
                category, member.version, member.record.repo_name
            )
        };
        let owner = parents
            .get(&format!("{}-{}", member.cp, member.version))
            .and_then(|requirers| requirers.first().cloned());
        let dep_chain = required_use_dep_chain(&owner, &top_level, entries);
        let xinfo = req
            .atoms
            .iter()
            .find(|atom| {
                portage_dep::parse_atom(atom).is_some_and(|parsed| {
                    format!("{}/{}", parsed.category, parsed.package) == member.cp
                })
            })
            .cloned()
            .or_else(|| {
                entries
                    .iter()
                    .flat_map(|entry| entry.deps.iter())
                    .find(|dep| dep.category == category && dep.package == package)
                    .map(|dep| dep.evaluated.clone())
            })
            .unwrap_or_else(|| member.cp.clone());
        // Walk (last-declared-first) order for top-level violations; a
        // dependency-only violation keeps plan order.
        let key = match req.atoms.iter().rposition(|atom| {
            portage_dep::parse_atom(atom).is_some_and(|parsed| {
                format!("{}/{}", parsed.category, parsed.package) == member.cp
            })
        }) {
            Some(position) => (false, usize::MAX - position),
            None => (true, 0),
        };
        violations.push((
            key,
            render_required_use_block(RenderRequiredUseBlockArgs {
                xinfo: &xinfo,
                cpv_repo: &cpv_repo,
                use_display: &use_display,
                reduced_human: &reduced,
                full_human: &full,
                dep_chain: &dep_chain,
            }),
        ));
    }
    if !violations.is_empty() {
        violations.sort_by_key(|(key, _)| *key);
        return Err(violations.into_iter().map(|(_, block)| block).collect());
    }
    // (a)/(b) edge pass over entry deps (already USE-reduced under the
    // member's config USE, like the walk's queued atoms): each dep either
    // resolves to a plan member whose USE satisfies it, needs a
    // `package.use` flip, or is a miss the disclosure helpers classify.
    let member_strings: Vec<String> = members
        .iter()
        .map(|member| {
            let entry = member.entry;
            format!(
                "{}-{}:{}/{}::{}",
                member.cp,
                member.version,
                entry.slot.clone().unwrap_or_default(),
                entry.sub_slot.clone().unwrap_or_default(),
                entry.repo_name.clone().unwrap_or_default(),
            )
        })
        .collect();
    let member_refs: Vec<&str> = member_strings.iter().map(String::as_str).collect();
    let mut flips: HashMap<String, HashMap<String, bool>> = HashMap::new();
    let mut changes: Vec<AutounmaskChange> = Vec::new();
    struct Miss {
        category: String,
        package: String,
        display: String,
        evaluated: String,
        owners: Vec<(String, String)>,
    }
    let mut misses: Vec<Miss> = Vec::new();
    for member in &members {
        // A kept install's own deps never expand under the engine's leaf
        // rule, so only merge-bound members contribute edges (they still
        // serve as match targets above).
        if !member.merge_bound {
            continue;
        }
        let owner = (member.entry.category.clone(), member.entry.package.clone());
        for dep in &member.entry.deps {
            // Choice groups resolve to the taken branch inside the engine;
            // attributing a USE atom to one branch needs the `||` walk the
            // fixpoint already ran, so deferred edges stay out.
            if dep.disjunctive {
                continue;
            }
            // Build-time keys only travel when the walk expands them.
            if (dep.key == 3 || dep.key == 4) && !req.with_bdeps {
                continue;
            }
            let Some(parsed) = portage_dep::parse_atom(&dep.evaluated) else {
                continue;
            };
            if parsed.blocker != portage_dep::Blocker::None {
                continue;
            }
            let member_matched = portage_dep::match_from_list(&dep.evaluated, &member_refs)
                .is_some_and(|hits| !hits.is_empty());
            // The walk settles `AlreadyInstalled` before any miss disclosure:
            // an installed instance satisfying the atom (USE-aware, like the
            // installed-only fallback) counts as satisfied even when no plan
            // member names it. Version-only helpers are repo-candidate-only,
            // so they must not decide this test.
            // Portage: resolve/pretend.rs:787-824.
            let installed_satisfies =
                super::best_installed_matching(&req.root, &dep.evaluated, &req.config).is_some();
            let matched = member_matched || installed_satisfies;
            let use_deps: Vec<portage_dep::UseDep> = parsed
                .use_deps
                .clone()
                .filter(|deps| !deps.is_empty())
                .unwrap_or_default();
            if matched {
                if use_deps.is_empty() {
                    continue;
                }
                // Installed-only satisfaction with no plan member: the vdb USE
                // already held (`best_installed_matching` above), so the edge
                // is satisfied like the walk's `AlreadyInstalled`.
                let Some(child) = members.iter().find(|candidate| {
                    format!("{}/{}", candidate.entry.category, candidate.entry.package)
                        == format!("{}/{}", parsed.category, parsed.package)
                }) else {
                    continue;
                };
                let mut child_use = child.use_set.clone();
                if let Some(applied) =
                    flips.get(&format!("{}/{}", child.entry.category, child.entry.package))
                {
                    for (flag, enabled) in applied {
                        if *enabled {
                            child_use.insert(flag.clone());
                        } else {
                            child_use.remove(flag);
                        }
                    }
                }
                let iuse_declared: HashSet<String> = child
                    .record
                    .iuse
                    .split_whitespace()
                    .map(|token| token.trim_start_matches(['+', '-']).to_string())
                    .collect();
                if portage_dep::use_deps_satisfied(
                    &use_deps,
                    &valid_iuse(&iuse_declared, &req.config),
                    &child_use,
                ) {
                    continue;
                }
                // The `entry_use_flags` predicate verbatim: an unsatisfied
                // plain USE-dep with a settable flip becomes a `package.use`
                // change (the need_config_change gate); otherwise a miss.
                let flip = list_candidates(&lazy.repos, &parsed.category, &parsed.package)
                    .ok()
                    .and_then(|candidates| {
                        candidates
                            .iter()
                            .find(|candidate| candidate.version == child.version)
                            .and_then(|candidate| {
                                suggested_use_flip(
                                    candidate,
                                    &parsed.category,
                                    &parsed.package,
                                    &use_deps,
                                    &req.config,
                                )
                                .map(|flip| (candidate.clone(), flip))
                            })
                    });
                if let Some((candidate, flip)) = flip
                    && req.autounmask_suggest_use
                {
                    let token = flip
                        .iter()
                        .map(|(flag, enabled)| {
                            if *enabled {
                                flag.clone()
                            } else {
                                format!("-{flag}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let candidates =
                        list_candidates(&lazy.repos, &parsed.category, &parsed.package)
                            .map(|arc| arc.as_ref().clone())
                            .unwrap_or_default();
                    changes.push(AutounmaskChange {
                        atom: autounmask_use_atom_form(
                            &candidate,
                            &candidates,
                            &parsed.category,
                            &parsed.package,
                            &req.config,
                        ),
                        token,
                        dep_chain: Vec::new(),
                        trigger: Some(owner.clone()),
                    });
                    flips
                        .entry(format!("{}/{}", parsed.category, parsed.package))
                        .or_default()
                        .extend(flip);
                    continue;
                }
            }
            match misses
                .iter_mut()
                .find(|miss| miss.category == parsed.category && miss.package == parsed.package)
            {
                Some(miss) => {
                    if !miss.owners.contains(&owner) {
                        miss.owners.push(owner.clone());
                    }
                }
                None => misses.push(Miss {
                    category: parsed.category.clone(),
                    package: parsed.package.clone(),
                    display: dep.atom.clone(),
                    evaluated: dep.evaluated.clone(),
                    owners: vec![owner.clone()],
                }),
            }
        }
    }
    if changes.is_empty() && misses.is_empty() {
        return Ok(None);
    }
    // (b) miss disclosure, one report per `cat/pkg` exactly like
    // `record_miss_disclosure` (masked first, then `[use]`, then plain).
    let mut validation = BridgeAbortValidation {
        autounmask_use_changes: changes,
        masked_deps: Vec::new(),
        use_unsat_deps: Vec::new(),
        plain_miss_deps: Vec::new(),
        nvc_entries: Vec::new(),
        nvc_dep_atoms: HashMap::new(),
    };
    for miss in &misses {
        if let Some(masked) = masked_candidates_for_atom(&lazy.repos, &miss.display, &req.config) {
            validation.masked_deps.push(MaskedDepReport {
                category: miss.category.clone(),
                package: miss.package.clone(),
                atom: miss.display.clone(),
                masked,
                chain: Vec::new(),
            });
        } else if let Some(rows) = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
            repos: &lazy.repos,
            root: &req.root,
            entries,
            display_atom: &miss.display,
            evaluated_atom: &miss.evaluated,
            owner: miss.owners.first(),
            config: &req.config,
        }) {
            validation.use_unsat_deps.push(UseUnsatDepReport {
                category: miss.category.clone(),
                package: miss.package.clone(),
                atom: miss.display.clone(),
                rows,
                chain: Vec::new(),
            });
        } else {
            validation.plain_miss_deps.push(PlainMissDepReport {
                category: miss.category.clone(),
                package: miss.package.clone(),
                atom: miss.display.clone(),
                chain: Vec::new(),
                binary_only: req.usepkgonly,
            });
        }
        let mut owners = miss.owners.clone();
        owners.sort();
        owners.dedup();
        validation.nvc_entries.push(GraphEntry {
            discovery: 0,
            category: miss.category.clone(),
            package: miss.package.clone(),
            outcome: PretendOutcome::NoVisibleCandidate,
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
            required_by: owners,
            source: super::CandidateSource::Ebuild,
            provenance: super::VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        });
        validation.nvc_dep_atoms.insert(
            (miss.category.clone(), miss.package.clone()),
            miss.display.clone(),
        );
    }
    Ok(Some(validation))
}

/// S9 (#363/#364/#365): file a [`BridgeAbortValidation`] into a
/// `graph_result_from_order` result: NVC entries join the graph, report
/// and change chains walk out of the final entries, and the outcome
/// re-derives through `abort_outcome` exactly like the settled walk.
fn bridge_apply_validation(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    result: &mut GraphResult,
    validation: BridgeAbortValidation,
) {
    result.entries.extend(validation.nvc_entries);
    result.masked_deps = validation.masked_deps;
    result.use_unsat_deps = validation.use_unsat_deps;
    result.plain_miss_deps = validation.plain_miss_deps;
    let top_level = TopLevel {
        atoms: req.atoms.iter().map(String::as_str).collect(),
        sets: &req.argument_sets,
    };
    for report in &mut result.masked_deps {
        let atom = report.atom.clone();
        report.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &lazy.repos,
            config: &req.config,
            entries: &result.entries,
            category: &report.category,
            package: &report.package,
            atoms: &req.atoms,
            argument_sets: &req.argument_sets,
            root: &req.root,
            atom: &atom,
        });
    }
    for report in &mut result.use_unsat_deps {
        let atom = report.atom.clone();
        report.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &lazy.repos,
            config: &req.config,
            entries: &result.entries,
            category: &report.category,
            package: &report.package,
            atoms: &req.atoms,
            argument_sets: &req.argument_sets,
            root: &req.root,
            atom: &atom,
        });
    }
    for report in &mut result.plain_miss_deps {
        let atom = report.atom.clone();
        report.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &lazy.repos,
            config: &req.config,
            entries: &result.entries,
            category: &report.category,
            package: &report.package,
            atoms: &req.atoms,
            argument_sets: &req.argument_sets,
            root: &req.root,
            atom: &atom,
        });
    }
    result.autounmask_use_changes = validation.autounmask_use_changes;
    fill_use_change_chains(
        &result.entries,
        &mut result.autounmask_use_changes,
        &top_level,
        &req.root,
    );
    // A bare USE-change gate keeps `Complete` (Portage fails it later in
    // `_display_autounmask`); a disclosed miss aborts like the walk.
    if !validation.nvc_dep_atoms.is_empty() {
        result.outcome = abort_outcome(AbortOutcomeArgs {
            entries: &result.entries,
            masked_deps: &result.masked_deps,
            circular_deps: &result.circular_deps,
            cycle_display: &result.cycle_display,
            cycle_display_idx: &[],
            nvc_dep_atoms: &validation.nvc_dep_atoms,
            virtual_cycle: &[],
        });
    }
}

fn resolve_pubgrub(req: &ResolveRequest) -> Result<GraphResult, String> {
    let repos = find_repos(&req.config_root).map_err(|e| format!("finding repos: {e}"))?;
    let (seeds, shallow) = closure_seeds(req)?;
    let seed_cpns: Vec<portage_solver::Cpn> = seeds
        .iter()
        .chain(shallow.iter())
        .filter_map(|cp| portage_solver::Cpn::parse(cp).ok())
        .collect();
    let lazy = LazyRepo::build(req, repos, &seeds, &shallow, true)
        .map_err(|e| format!("reading repo facts: {e}"))?;
    // The kept/rebuild set is computed before the provider exists (deps are
    // converted inside `new_for_targets`).
    let installed = all_installed_packages(&req.root);
    let mut kept = KeptInstalled::of(req, &lazy, &installed);
    // Root-target cps for the `||` pass: an alternative naming the probe
    // target itself is in-graph trivially (the texinfo/emacs shape).
    let mut root_cps = HashSet::new();
    for atom in &req.atoms {
        let Ok(dep) = portage_solver::Dep::parse(atom) else {
            continue;
        };
        if dep.blocker.is_some() {
            continue;
        }
        root_cps.insert(format!("{}", dep.cpn));
    }
    // #34 S5 G2 (round 2) fixpoint: the parent-USE reinstall pass needs
    // the solved plan -- only solution members whose deps expand exert
    // USE pressure on an installed child -- and each new rebuild
    // member's expanded deps can pull further parents in.
    // `apply_parent_use_reinstalls` only grows `rebuild`, so that half of
    // the loop terminates when a pass adds nothing.
    // #34 S5 G5/G6: the `||` reorder/drop pass rides the same loop -- a
    // solve whose `||` groups resolve away from the already-in-graph
    // branch re-solves with that branch first (or the installed wrong
    // pick dropped) until the override set stops changing. Overrides are
    // not monotonic, so an override signature repeating means the loop
    // oscillates and the previous plan wins.
    let mut overrides = OrChoiceOverrides::new();
    let mut seen_override_sigs: HashSet<String> = HashSet::new();
    let mut previous: Option<SolvedRound> = None;
    let mut rounds = 0;
    let (order, parents) = loop {
        rounds += 1;
        let mut provider = pubgrub_provider(req, &lazy, &kept, &installed, &seed_cpns, &overrides);
        let targets = pubgrub_targets(req, &lazy, &installed, &provider)?;
        let solution = match provider.resolve_targets(targets) {
            Ok(solution) => solution,
            Err(engine_error) => {
                // An override pass that breaks the solve falls back to the
                // previous plan (the dropped branch was load-bearing); the
                // first pass has no previous plan and reports the error.
                if let Some((order, parents)) = previous {
                    break (order, parents);
                }
                return Err(portage_atom_pubgrub::format_solve_error(engine_error));
            }
        };
        let mut order = Vec::new();
        for (pkg, ver) in provider.install_order(&solution) {
            if let portage_atom_pubgrub::PortagePackage::Real { cpn, .. } = &pkg {
                pg_trace(&mut std::io::stderr(), format_args!("pg-plan {cpn}-{ver}"));
                order.push((format!("{cpn}"), format!("{ver}")));
            }
        }
        let mut parents: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for edge in provider.dependency_graph(&solution) {
            let portage_atom_pubgrub::PortagePackage::Real { cpn: from_cpn, .. } = &edge.from.0
            else {
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
        let before = kept.rebuild.len();
        let plan = SolvedPlan {
            members: order.iter().cloned().collect(),
            taken: solution_taken_edges(&parents),
        };
        kept.apply_parent_use_reinstalls(req, &lazy, &installed, &plan);
        let grew = kept.rebuild.len() != before;
        let fresh = compute_or_overrides(
            req, &lazy, &kept, &installed, &order, &parents, &root_cps, &overrides,
        );
        if !grew && fresh == overrides {
            break (order, parents);
        }
        if fresh != overrides {
            let signature = fresh.signature();
            if !seen_override_sigs.insert(signature)
                || seen_override_sigs.len() > OR_FIXPOINT_MAX_PASSES
            {
                pg_trace(
                    &mut std::io::stderr(),
                    format_args!("pg-or-oscillate fallback"),
                );
                if let Some((order, parents)) = previous {
                    break (order, parents);
                }
                break (order, parents);
            }
            overrides = fresh;
        }
        if rounds >= PUBGRUB_FIXPOINT_MAX_ROUNDS {
            pg_trace(
                &mut std::io::stderr(),
                format_args!("pg-or-round-cap fallback"),
            );
            if let Some((order, parents)) = previous {
                break (order, parents);
            }
            break (order, parents);
        }
        previous = Some((order, parents));
    };
    let mut result =
        graph_result_from_order(req, &lazy.repos, &lazy, &order, &parents, Some(&kept));
    // S9 (#363/#364/#365): the engine never sees REQUIRED_USE, masked
    // disclosure or the autounmask gate, so replay them read-only over the
    // solved plan; passing probes validate clean and keep their output.
    if let Some(validation) = bridge_validate_aborts(req, &lazy, &parents, &result.entries)? {
        bridge_apply_validation(req, &lazy, &mut result, validation);
    }
    Ok(result)
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
    // Old queue order: S2's seeds-first reorder stays behind the pubgrub path,
    // so the resolvo pool -- and its output -- does not move.
    let lazy = LazyRepo::build(req, repos, &seeds, &shallow, false)
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
    // Resolvo has no kept/rebuild set: its output does not move.
    Ok(graph_result_from_order(
        req,
        &lazy.repos,
        &lazy,
        &order,
        &parents,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::{SolverKind, active_resolver_for};
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

    #[test]
    fn active_resolver_for_selects_all_three_backends() {
        // Every kind yields a resolver; behaviour is pinned elsewhere.
        let _portage = active_resolver_for(SolverKind::Portage);
        let _pubgrub = active_resolver_for(SolverKind::PubGrub);
        let _resolvo = active_resolver_for(SolverKind::Resolvo);
    }

    fn fixture_request(atoms: &[&str]) -> super::super::ResolveRequest {
        use super::super::{Deep, ResolveRequest, SolverKind};
        use std::collections::HashMap;
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
            argument_sets: HashMap::new(),
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
            complete_reuse: HashMap::new(),
            set_args: std::collections::HashSet::new(),
            solver: SolverKind::Portage,
        }
    }

    /// The gated trace names each installed package (`PORTUALE_PG_TRACE=1`;
    /// silent without it).
    #[test]
    fn pg_trace_emits_the_installed_line() {
        use std::io::Write as _;
        let dir = TempDir::new("portage-bridge-pgtrace").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let d = dir.join("var/db/pkg/dev-libs/pgtrecord-1.0");
        std::fs::create_dir_all(&d).unwrap();
        for (file, content) in [
            ("CATEGORY", "dev-libs\n"),
            ("SLOT", "0\n"),
            ("repository", "testrepo\n"),
        ] {
            let mut f = std::fs::File::create(d.join(file)).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        let mut req = fixture_request(&["dev-libs/newpkg"]);
        req.root = dir.to_path_buf();
        let installed = super::super::all_installed_packages(&req.root);
        assert_eq!(
            installed.len(),
            1,
            "expected one vdb package: {installed:?}"
        );
        // `unsafe`: single-threaded test contract, no other test reads this var.
        unsafe {
            std::env::set_var("PORTUALE_PG_TRACE", "1");
        }
        let mut buf = Vec::new();
        for inst in &installed {
            super::pg_trace(
                &mut buf,
                format_args!(
                    "{}",
                    super::pg_installed_line(inst, portage_atom_pubgrub::InstalledPolicy::Favor)
                ),
            );
        }
        unsafe {
            std::env::remove_var("PORTUALE_PG_TRACE");
        }
        let text = String::from_utf8(buf).expect("trace is utf-8");
        assert!(
            text.contains("pg-installed dev-libs/pgtrecord-1.0 slot=0 policy=Favor"),
            "unexpected trace: {text}"
        );
        // Silent without the env var.
        let mut buf = Vec::new();
        for inst in &installed {
            super::pg_trace(
                &mut buf,
                format_args!(
                    "{}",
                    super::pg_installed_line(inst, portage_atom_pubgrub::InstalledPolicy::Favor)
                ),
            );
        }
        assert!(buf.is_empty(), "trace leaked without PORTUALE_PG_TRACE=1");
        let _ = std::fs::remove_dir_all(&dir);
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
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
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
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
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
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
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

    /// Unsatisfiable targets render the engine-native conflict naming the
    /// packages, never the internals dump.
    #[test]
    fn resolvo_failure_text_is_engine_native_not_a_debug_dump() {
        use super::super::active_resolver_for;
        let mut req = fixture_request(&[">=dev-libs/newpkg-99.0"]);
        req.solver = super::super::SolverKind::Resolvo;
        let err = active_resolver_for(super::super::SolverKind::Resolvo)
            .resolve(&req)
            .expect_err("nothing provides newpkg-99.0");
        let msg = err.to_string();
        assert!(
            msg.contains("dev-libs/newpkg"),
            "names the conflicted package: {msg}"
        );
        assert!(
            !msg.contains("ClauseId") && !msg.contains("Unsolvable("),
            "no engine-internals debug dump leaks out: {msg}"
        );
    }

    /// Bridge entries report matched blockers through the shared walk-path helper,
    /// in the walk's own shapes.
    #[test]
    fn bridge_entries_report_matched_blockers() {
        use super::super::active_resolver_for;
        let mut req = fixture_request(&["dev-libs/graphblockerparent"]);
        req.solver = super::super::SolverKind::PubGrub;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("graphblockerparent solves");
        let weak = result
            .entries
            .iter()
            .find(|e| e.package == "weakblockerpkg")
            .expect("weakblockerpkg entry");
        assert_eq!(
            weak.blockers,
            vec![super::super::BlockerConflict {
                atom_str: "!dev-libs/blockerpartnerpkg".to_string(),
                strong: false,
                matched_category: "dev-libs".to_string(),
                matched_package: "blockerpartnerpkg".to_string(),
                matched_version: "1.0".to_string(),
                // Merge-bound match with a merging parent: unresolved.
                unsolvable: true,
                satisfied_by: None,
                tree_scheduled_uninstall: false,
            }]
        );

        for kind in [
            super::super::SolverKind::PubGrub,
            super::super::SolverKind::Resolvo,
        ] {
            let mut req = fixture_request(&["dev-libs/blockerpkg"]);
            req.solver = kind;
            let result = active_resolver_for(kind)
                .resolve(&req)
                .expect("blockerpkg solves");
            let entry = result
                .entries
                .iter()
                .find(|e| e.package == "blockerpkg")
                .expect("blockerpkg entry");
            assert_eq!(
                entry.blockers,
                vec![super::super::BlockerConflict {
                    atom_str: "!!dev-libs/samepkg".to_string(),
                    strong: true,
                    matched_category: "dev-libs".to_string(),
                    matched_package: "samepkg".to_string(),
                    matched_version: "1.0".to_string(),
                    // Not walked graph nodes, so Portage uninstalls the match.
                    unsolvable: false,
                    satisfied_by: Some(super::super::BlockerSatisfiedBy::Uninstall {
                        cpv: "dev-libs/samepkg-1.0".to_string(),
                        anchor: ("dev-libs".to_string(), "blockerpkg".to_string()),
                    }),
                    tree_scheduled_uninstall: false,
                }]
            );
        }
    }

    /// Plans moving a slot's sub-slot reinstall stale `:=` consumers via the
    /// walk's fixpoint, recording the `(provider, consumer)` pair.
    #[test]
    fn bridge_plans_schedule_stale_equals_consumer_reinstalls() {
        use std::io::Write as _;
        let dir = TempDir::new("portage-bridge-slotop").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let mk = |name: &str, files: &[(&str, &str)]| {
            let d = dir.join("var/db/pkg/dev-libs").join(name);
            std::fs::create_dir_all(&d).unwrap();
            for (file, content) in files {
                let mut f = std::fs::File::create(d.join(file)).unwrap();
                f.write_all(content.as_bytes()).unwrap();
            }
        };
        mk(
            "slotbindtarget-1.0",
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", "2\n"),
                ("repository", "testrepo\n"),
            ],
        );
        mk(
            "slotbindconsumer-1.0",
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", "0\n"),
                ("repository", "testrepo\n"),
                ("RDEPEND", "dev-libs/slotbindtarget:2/2=\n"),
            ],
        );
        mk(
            "slotbindfresh-1.0",
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", "0\n"),
                ("repository", "testrepo\n"),
                ("RDEPEND", "dev-libs/slotbindtarget:2/9=\n"),
            ],
        );
        let mut req = fixture_request(&["dev-libs/slotbindtarget"]);
        req.root = dir.to_path_buf();
        req.solver = super::super::SolverKind::PubGrub;
        req.rebuild_if_new_slot = true;
        req.config.complete_seed_atoms = vec![
            "dev-libs/slotbindconsumer".to_string(),
            "dev-libs/slotbindfresh".to_string(),
        ];
        let result = super::super::active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("slotbindtarget resolves");
        let by_pkg = |p: &str| {
            result
                .entries
                .iter()
                .find(|e| e.package == p)
                .unwrap_or_else(|| {
                    panic!(
                        "no {p} entry in {:?}",
                        result
                            .entries
                            .iter()
                            .map(|e| format!("{}/{}", e.category, e.package))
                            .collect::<Vec<_>>()
                    )
                })
        };
        // Only the stale consumer reinstalls as a slot-operator rebuild.
        assert!(matches!(
            by_pkg("slotbindtarget").outcome,
            super::super::PretendOutcome::Upgrade { .. }
        ));
        let consumer = by_pkg("slotbindconsumer");
        assert!(
            matches!(
                consumer.outcome,
                super::super::PretendOutcome::Reinstall {
                    slot_operator_rebuild: true,
                    ..
                }
            ),
            "unexpected outcome: {:?}",
            consumer.outcome
        );
        assert!(!result.abi_rebuilds.is_empty());
        assert!(
            result.entries.iter().all(|e| e.package != "slotbindfresh"),
            "the fresh :2/9= consumer is already bound correctly -- never rebuilt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Bridge entries carry walk-derived `deps` edges, so the shared sort -- not
    /// engine install order -- decides the display order.
    #[test]
    fn bridge_entries_carry_merge_order_dep_edges() {
        for kind in [
            super::super::SolverKind::PubGrub,
            super::super::SolverKind::Resolvo,
        ] {
            let mut req = fixture_request(&["dev-libs/diamond"]);
            req.solver = kind;
            let result = active_resolver_for(kind)
                .resolve(&req)
                .expect("diamond resolves");
            assert_eq!(result.entries.len(), 4);
            let diamond = result
                .entries
                .iter()
                .find(|e| e.package == "diamond")
                .expect("diamond entry");
            let mut dep_pkgs: Vec<&str> = diamond.deps.iter().map(|d| d.package.as_str()).collect();
            dep_pkgs.sort();
            dep_pkgs.dedup();
            assert_eq!(dep_pkgs, vec!["shared-a", "shared-b"]);
        }
    }

    /// Both backends render the walk's forced/masked `( )` wraps (same fixture
    /// package the walk-path contract pins).
    #[test]
    fn bridge_use_display_wraps_forced_and_masked_flags() {
        for kind in [
            super::super::SolverKind::PubGrub,
            super::super::SolverKind::Resolvo,
        ] {
            let mut req = fixture_request(&["dev-libs/pkgusemaskforcepkg"]);
            req.solver = kind;
            // One synthetic repo level forcing `forceflag` and masking `maskflag`.
            req.config.use_mask_force_levels = vec![portage_profile::UseMaskForceLevel {
                package_use_force: vec![(
                    "dev-libs/pkgusemaskforcepkg".to_string(),
                    vec!["forceflag".to_string()],
                )],
                package_use_mask: vec![(
                    "dev-libs/pkgusemaskforcepkg".to_string(),
                    vec!["maskflag".to_string()],
                )],
                ..Default::default()
            }];
            let result = active_resolver_for(kind)
                .resolve(&req)
                .expect("pkgusemaskforcepkg resolves");
            assert_eq!(result.entries.len(), 1);
            let body: String = result.entries[0]
                .use_expand_display
                .iter()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                body.contains("(forceflag)"),
                "forced flag unwrapped ({kind:?}): {body}"
            );
            assert!(
                body.contains("(-maskflag)"),
                "masked flag unwrapped ({kind:?}): {body}"
            );
        }
    }

    /// The bridge reports the unbreakable build-time cycle through the walk's own
    /// map; resolvo cannot order a cyclic closure and errors first.
    #[test]
    fn bridge_plan_reports_the_unbreakable_build_time_cycle() {
        let mut req = fixture_request(&["dev-libs/hardcyclea"]);
        req.solver = super::super::SolverKind::PubGrub;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("hardcyclea solves under pubgrub");
        assert_eq!(result.circular_deps.len(), 1);
        let cycle = &result.circular_deps[0];
        assert!(
            cycle.contains(&"dev-libs/hardcyclea-1.0".to_string())
                && cycle.contains(&"dev-libs/hardcycleb-1.0".to_string()),
            "unexpected cycle: {cycle:?}"
        );

        let mut req = fixture_request(&["dev-libs/hardcyclea"]);
        req.solver = super::super::SolverKind::Resolvo;
        assert!(
            active_resolver_for(super::super::SolverKind::Resolvo)
                .resolve(&req)
                .is_err(),
            "resolvo still cannot order a cyclic closure (Tier-4 item)"
        );
    }

    /// Merge-bound `cpv:slot` strings, sorted, for the pubgrub S1 pins.
    fn pg34_plan(result: &super::super::GraphResult) -> Vec<String> {
        let mut planned: Vec<String> = result
            .entries
            .iter()
            .filter_map(|e| {
                super::super::merge_bound_cpv(e)
                    .map(|cpv| format!("{cpv}:{}", e.slot.clone().unwrap_or_default()))
            })
            .collect();
        planned.sort();
        planned
    }

    /// #34 S1: a bare multi-slot target plans only its best visible slot
    /// (Portage `_greedy_slots`); the bridge still plans slot 1 as well.
    #[test]
    fn pg34_sib_pubgrub_plans_best_slot_only() {
        let mut req = fixture_request(&["dev-libs/pg34-sib"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-sib resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-sib-2:2"]);
    }

    /// #34 S1: without `--update` no installed slot joins the plan
    /// (Portage `_greedy_slots`); the bridge still upgrades slot 1.
    #[test]
    fn pg34_gs_pubgrub_pv_plans_new_slot_only() {
        let mut req = fixture_request(&["dev-libs/pg34-gs"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-gs resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-gs-2:2"]);
    }

    /// #34 S1: with `--update` the installed slot upgrades in place.
    #[test]
    fn pg34_gs_pubgrub_pvu_upgrades_installed_slot() {
        let mut req = fixture_request(&["dev-libs/pg34-gs"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pvu`: selective through `--update`, build deps on.
        req.selective = true;
        req.update = true;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-gs resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-gs-1.1:1", "dev-libs/pg34-gs-2:2"]
        );
    }

    /// #34 S2: request + closure + installed set for the pg34 arg/app fixtures.
    /// `with_bdeps` on (like `-pv`); each test sets `selective`/`update`/`empty`
    /// explicitly before resolving or computing `KeptInstalled::of`, mirroring
    /// the CLI fold (`resolve_plan.rs`: selective = update||newuse||…||noreplace
    /// ||newrepo; `-pve` keeps request-level selective, the walk clears it).
    /// `fixture_request` defaults `selective` and `with_bdeps` to `false`.
    fn pg34_s2_parts(
        atoms: &[&str],
    ) -> (
        super::super::ResolveRequest,
        super::LazyRepo,
        Vec<super::super::InstalledPackage>,
    ) {
        use super::super::{SolverKind, find_repos};
        let mut req = fixture_request(atoms);
        req.solver = SolverKind::PubGrub;
        // Build deps on in every mode; selective/update/empty per test.
        req.with_bdeps = true;
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        (req, lazy, installed)
    }

    /// #34 S2: the kept/rebuild cpv set. Non-selective `-pv` rebuilds the
    /// installed best accepted version of the root target; selective `-pvn`
    /// rebuilds nothing; `-pve` rebuilds every installed cpv.
    #[test]
    fn pg34_arg_kept_rebuild_set_follows_selectivity() {
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pv`: not selective, no update.
        req.selective = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.installed.contains("dev-libs/pg34-arg-1"),
            "the vdb cpv is kept"
        );
        assert_eq!(
            kept.rebuild,
            std::collections::HashSet::from(["dev-libs/pg34-arg-1".to_string()]),
            "only the named installed target rebuilds under -pv"
        );
        assert!(!kept.deep, "no --deep under -pv");

        // `-pvn`: selective through `--noreplace`.
        req.selective = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.rebuild.is_empty(),
            "selective -pvn rebuilds nothing: {:?}",
            kept.rebuild
        );

        // `-pve`: the request level keeps selective (the walk clears it);
        // `empty` alone drives the rebuild set.
        req.empty = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-arg-1")
                && kept.rebuild.contains("dev-libs/pg34-lib-1"),
            "emptytree rebuilds every installed cpv"
        );
        assert!(kept.deep, "--emptytree counts as deep");
    }

    /// #34 S2: the named installed target registers `InstalledPolicy::Rebuild`
    /// (a selective resolve keeps `Favor`); an installed non-target stays `Favor`.
    #[test]
    fn pg34_arg_installed_policy_rebuild_for_named_target_only() {
        use portage_atom_pubgrub::InstalledPolicy;
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pv`: not selective, no update.
        req.selective = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let seed_cpns: Vec<portage_solver::Cpn> = seeds
            .iter()
            .chain(shallow.iter())
            .filter_map(|cp| portage_solver::Cpn::parse(cp).ok())
            .collect();
        let provider = super::pubgrub_provider(
            &req,
            &lazy,
            &kept,
            &installed,
            &seed_cpns,
            &super::OrChoiceOverrides::new(),
        );
        let slotted = |cp: &str, slot: &str| {
            portage_atom_pubgrub::PortagePackage::slotted(
                portage_solver::Cpn::parse(cp).expect("cp parses"),
                super::interned(slot),
            )
        };
        assert_eq!(
            provider.installed_policy(&slotted("dev-libs/pg34-arg", "0")),
            Some(InstalledPolicy::Rebuild)
        );
        assert_eq!(
            provider.installed_policy(&slotted("dev-libs/pg34-lib", "0")),
            Some(InstalledPolicy::Favor),
            "an installed non-target is kept, not rebuilt"
        );

        // `-pvn`: selective through `--noreplace`.
        req.selective = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let provider = super::pubgrub_provider(
            &req,
            &lazy,
            &kept,
            &installed,
            &seed_cpns,
            &super::OrChoiceOverrides::new(),
        );
        assert_eq!(
            provider.installed_policy(&slotted("dev-libs/pg34-arg", "0")),
            Some(InstalledPolicy::Favor),
            "selective -pvn keeps Favor"
        );
    }

    /// #34 S2: `-pv` on the installed named argument plans `N pg34-new` +
    /// a bare, reasonless `R pg34-arg` (Portage `depgraph.py` named-argument
    /// reinstall); the rebuilt target's RDEPEND expands, so pg34-new appears.
    #[test]
    fn pg34_arg_pubgrub_pv_reinstalls_named_argument() {
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-arg resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-arg-1:0", "dev-libs/pg34-new-1:0"]
        );
        let new = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-new")
            .expect("pg34-new entry");
        assert!(
            matches!(new.outcome, super::super::PretendOutcome::New { .. }),
            "unexpected outcome: {:?}",
            new.outcome
        );
        let arg = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-arg")
            .expect("pg34-arg entry");
        match &arg.outcome {
            super::super::PretendOutcome::Reinstall {
                version,
                changed_flags,
                deps_changed,
                slot_changed,
                rebuilt_binary,
                new_repo,
                slot_operator_rebuild,
            } => {
                assert_eq!(version, "1");
                assert!(changed_flags.is_empty());
                assert!(
                    !deps_changed
                        && !slot_changed
                        && !rebuilt_binary
                        && !new_repo
                        && !slot_operator_rebuild,
                    "bare reinstall carries no reason"
                );
            }
            other => panic!("expected bare Reinstall, got {other:?}"),
        }
    }

    /// #34 S2: selective `-pvn` on the up-to-date named argument plans nothing:
    /// the kept target is a leaf, so its not-installed dep never enters the plan.
    #[test]
    fn pg34_arg_pubgrub_pvn_selective_stays_empty() {
        // `-pvn`: selective through `--noreplace`, build deps on.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pvn`: selective through `--noreplace`, build deps on.
        req.selective = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-arg resolves");
        assert!(
            result.entries.iter().all(|e| matches!(
                e.outcome,
                super::super::PretendOutcome::AlreadyInstalled { .. }
            )),
            "nothing merges under -pvn: {:?}",
            result
                .entries
                .iter()
                .map(|e| format!("{}/{} {:?}", e.category, e.package, e.outcome))
                .collect::<Vec<_>>()
        );
    }

    /// #34 S2: `-pvu` on the up-to-date named argument plans nothing either.
    #[test]
    fn pg34_arg_pubgrub_pvu_update_stays_empty() {
        // `-pvu`: selective through `--update`, build deps on.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pvu`: selective through `--update`, build deps on.
        req.selective = true;
        req.update = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-arg resolves");
        assert!(
            result.entries.iter().all(|e| matches!(
                e.outcome,
                super::super::PretendOutcome::AlreadyInstalled { .. }
            )),
            "nothing merges under -pvu: {:?}",
            result
                .entries
                .iter()
                .map(|e| format!("{}/{} {:?}", e.category, e.package, e.outcome))
                .collect::<Vec<_>>()
        );
    }

    /// #34 S2: `-pve` rebuilds the named argument and its new dep.
    #[test]
    fn pg34_arg_pubgrub_pve_emptytree_reinstalls() {
        // `-pve`: the request level keeps selective (the walk clears it);
        // `empty` alone drives the rebuild set.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // `-pve`: `empty` alone drives the rebuild set.
        req.empty = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-arg resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-arg-1:0", "dev-libs/pg34-new-1:0"]
        );
        let arg = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-arg")
            .expect("pg34-arg entry");
        assert!(
            matches!(arg.outcome, super::super::PretendOutcome::Reinstall { .. }),
            "unexpected outcome: {:?}",
            arg.outcome
        );
    }

    /// #34 S2: `-pv` on pg34-app plans only the new target. The installed dep
    /// pg34-lib is a kept leaf, so transitive pg34-extra never enters the plan
    /// (non-regression pin: this cell already matches Portage).
    #[test]
    fn pg34_app_pubgrub_pv_keeps_installed_dep_a_leaf() {
        // `-pv`: not selective, no update, build deps on.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-app resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-app-1:0"]);
    }

    /// #34 S2: `-pvu` on pg34-app plans only the new target (non-regression pin).
    #[test]
    fn pg34_app_pubgrub_pvu_keeps_installed_dep_a_leaf() {
        // `-pvu`: selective through `--update`, build deps on.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        // `-pvu`: selective through `--update`, build deps on.
        req.selective = true;
        req.update = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-app resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-app-1:0"]);
    }

    /// #34 S3: request for the `-pvuD` cells. `selective`/`update`/`deep` set
    /// explicitly like the S2 helper; `dynamic_deps` on like the CLI default;
    /// the fixture `package.use` line (`dev-libs/pg34-ulib x`) the container
    /// captures were taken with (`fixture_request` builds a bare config, so
    /// without it the config USE and the vdb USE agree and the test is vacuous).
    fn pg34_s3_request(atoms: &[&str]) -> super::super::ResolveRequest {
        use super::super::{Deep, SolverKind};
        let mut req = fixture_request(atoms);
        req.solver = SolverKind::PubGrub;
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        req.selective = true;
        req.update = true;
        req.deep = Deep::Unlimited;
        req.with_bdeps = true;
        req.dynamic_deps = true;
        req.config
            .package_use
            .push(("dev-libs/pg34-ulib".to_string(), vec!["x".to_string()]));
        req
    }

    /// #34 S3: under deep, an installed cpv answers the vdb USE, not the
    /// config USE. `pg34-ulib` is installed with `USE=""` while the config
    /// enables `x` (`package.use`); without the vdb answer its `x?` dep on
    /// `pg34-uextra` wrongly enters the `-pvuD` plan.
    #[test]
    fn pg34_uapp_desired_use_is_vdb_use_under_deep() {
        use portage_atom_pubgrub::PackageRepository;
        let req = pg34_s3_request(&["dev-libs/pg34-uapp"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(kept.deep, "the S3 request counts as deep");
        let bridge = super::BridgeRepo {
            lazy,
            kept,
            installed: installed.clone(),
            or_overrides: super::OrChoiceOverrides::new(),
        };
        let cpv = portage_solver::Cpv::parse("dev-libs/pg34-ulib-1").expect("cpv parses");
        let use_config = bridge.desired_use(&cpv);
        assert!(
            !use_config.enabled_flags().contains(&super::interned("x")),
            "installed pg34-ulib was built with USE=\"\": x must stay off"
        );
    }

    /// #34 S3: without deep the config USE still answers (the S2 leaf rule
    /// empties the deps, so the flag value is unobservable there; this pins
    /// the reader choice itself).
    #[test]
    fn pg34_uapp_desired_use_is_config_use_without_deep() {
        use portage_atom_pubgrub::PackageRepository;
        let mut req = pg34_s3_request(&["dev-libs/pg34-uapp"]);
        req.deep = super::super::Deep::NotRequested;
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(!kept.deep, "no --deep in this request");
        let bridge = super::BridgeRepo {
            lazy,
            kept,
            installed: installed.clone(),
            or_overrides: super::OrChoiceOverrides::new(),
        };
        let cpv = portage_solver::Cpv::parse("dev-libs/pg34-ulib-1").expect("cpv parses");
        let use_config = bridge.desired_use(&cpv);
        assert!(
            use_config.enabled_flags().contains(&super::interned("x")),
            "without deep the package.use x flag answers"
        );
    }

    /// #34 S3: `versions_for` offers an installed version absent from the repo
    /// with its vdb deps (`pg34-gone` → `pg34-gextra`), instead of leaving the
    /// crate's empty-deps stub in place.
    #[test]
    fn pg34_gone_versions_for_offers_vdb_deps() {
        use portage_atom_pubgrub::PackageRepository;
        let req = pg34_s3_request(&["dev-libs/pg34-gapp"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let bridge = super::BridgeRepo {
            lazy,
            kept,
            installed: installed.clone(),
            or_overrides: super::OrChoiceOverrides::new(),
        };
        let cpn = portage_solver::Cpn::parse("dev-libs/pg34-gone").expect("cpn parses");
        let rows = bridge.versions_for(&cpn);
        assert_eq!(rows.len(), 1, "the installed-only version is offered");
        assert_eq!(format!("{}", rows[0].0), "dev-libs/pg34-gone-1");
        assert_eq!(rows[0].1.slot, Some(super::interned("0")));
        let mut refs = Vec::new();
        super::referenced_cpns(&rows[0].1.deps.rdepend, &mut refs);
        assert_eq!(refs, ["dev-libs/pg34-gextra"]);
    }

    /// #34 S3: `-pvuD` on pg34-uapp plans only the new target. The installed
    /// dep keeps its vdb `USE=""`, so the `x?` dep on pg34-uextra stays off
    /// (Portage `N pg34-uapp` only; pubgrub used to add `N pg34-uextra` from
    /// the config USE).
    #[test]
    fn pg34_uapp_pubgrub_pvud_uses_vdb_use() {
        let req = pg34_s3_request(&["dev-libs/pg34-uapp"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-uapp resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-uapp-1:0"]);
    }

    /// #34 S3: `-pvuD` on pg34-gapp plans the installed-only dep's own dep.
    /// `pg34-gone` is installed but not in the repo; Portage expands it from
    /// the vdb (`N pg34-gextra + N pg34-gapp`), pubgrub used to plan only
    /// `N pg34-gapp`.
    #[test]
    fn pg34_gapp_pubgrub_pvud_expands_installed_only() {
        let req = pg34_s3_request(&["dev-libs/pg34-gapp"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-gapp resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-gapp-1:0", "dev-libs/pg34-gextra-1:0"]
        );
    }

    /// #34 S3 non-regression: `-pvuD` on pg34-app still plans the fresh dep
    /// (`N pg34-extra + N pg34-app`, like Portage): S2's leaf rule stays off
    /// under deep and the vdb reader agrees with the repo for pg34-lib.
    #[test]
    fn pg34_app_pubgrub_pvud_expands_fresh_dep() {
        let req = pg34_s3_request(&["dev-libs/pg34-app"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-app resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-app-1:0", "dev-libs/pg34-extra-1:0"]
        );
    }

    /// `-pvuD` on pg34-blib plans `U pg34-btool-2` under pubgrub: an
    /// installed, already-built parent's BDEPEND is traversed when bdeps is
    /// on (Portage `_add_pkg_deps`; captures
    /// `361-blib-pvuD-noflags.txt`/`361-blib-pvuD-bdy.txt` plan
    /// `U pg34-btool-2`, `Total: 1 package (1 upgrade)`).
    #[test]
    fn pg34_blib_pubgrub_pvud_plans_nothing() {
        let req = pg34_s3_request(&["dev-libs/pg34-blib"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-blib resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-btool-2:0"]);
    }

    /// #34 S3 non-regression: `-pvuD --with-bdeps=n` on pg34-blib plans
    /// nothing either (Portage agrees; the ebuild DESCRIPTIONs record the
    /// corrected guard).
    #[test]
    fn pg34_blib_pubgrub_pvud_nobdeps_plans_nothing() {
        let mut req = pg34_s3_request(&["dev-libs/pg34-blib"]);
        req.with_bdeps = false;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-blib resolves");
        assert_eq!(pg34_plan(&result), Vec::<String>::new());
    }

    /// #432 S-11(c): `-pvu --deep=1` on dev-libs/deeppkg plans nothing.
    /// deeppkg (installed, depth 0) recurses since 0 < 1, discovering
    /// installed deeppkg2 at depth 1 -- but deeppkg2 itself does NOT
    /// recurse (1 < 1 is false), so newpkg is never reached, exactly like
    /// the default solver (Portage `_too_deep`, `depgraph.py:7359-7375`;
    /// pmtest `test_deep_bounded_depth_stops_short_of_the_full_chain`).
    #[test]
    fn pg432_deep_bounded_one_plans_nothing() {
        let mut req = pg34_s3_request(&["dev-libs/deeppkg"]);
        req.deep = super::super::Deep::Bounded(1);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("deeppkg resolves");
        assert_eq!(pg34_plan(&result), Vec::<String>::new());
    }

    /// #432 S-11(c): `-pvu --deep=2` on dev-libs/deeppkg plans newpkg.
    /// deeppkg2 at depth 1 recurses since 1 < 2, reaching newpkg (New) --
    /// the same plan the default solver prints, and the same as unlimited
    /// `--deep`.
    #[test]
    fn pg432_deep_bounded_two_plans_newpkg() {
        let mut req = pg34_s3_request(&["dev-libs/deeppkg"]);
        req.deep = super::super::Deep::Bounded(2);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("deeppkg resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/newpkg-1.0:0"]);
    }

    /// #34 S4 request: like the S3 `-pvuD` folds, plus the S4 engine-switch
    /// inputs (`newuse`, `empty`, `nodeps`) set explicitly per test, mirroring
    /// the CLI fold (`resolve_plan.rs`: selective = update||newuse||…; the
    /// walk clears selective for `-e`; `-O` sets `nodeps`).
    /// `fixture_request` defaults every one of these to `false`/`NotRequested`.
    fn pg34_s4_request(atoms: &[&str]) -> super::super::ResolveRequest {
        let mut req = pg34_s3_request(atoms);
        req.newuse = false;
        req.empty = false;
        req.nodeps = false;
        req
    }

    /// #34 S4: `-pvuD` upgrades an installed dep in its slot
    /// (`set_prefer_update(update && deep && !empty)`). `pg34-dlib-1` is
    /// installed with `-2` in the repo; Portage plans `U pg34-dlib-2 +
    /// N pg34-dtop`, pubgrub used to plan only `N pg34-dtop`.
    #[test]
    fn pg34_dtop_pubgrub_pvud_upgrades_installed_dep() {
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        let req = pg34_s4_request(&["dev-libs/pg34-dtop"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-dtop resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-dlib-2:0", "dev-libs/pg34-dtop-1:0"]
        );
        let dlib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-dlib")
            .expect("pg34-dlib entry");
        assert!(
            matches!(dlib.outcome, super::super::PretendOutcome::Upgrade { .. }),
            "unexpected outcome: {:?}",
            dlib.outcome
        );
    }

    /// #34 S4: `-pvu` without `-D` updates a named target but not its
    /// installed dep. `pg34-dtop` is new (plans `N`), its installed dep
    /// `pg34-dlib-1` stays (`prefer_update` needs deep); naming
    /// `pg34-dlib` directly still upgrades it to `-2` (a root target is
    /// never favored).
    #[test]
    fn pg34_dtop_pubgrub_pvu_updates_named_target_not_installed_dep() {
        // `-pvu`: selective through `--update`, no deep, build deps on.
        let mut req = pg34_s4_request(&["dev-libs/pg34-dtop"]);
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-dtop resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-dtop-1:0"]);

        let mut req = pg34_s4_request(&["dev-libs/pg34-dlib"]);
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-dlib resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-dlib-2:0"]);
    }

    /// #367: `-pvuD` on the new parent takes the newest slot. Expected from
    /// Portage 3.0.82.2 (`LLM/evidence/2026-10-15-367-s0/cap-top-pvuD-portage.txt`
    /// plans `NS s-2 + N top`), never from portuale output.
    #[test]
    fn pg367_top_pubgrub_pvud_takes_the_newest_slot() {
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        let req = pg34_s4_request(&["dev-libs/pg367-top"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg367-top resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg367-s-2:2", "dev-libs/pg367-top-1:0"]
        );
        let slot = result
            .entries
            .iter()
            .find(|e| e.package == "pg367-s")
            .expect("pg367-s entry");
        assert!(
            matches!(slot.outcome, super::super::PretendOutcome::New { .. }) && slot.new_slot,
            "NS renders New in a new slot: {:?} new_slot={}",
            slot.outcome,
            slot.new_slot
        );
    }

    /// #367: `-pvD` (deep without update) keeps the installed slot. Expected
    /// from Portage 3.0.82.2 (`LLM/evidence/2026-10-15-367-s0/cap-top-pvD-portage.txt`
    /// plans `N top` only), never from portuale output.
    #[test]
    fn pg367_top_pubgrub_pvd_keeps_the_installed_slot() {
        // `-pvD`: deep without update is not selective.
        let mut req = pg34_s4_request(&["dev-libs/pg367-top"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::Unlimited;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg367-top resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg367-top-1:0"]);
    }

    /// #367: `-pvu` without deep keeps the installed slot. Expected from
    /// Portage 3.0.82.2 (`LLM/evidence/2026-10-15-367-s0/cap-top-pvu-portage.txt`
    /// plans `N top` only), never from portuale output.
    #[test]
    fn pg367_top_pubgrub_pvu_keeps_the_installed_slot() {
        // `-pvu`: selective through `--update`, no deep, build deps on.
        let mut req = pg34_s4_request(&["dev-libs/pg367-top"]);
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg367-top resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg367-top-1:0"]);
    }

    /// #367: `-pve` takes the newest slot. Expected from Portage 3.0.82.2
    /// (`LLM/evidence/2026-10-15-367-s0/cap-top-pve-portage.txt` plans
    /// `NS s-2 + N top`), never from portuale output.
    #[test]
    fn pg367_top_pubgrub_pve_takes_the_newest_slot() {
        // `-pve`: the request level keeps selective unset; `empty` drives it.
        let mut req = pg34_s4_request(&["dev-libs/pg367-top"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        req.empty = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg367-top resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg367-s-2:2", "dev-libs/pg367-top-1:0"]
        );
    }

    /// #34 S4: the USE-drift kept/rebuild set. Under `--newuse` the installed
    /// `pg34-ulib-1` (vdb `USE=""`, config `x`) joins `rebuild` with its
    /// changed flags; without `--newuse`/`--changed-use` it stays kept.
    #[test]
    fn pg34_ulib_kept_rebuild_set_gains_use_drift() {
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-uapp"]);
        // `-pvuDN`: selective through `--update`, unlimited deep, newuse.
        req.selective = true;
        req.update = true;
        req.deep = super::super::Deep::Unlimited;
        req.newuse = true;
        req.dynamic_deps = true;
        req.config
            .package_use
            .push(("dev-libs/pg34-ulib".to_string(), vec!["x".to_string()]));
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert_eq!(
            kept.use_drift.get("dev-libs/pg34-ulib-1"),
            Some(&vec!["x".to_string()]),
            "USE drift flags: {:?}",
            kept.use_drift
        );
        assert!(
            kept.rebuild.contains("dev-libs/pg34-ulib-1"),
            "drifted cpv rebuilds: {:?}",
            kept.rebuild
        );

        // Same request without `--newuse`: no drift, no rebuild.
        req.newuse = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-ulib-1"),
            "no drift without --newuse: {:?}",
            kept.rebuild
        );
        assert!(kept.use_drift.is_empty());
    }

    /// #34 S4: `-pvuDN` on pg34-uapp plans `N uextra + R ulib + N uapp`. The
    /// drifted dep rebuilds with the config USE, so its `x?` dep switches on
    /// (Portage `USE="x*"`); pubgrub used to plan only `N uapp` (S3's xfail).
    #[test]
    fn pg34_uapp_pubgrub_pvudn_rebuilds_use_drift() {
        // `-pvuDN`: the S4 request plus `--newuse`.
        let mut req = pg34_s4_request(&["dev-libs/pg34-uapp"]);
        req.newuse = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-uapp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-uapp-1:0",
                "dev-libs/pg34-uextra-1:0",
                "dev-libs/pg34-ulib-1:0"
            ]
        );
        let ulib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-ulib")
            .expect("pg34-ulib entry");
        match &ulib.outcome {
            super::super::PretendOutcome::Reinstall { changed_flags, .. } => {
                assert_eq!(changed_flags, &["x".to_string()]);
            }
            other => panic!("expected USE-drift Reinstall, got {other:?}"),
        }
    }

    /// #34 S4: `-pvN` on the installed named target plans `N uextra +
    /// R ulib` (Portage; pubgrub used to plan nothing). `selective_no_update`
    /// without `--update` keeps the satisfied target at its installed
    /// version instead of upgrading, and the drift rebuild carries the flags.
    #[test]
    fn pg34_ulib_pubgrub_pvn_newuse_rebuilds_named_target() {
        // `-pvN`: selective through `--newuse`, no update, no deep.
        let mut req = pg34_s4_request(&["dev-libs/pg34-ulib"]);
        req.selective = true;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        req.newuse = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-ulib resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-uextra-1:0", "dev-libs/pg34-ulib-1:0"]
        );
        let ulib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-ulib")
            .expect("pg34-ulib entry");
        match &ulib.outcome {
            super::super::PretendOutcome::Reinstall { changed_flags, .. } => {
                assert_eq!(changed_flags, &["x".to_string()]);
            }
            other => panic!("expected USE-drift Reinstall, got {other:?}"),
        }
    }

    /// #34 S4: `-pve` on pg34-uapp plans `N uextra + R ulib + N uapp`
    /// (Portage; pubgrub used to plan `R ulib + N uapp` with no `uextra`).
    /// Under emptytree every installed cpv rebuilds with the config USE.
    #[test]
    fn pg34_uapp_pubgrub_pve_rebuilds_with_config_use() {
        // `-pve`: the request level keeps selective unset (the CLI fold sets
        // it only through update/newuse/…); `empty` alone drives the set.
        let mut req = pg34_s4_request(&["dev-libs/pg34-uapp"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        req.empty = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-uapp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-uapp-1:0",
                "dev-libs/pg34-uextra-1:0",
                "dev-libs/pg34-ulib-1:0"
            ]
        );
        let ulib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-ulib")
            .expect("pg34-ulib entry");
        assert!(
            matches!(ulib.outcome, super::super::PretendOutcome::Reinstall { .. }),
            "unexpected outcome: {:?}",
            ulib.outcome
        );
    }

    /// #34 S4: `-pvO` (`--nodeps`) plans the target only. Both cells already
    /// matched Portage; the switch (`set_nodeps`) pins them against
    /// dependency expansion.
    #[test]
    fn pg34_nodeps_pubgrub_pvo_plans_target_only() {
        for atom in ["dev-libs/pg34-dtop", "dev-libs/pg34-app"] {
            // `-pvO`: not selective, no update, no deep, nodeps.
            let mut req = pg34_s4_request(&[atom]);
            req.selective = false;
            req.update = false;
            req.deep = super::super::Deep::NotRequested;
            req.nodeps = true;
            let result = active_resolver_for(super::super::SolverKind::PubGrub)
                .resolve(&req)
                .expect("target resolves");
            let package = atom.split('/').nth(1).expect("cat/pkg");
            assert_eq!(pg34_plan(&result), [format!("dev-libs/{package}-1:0")]);
        }
    }

    /// #34 S4 non-regression: `-pvuDN` on pg34-gs still plans
    /// `NS gs-2 + U gs-1.1` like Portage (this cell already matched; the new
    /// switches must not move it).
    #[test]
    fn pg34_gs_pubgrub_pvudn_stays_equal() {
        // `-pvuDN`: the S4 request plus `--newuse`.
        let mut req = pg34_s4_request(&["dev-libs/pg34-gs"]);
        req.newuse = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-gs resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-gs-1.1:1", "dev-libs/pg34-gs-2:2"]
        );
    }

    /// #34 S5 G1: the installed version is keyword-masked, so no visible
    /// version is `>=` it and Portage re-selects the best visible one
    /// (`_downgrade_probe`, `depgraph.py:2946-2957`, gated on `want_update`,
    /// `:3805-3809`). `pg34-downer-2` is installed and `~amd64`-only while
    /// `-1` is stable; every `-pv`/`-pvu`/`-pvuD`/`-pvuDN`/`-pve` cell on the
    /// named target plans `UD downer-1 + N dep-1` (container captures in
    /// `LLM/evidence/2026-10-15-34-s5/g1/`). Pubgrub keeps the installed
    /// version instead (omitted, or `R -2` with stub deps under `-e`).
    fn pg34_downer_downgrade_outcome(result: &super::super::GraphResult) {
        let downer = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-downer")
            .expect("pg34-downer entry");
        match &downer.outcome {
            super::super::PretendOutcome::Downgrade { from, to } => {
                assert_eq!(from, "2");
                assert_eq!(to, "1");
            }
            other => panic!("expected Downgrade 2 -> 1, got {other:?}"),
        }
    }

    /// #34 S5 G1: the downgrade-probe set follows Portage's `want_update`
    /// gate. `pg34-downer-2` is installed with only stable `-1` visible
    /// (nothing `>=` it); `pg34-dlib-1` is installed with `-2` visible
    /// (never a downgrade).
    #[test]
    fn pg34_downer_downgrade_set_follows_update_walk() {
        // Named target without update or deep: a re-select (`arg_atoms`).
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-downer"]);
        req.selective = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.downgrade.contains("dev-libs/pg34-downer-2"),
            "named masked-installed target downgrades: {:?}",
            kept.downgrade
        );
        assert!(
            !kept.downgrade.contains("dev-libs/pg34-dlib-1"),
            "a version with a visible upgrade never downgrades: {:?}",
            kept.downgrade
        );

        // Unrelated target, no update, no deep: Portage keeps the instance.
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        req.selective = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.downgrade.is_empty(),
            "nothing downgrades without a walk: {:?}",
            kept.downgrade
        );

        // Update without deep still keeps an installed dep (`_too_deep`).
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        req.selective = true;
        req.update = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.downgrade.is_empty(),
            "no downgrade without deep: {:?}",
            kept.downgrade
        );

        // Update with deep reaches the masked dep (`want_update` true).
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        req.selective = true;
        req.update = true;
        req.deep = super::super::Deep::Unlimited;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.downgrade.contains("dev-libs/pg34-downer-2"),
            "deep update downgrades the masked dep: {:?}",
            kept.downgrade
        );

        // Emptytree re-resolves everything from the repo.
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-app"]);
        req.empty = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.downgrade.contains("dev-libs/pg34-downer-2"),
            "emptytree downgrades the masked dep: {:?}",
            kept.downgrade
        );
    }

    /// #34 S5 G1: `-pv` on the named masked-installed target downgrades to
    /// the best visible version (Portage `UD downer-1 + N dep-1`; pubgrub
    /// used to plan nothing).
    #[test]
    fn pg34_downer_pubgrub_pv_downgrades_masked_installed() {
        // `-pv`: not selective, no update, no deep, build deps on.
        let mut req = pg34_s4_request(&["dev-libs/pg34-downer"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-downer resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-dep-1:0", "dev-libs/pg34-downer-1:0"]
        );
        pg34_downer_downgrade_outcome(&result);
    }

    /// #34 S5 G1: `-pvuD` on the named masked-installed target downgrades
    /// (Portage `UD downer-1 + N dep-1`; pubgrub used to plan only `N dep-1`
    /// while the installed `-2` stayed hidden).
    #[test]
    fn pg34_downer_pubgrub_pvud_downgrades_masked_installed() {
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        let req = pg34_s4_request(&["dev-libs/pg34-downer"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-downer resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-dep-1:0", "dev-libs/pg34-downer-1:0"]
        );
        pg34_downer_downgrade_outcome(&result);
    }

    /// #34 S5 G1: `-pve` on the named masked-installed target downgrades and
    /// expands the re-selected version's deps (Portage `UD downer-1 +
    /// N dep-1`; pubgrub used to plan `R downer-2` with stub deps + `N dep-1`).
    #[test]
    fn pg34_downer_pubgrub_pve_downgrades_masked_installed() {
        // `-pve`: the request level keeps selective unset (the CLI fold sets
        // it only through update/newuse/…); `empty` alone drives the set.
        let mut req = pg34_s4_request(&["dev-libs/pg34-downer"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        req.empty = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-downer resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-dep-1:0", "dev-libs/pg34-downer-1:0"]
        );
        pg34_downer_downgrade_outcome(&result);
    }

    /// Round-2 helper: a [`super::SolvedPlan`] from `(cp, version)` members
    /// and `(parent cp, child cp)` taken edges.
    fn pg34_solved_plan(members: &[(&str, &str)], taken: &[(&str, &str)]) -> super::SolvedPlan {
        super::SolvedPlan {
            members: members
                .iter()
                .map(|(cp, version)| (cp.to_string(), version.to_string()))
                .collect(),
            taken: taken
                .iter()
                .map(|(parent, child)| (parent.to_string(), child.to_string()))
                .collect(),
        }
    }

    /// #34 S5 G2 (round 2): the parent-USE reinstall pass over an explicit
    /// solution-member set. `pg34-capp-1` (not installed) requires
    /// `dev-libs/pg34-clib[f]` while the installed `pg34-clib-1` was built
    /// with `USE=""` and the repo ebuild carries `IUSE="+f"`
    /// (default-on): Portage re-selects the repo ebuild (`R` with the
    /// changed flag) and expands `f? ( pg34-cdep )` even without `--deep`
    /// (`depgraph.py:6030-6110` region; triage G2). Pubgrub kept the
    /// installed child as a hidden S2 leaf instead, dropping the `R` row
    /// and the subtree.
    ///
    /// Round 1 evaluated every reachable cp's strings pre-solve, so a USE
    /// atom from a package the solve never selects (an untaken `||`
    /// branch, an unselected version) rebuilt the installed child anyway
    /// (`dev-qt/qtwebengine` rebuilding `dev-libs/libxml2` for an
    /// `app-editors/vim` probe: bed delta +1208 extras in `-pv`). The pass
    /// now only reads solution members' strings: the out-of-plan arm below
    /// pins that a non-member parent contributes nothing.
    #[test]
    fn pg34_capp_kept_parent_use_atom_reinstalls_child() {
        // `-pv`: not selective, no update, no deep. The base set knows no
        // G2 rebuilds; the pass with the merged parent as a member
        // evaluates its `clib[f]` atom against the installed instance.
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-capp"]);
        req.selective = false;
        let mut kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "base set carries no parent-USE rebuilds: {:?}",
            kept.rebuild
        );
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "parent-required clib[f] rebuilds the flag-less installed child: {:?}",
            kept.rebuild
        );
        assert_eq!(
            kept.use_drift.get("dev-libs/pg34-clib-1"),
            Some(&vec!["f".to_string()]),
            "the R row carries the changed flag: {:?}",
            kept.use_drift
        );

        // Round-2 regression pin: the same pass with the parent NOT a
        // solution member (an untaken branch, an unselected version, a
        // reachable-but-unmerged cp) contributes nothing, even though the
        // `clib[f]` atom sits in the reachable cone.
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-capp"]);
        req.selective = false;
        let mut kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let plan = pg34_solved_plan(&[], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "an out-of-plan parent's USE atom must not reinstall the child: {:?}",
            kept.rebuild
        );

        // `-pvu` (selective through `--update`, no deep): Portage
        // re-evaluates the merged parent's dep the same way (container
        // capture `cap-pg34-capp-pvu-portage.txt` shows the same R row).
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-capp"]);
        req.selective = true;
        req.update = true;
        let mut kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "selective -pvu still rebuilds the USE-pressured child: {:?}",
            kept.rebuild
        );

        // `-pvO` (`--nodeps`): deps never expand, so no parent USE
        // pressure exists and nothing rebuilds.
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-capp"]);
        req.selective = false;
        req.nodeps = true;
        let mut kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "nodeps plans the target only: {:?}",
            kept.rebuild
        );

        // Config-change variant: `pg34-nlib` carries `IUSE="f"`
        // (default-off) and the config does not enable it, so the repo
        // ebuild cannot satisfy `nlib[f]` under the current config --
        // Portage stops at the autounmask gate (rc 1, G4a) instead of a
        // plain R. No G2 rebuild (capture only, no fix).
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-napp"]);
        req.selective = false;
        let mut kept = super::KeptInstalled::of(&req, &lazy, &installed);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-napp", "1")], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-nlib-1"),
            "an unsatisfiable-under-config USE atom is not a G2 rebuild: {:?}",
            kept.rebuild
        );
    }

    /// #34 S5 G2: `-pv` on pg34-capp plans `N cdep + R clib + N capp`
    /// (Portage `cap-pg34-capp-pv-portage.txt`; pubgrub used to plan only
    /// `N capp`).
    #[test]
    fn pg34_capp_pubgrub_pv_reinstalls_use_atom_child() {
        // `-pv`: not selective, no update, no deep, build deps on.
        let mut req = pg34_s4_request(&["dev-libs/pg34-capp"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-capp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-capp-1:0",
                "dev-libs/pg34-cdep-1:0",
                "dev-libs/pg34-clib-1:0"
            ]
        );
        let clib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-clib")
            .expect("pg34-clib entry");
        match &clib.outcome {
            super::super::PretendOutcome::Reinstall { changed_flags, .. } => {
                assert_eq!(changed_flags, &["f".to_string()]);
            }
            other => panic!("expected Reinstall with changed flag f, got {other:?}"),
        }
    }

    /// #34 S5 G2: `-pvu` on pg34-capp plans the same `N cdep + R clib +
    /// N capp` (Portage `cap-pg34-capp-pvu-portage.txt`; pubgrub used to
    /// plan only `N capp`).
    #[test]
    fn pg34_capp_pubgrub_pvu_reinstalls_use_atom_child() {
        // `-pvu`: selective through `--update`, no deep, build deps on.
        let mut req = pg34_s4_request(&["dev-libs/pg34-capp"]);
        req.deep = super::super::Deep::NotRequested;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-capp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-capp-1:0",
                "dev-libs/pg34-cdep-1:0",
                "dev-libs/pg34-clib-1:0"
            ]
        );
    }

    /// #34 S5 G2: `-pvuD` on pg34-capp plans the same `N cdep + R clib +
    /// N capp` (Portage `cap-pg34-capp-pvuD-portage.txt`; pubgrub used to
    /// plan only `N capp`).
    #[test]
    fn pg34_capp_pubgrub_pvud_reinstalls_use_atom_child() {
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        let req = pg34_s4_request(&["dev-libs/pg34-capp"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-capp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-capp-1:0",
                "dev-libs/pg34-cdep-1:0",
                "dev-libs/pg34-clib-1:0"
            ]
        );
    }

    /// #34 S5 G2 control: `-pvuDN` already plans the triple through S4's
    /// USE-drift arm (Portage `cap-pg34-capp-pvuDN-portage.txt`); the G2
    /// arm must union its flags, not overwrite them.
    #[test]
    fn pg34_capp_pubgrub_pvudn_keeps_use_drift_triple() {
        // `-pvuDN`: the S3 folds plus `--newuse`.
        let mut req = pg34_s4_request(&["dev-libs/pg34-capp"]);
        req.newuse = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-capp resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-capp-1:0",
                "dev-libs/pg34-cdep-1:0",
                "dev-libs/pg34-clib-1:0"
            ]
        );
        let clib = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-clib")
            .expect("pg34-clib entry");
        match &clib.outcome {
            super::super::PretendOutcome::Reinstall { changed_flags, .. } => {
                assert_eq!(changed_flags, &["f".to_string()]);
            }
            other => panic!("expected Reinstall with changed flag f, got {other:?}"),
        }
    }

    /// #34 S5 G3: `-pv` on pg34-self avoids the direct self-cycle in the
    /// `||` (the go/go-bootstrap shape) and picks the bootstrap sibling
    /// (Portage `depgraph.py:778-793`); the bridge resolves the `||` to
    /// itself and fails with a circular self-dependency instead.
    #[test]
    fn pg34_self_pubgrub_pv_picks_the_bootstrap_sibling() {
        let mut req = fixture_request(&["dev-libs/pg34-self"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-self resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-self-1:0", "dev-libs/pg34-selfboot-1:0"]
        );
    }

    /// #34 S5 G3: `-pvuD` on pg34-self plans the same pair (deep changes
    /// nothing with nothing installed; Portage plans both in every mode).
    #[test]
    fn pg34_self_pubgrub_pvud_picks_the_bootstrap_sibling() {
        use super::super::Deep;
        let mut req = fixture_request(&["dev-libs/pg34-self"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        req.selective = true;
        req.update = true;
        req.deep = Deep::Unlimited;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-self resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-self-1:0", "dev-libs/pg34-selfboot-1:0"]
        );
    }

    /// #34 S5 G5: `-pv` on pg34-svt prefers the in-graph `||`
    /// alternative. `pg34-sv-1` first-lists `pg34-sva` (nothing installed,
    /// so the engine's installed-preference cannot decide and its final
    /// fallback takes the first-listed branch), but the target also pulls
    /// `pg34-svb` into the graph, so Portage resolves the group to `svb`
    /// (`depgraph.py:778-793`; the `virtual/secret-service` shape from
    /// bed `-pv kde-apps/kdecore-meta`). Portage plans `N svb + N sv +
    /// N svt` (container capture
    /// `LLM/evidence/2026-10-15-34-s5/g5/cap-pg34-svt-pv-portage.txt`);
    /// pre-fix pubgrub additionally merges first-listed `sva` and its
    /// `svx` subtree.
    #[test]
    fn pg34_svt_pubgrub_pv_prefers_in_graph_alternative() {
        let mut req = fixture_request(&["dev-libs/pg34-svt"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-svt resolves");
        assert_eq!(
            pg34_plan(&result),
            [
                "dev-libs/pg34-sv-1:0",
                "dev-libs/pg34-svb-1:0",
                "dev-libs/pg34-svt-1:0"
            ]
        );
    }

    /// #34 S5 G5/G6c: `-pve` on pg34-tt prefers the in-graph target over
    /// the installed first-listed `||` branch. `pg34-tx-1` carries
    /// `|| ( pg34-info pg34-tt )` (the texinfo
    /// `|| ( info-manual emacs )` shape from bed `-pve
    /// app-editors/emacs`); `pg34-info-1` is installed, `pg34-tt` is the
    /// probe target itself. Portage prefers the already-in-graph
    /// alternative even over an installed one (`dep_check.py`
    /// `dep_zapdeps` in-graph promotion) and prints no info row
    /// (container capture
    /// `LLM/evidence/2026-10-15-34-s5/g5/cap-pg34-stt-pve-portage.txt`,
    /// single-root set: the staged-ROOT capture's `N info` row merges to
    /// the host root, which lacks the installed instance); pre-fix
    /// pubgrub's installed-preference takes `info` (`R info-1`).
    #[test]
    fn pg34_stt_pubgrub_pve_prefers_target_over_installed_branch() {
        // `-pve`: the request level keeps selective (the walk clears it);
        // `empty` alone drives the rebuild set.
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-tt"]);
        req.empty = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-tt resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-tt-1:0", "dev-libs/pg34-tx-1:0"]
        );
        assert!(
            result.entries.iter().all(|e| e.package != "pg34-info"),
            "no info-manual row: {:?}",
            pg34_plan(&result)
        );
    }

    /// #34 S5 G5/G6c: `-pv` on pg34-tt already agrees with Portage (the
    /// installed `pg34-info-1` stays a hidden kept leaf, so the wrong
    /// branch choice prints no row). Non-regression pin: the fix must not
    /// add rows here (container capture
    /// `LLM/evidence/2026-10-15-34-s5/g5/cap-pg34-stt-pv-portage.txt`,
    /// single-root set).
    #[test]
    fn pg34_stt_pubgrub_pv_ignores_installed_alternative() {
        let mut req = fixture_request(&["dev-libs/pg34-tt"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-tt resolves");
        assert_eq!(
            pg34_plan(&result),
            ["dev-libs/pg34-tt-1:0", "dev-libs/pg34-tx-1:0"]
        );
    }

    /// #34 S5 G7: `-pv` on pg34-snew keeps the installed soft-block
    /// unresolved: `N snew` plus an unsatisfied `[blocks B]` row, no
    /// `[uninstall]` (Portage's "pulled in by installed pg34-smid" parent
    /// keeps the block; the bridge invents the removal instead).
    #[test]
    fn pg34_snew_pubgrub_pv_keeps_soft_block_unresolved() {
        let mut req = fixture_request(&["dev-libs/pg34-snew"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pv`: not selective, no update, build deps on.
        req.selective = false;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-snew resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-snew-1:0"]);
        assert!(
            result
                .entries
                .iter()
                .all(|e| !matches!(e.outcome, super::super::PretendOutcome::Uninstall { .. })),
            "no package is scheduled for unmerge"
        );
        let snew = result
            .entries
            .iter()
            .find(|e| e.package == "pg34-snew")
            .expect("pg34-snew entry");
        assert_eq!(snew.blockers.len(), 1, "the soft block is still reported");
        assert!(
            snew.blockers[0].unsolvable && snew.blockers[0].satisfied_by.is_none(),
            "the block stays unresolved, like Portage's [blocks B]"
        );
    }

    /// #34 S5 G7: `-pvuD` on pg34-snew already agrees (deep expands installed
    /// pg34-smid into the solution, parenting pg34-sblk); non-regression pin.
    #[test]
    fn pg34_snew_pubgrub_pvud_keeps_soft_block_unresolved() {
        use super::super::Deep;
        let mut req = fixture_request(&["dev-libs/pg34-snew"]);
        req.solver = super::super::SolverKind::PubGrub;
        // `-pvuD`: selective through `--update`, unlimited deep, build deps on.
        req.selective = true;
        req.update = true;
        req.deep = Deep::Unlimited;
        req.with_bdeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-snew resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-snew-1:0"]);
        assert!(
            result
                .entries
                .iter()
                .all(|e| !matches!(e.outcome, super::super::PretendOutcome::Uninstall { .. })),
            "no package is scheduled for unmerge"
        );
    }

    /// #34 S5 G3: the `||` self-cycle rule keeps what it must: a bare self
    /// atom outside any group, a `^^` self branch, and an all-self `||`
    /// (emptying the group would invent a worse failure); an installed
    /// instance satisfying the branch keeps it too (Portage builds against
    /// the installed self, no cycle).
    #[test]
    fn pg34_self_cycle_rule_keeps_non_choice_self_edges() {
        use super::super::InstalledPackage;
        let no_installed: Vec<InstalledPackage> = Vec::new();
        let parse = |text: &str| portage_solver::DepEntry::parse(text).expect("fixture dep parses");
        let render = |entries: &[portage_solver::DepEntry]| {
            entries
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Bare self outside `||` stays (a real cycle the engine must report).
        let bare = parse("dev-libs/pg34-self");
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &no_installed,
            &[bare.clone(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        );
        assert_eq!(render(&kept[0]), render(&bare));

        // `^^` is not `||`: untouched even with an uninstalled self inside.
        let exactly_one = parse("^^ ( dev-libs/pg34-self dev-libs/pg34-selfboot )");
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &no_installed,
            &[
                exactly_one.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        );
        assert_eq!(render(&kept[0]), render(&exactly_one));

        // An all-self `||` keeps its form (nothing else to take).
        let all_self = parse("|| ( dev-libs/pg34-self )");
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &no_installed,
            &[
                all_self.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        );
        assert_eq!(render(&kept[0]), render(&all_self));

        // The choice itself drops the self branch, keeping the sibling.
        let choice = parse("|| ( dev-libs/pg34-self dev-libs/pg34-selfboot )");
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &no_installed,
            &[
                choice.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        );
        assert_eq!(kept[0].len(), 1);
        let portage_solver::DepEntry::AnyOf(survivors) = &kept[0][0] else {
            panic!("the || group survives with one branch");
        };
        assert!(matches!(
            &survivors[..],
            [portage_solver::DepEntry::Atom(dep)] if format!("{}", dep.cpn) == "dev-libs/pg34-selfboot"
        ));

        // An installed self keeps the branch (no cycle to avoid).
        let installed = vec![InstalledPackage {
            category: "dev-libs".to_string(),
            package: "pg34-self".to_string(),
            version: "1".to_string(),
            slot: "0".to_string(),
        }];
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &installed,
            &[
                choice.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        );
        assert_eq!(render(&kept[0]), render(&choice));

        // A self branch pinned to another slot is not this candidate.
        let other_slot = parse("|| ( dev-libs/pg34-self:1 dev-libs/pg34-selfboot )");
        let kept = super::without_self_alternatives(
            "dev-libs/pg34-self",
            "0",
            &no_installed,
            &[
                other_slot.clone(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        );
        assert_eq!(render(&kept[0]), render(&other_slot));
    }

    /// #34 S5 G2 control: `-pvO` (`--nodeps`) on pg34-capp plans only
    /// `N capp` (Portage behaviour for nodeps; the G2 arm stays quiet).
    #[test]
    fn pg34_capp_pubgrub_pvo_plans_target_only() {
        // `-pvO`: the S4 request with `nodeps` on, no deep.
        let mut req = pg34_s4_request(&["dev-libs/pg34-capp"]);
        req.selective = false;
        req.update = false;
        req.deep = super::super::Deep::NotRequested;
        req.nodeps = true;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-capp resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-capp-1:0"]);
    }

    // --- #34 S6 mutation kills -------------------------------------------
    //
    // Each test below pins one pubgrub-path behaviour that `cargo mutants`
    // showed the suite did not cover (baseline 2026-10-15: 115 missed of
    // 413). Behavioural references stay Portage-side per the plan; the
    // assertions pin the bridge's own reading of Portage's rules.

    /// `newest_installed` orders by `vercmp`: the greatest version wins no
    /// matter where it sits in the list (Portage picks the best installed
    /// instance, not the first or last one seen).
    #[test]
    fn s6_newest_installed_prefers_the_greatest_version() {
        assert_eq!(super::newest_installed(&["3.0".to_string()]), "3.0");
        assert_eq!(
            super::newest_installed(&["3.0".to_string(), "1.0".to_string(), "2.0".to_string()]),
            "3.0",
            "the greatest version wins even when it is not last"
        );
        assert_eq!(
            super::newest_installed(&["1.0".to_string(), "3.0".to_string(), "2.0".to_string()]),
            "3.0",
            "the greatest version wins even when it is not first"
        );
    }

    /// `version_row` leaves the sub-slot empty for a shared slot/sub-slot
    /// (the walk's own call renders no `/sub` then) and records a distinct
    /// one.
    #[test]
    fn s6_version_row_marks_a_shared_sub_slot_as_absent() {
        let record = super::LoadedVersion {
            category: "dev-libs".to_string(),
            package: "pg34-x".to_string(),
            version: "1".to_string(),
            slot: "0".to_string(),
            sub_slot: "0".to_string(),
            repo_name: "testrepo".to_string(),
            iuse: String::new(),
            keywords: vec!["amd64".to_string()],
            required_use: String::new(),
            deps: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            raw_deps: Default::default(),
        };
        let (_, versions) = super::version_row(&record, Default::default()).expect("row builds");
        assert!(
            versions.subslot.is_none(),
            "a sub-slot equal to the slot stays absent"
        );
        let distinct = super::LoadedVersion {
            sub_slot: "0/1".to_string(),
            ..record.clone()
        };
        let (_, versions) = super::version_row(&distinct, Default::default()).expect("row builds");
        assert!(
            versions.subslot.is_some(),
            "a distinct sub-slot is recorded"
        );
    }

    /// `bridge_outcome_for` maps one pick against the installed db: a
    /// rebuild member reinstalls, a lower pick downgrades (even across a
    /// `vercmp`-equal boundary like `1.0-r0` vs `1.0`), and a new slot
    /// lists every installed instance as `oldbest`.
    #[test]
    fn s6_bridge_outcome_for_sorts_same_slot_versions() {
        use super::super::InstalledRef;
        use std::collections::{HashMap, HashSet};
        let record = super::LoadedVersion {
            category: "dev-libs".to_string(),
            package: "pg34-x".to_string(),
            version: "1.0-r0".to_string(),
            slot: "0".to_string(),
            sub_slot: "0".to_string(),
            repo_name: "testrepo".to_string(),
            iuse: String::new(),
            keywords: Vec::new(),
            required_use: String::new(),
            deps: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            raw_deps: Default::default(),
        };
        let installed = super::InstalledView {
            by_slot: HashMap::from([(
                (
                    "dev-libs".to_string(),
                    "pg34-x".to_string(),
                    "0".to_string(),
                ),
                vec!["1.0".to_string()],
            )]),
            occupied: HashSet::from([("dev-libs".to_string(), "pg34-x".to_string())]),
        };
        // A `vercmp`-equal pick is a downgrade, not an upgrade: `Equal`
        // does not satisfy `> 0` (Portage `vercmp` revision default).
        let (outcome, _, _) =
            super::bridge_outcome_for("dev-libs/pg34-x", &record, "1.0-r0", &installed, None);
        assert!(
            matches!(outcome, super::super::PretendOutcome::Downgrade { .. }),
            "a vercmp-equal pick downgrades: {outcome:?}"
        );
        // A new slot collects every installed instance, same category and
        // package only, slot-sorted.
        let installed = super::InstalledView {
            by_slot: HashMap::from([
                (
                    (
                        "dev-libs".to_string(),
                        "pg34-x".to_string(),
                        "1".to_string(),
                    ),
                    vec!["1".to_string()],
                ),
                (
                    (
                        "dev-libs".to_string(),
                        "pg34-x".to_string(),
                        "2".to_string(),
                    ),
                    vec!["2".to_string()],
                ),
                (
                    ("other".to_string(), "pg34-x".to_string(), "1".to_string()),
                    vec!["9".to_string()],
                ),
            ]),
            occupied: HashSet::from([("dev-libs".to_string(), "pg34-x".to_string())]),
        };
        let record = super::LoadedVersion {
            version: "3".to_string(),
            slot: "3".to_string(),
            ..record
        };
        let (outcome, oldbest, new_slot) =
            super::bridge_outcome_for("dev-libs/pg34-x", &record, "3", &installed, None);
        assert!(
            matches!(outcome, super::super::PretendOutcome::New { .. }),
            "an uninstalled slot is New: {outcome:?}"
        );
        assert_eq!(
            oldbest,
            vec![
                InstalledRef {
                    version: "1".to_string(),
                    slot: "1".to_string(),
                    sub_slot: String::new(),
                    repo: String::new(),
                },
                InstalledRef {
                    version: "2".to_string(),
                    slot: "2".to_string(),
                    sub_slot: String::new(),
                    repo: String::new(),
                },
            ],
            "oldbest holds the same-cp instances only, slot-sorted"
        );
        assert!(new_slot, "another slot is already installed");
    }

    /// `bridge_use_columns` diffs an `R` row against the vdb USE like the
    /// walk's own display, so a drifted flag renders `x*` (S4).
    #[test]
    fn s6_bridge_use_columns_paints_reinstall_drift() {
        let req = fixture_request(&["dev-libs/pg34-ulib"]);
        // Installed `USE=""` against `IUSE="x"` in the fixture vdb.
        let record = super::LoadedVersion {
            category: "dev-libs".to_string(),
            package: "pg34-ulib".to_string(),
            version: "1".to_string(),
            slot: "0".to_string(),
            sub_slot: "0".to_string(),
            repo_name: "testrepo".to_string(),
            iuse: "x".to_string(),
            keywords: vec!["amd64".to_string()],
            required_use: String::new(),
            deps: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            raw_deps: Default::default(),
        };
        let outcome = super::super::PretendOutcome::Reinstall {
            version: "1".to_string(),
            changed_flags: vec!["x".to_string()],
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        };
        let use_set = ["x".to_string()].into_iter().collect();
        let (_, expand, _) =
            super::bridge_use_columns(&req, "dev-libs/pg34-ulib", &record, "1", &use_set, &outcome);
        let body = expand
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            body.contains("x*"),
            "a drifted flag carries the reinstall marker: {body}"
        );
    }

    /// `collect_bridge_blockers` recovers buildtime provenance from the
    /// key order: `DEPEND`/`BDEPEND` blockers are buildtime, `RDEPEND`
    /// ones are not (Portage `blocker.priority.buildtime`).
    #[test]
    fn s6_collect_bridge_blockers_marks_buildtime_keys() {
        let record = super::LoadedVersion {
            category: "dev-libs".to_string(),
            package: "pg34-x".to_string(),
            version: "1".to_string(),
            slot: "0".to_string(),
            sub_slot: "0".to_string(),
            repo_name: "testrepo".to_string(),
            iuse: String::new(),
            keywords: Vec::new(),
            required_use: String::new(),
            deps: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            raw_deps: [
                "!!dev-libs/pg34-blka".to_string(),
                "!dev-libs/pg34-blkb".to_string(),
                "!!dev-libs/pg34-blkc".to_string(),
                String::new(),
                String::new(),
            ],
        };
        let use_set = std::collections::HashSet::new();
        let mut pending = Vec::new();
        let mut seen = std::collections::HashSet::new();
        super::collect_bridge_blockers(&record, &use_set, "1", &mut pending, &mut seen);
        let mut buildtime: Vec<(&str, bool)> = pending
            .iter()
            .map(|blocker| (blocker.target_package.as_str(), blocker.buildtime))
            .collect();
        buildtime.sort();
        assert_eq!(
            buildtime,
            vec![
                ("pg34-blka", true),
                ("pg34-blkb", false),
                ("pg34-blkc", true),
            ],
            "DEPEND/BDEPEND block, RDEPEND does not"
        );
    }

    /// `target_slot` reads only a named `:slot` pin: `=`/`*` operators
    /// and bare atoms select no slot (Portage `_greedy_slots`).
    #[test]
    fn s6_target_slot_reads_only_named_slots() {
        let named = portage_solver::Dep::parse("dev-libs/pg34-multi:1").expect("parses");
        assert_eq!(super::target_slot(&named), Some("1".to_string()));
        let bare = portage_solver::Dep::parse("dev-libs/pg34-multi").expect("parses");
        assert_eq!(super::target_slot(&bare), None);
        let equals = portage_solver::Dep::parse("dev-libs/pg34-multi:=").expect("parses");
        assert_eq!(super::target_slot(&equals), None);
        let any = portage_solver::Dep::parse("dev-libs/pg34-multi:*").expect("parses");
        assert_eq!(super::target_slot(&any), None);
    }

    /// `strip_self_from_entries` reaches through plain and USE-conditional
    /// groups: a direct self-cycle `||` alternative is dropped wherever it
    /// nests (Portage `depgraph.py:778-793` circular handling).
    #[test]
    fn s6_strip_self_reaches_into_plain_and_conditional_groups() {
        use portage_solver::DepEntry;
        let parse = |text: &str| DepEntry::parse(text).expect("parses");
        let atom_cpn = |entry: &DepEntry| match entry {
            DepEntry::Atom(dep) => format!("{}", dep.cpn),
            other => panic!("expected an atom, got {other:?}"),
        };
        // A self alternative nested in a plain group is dropped.
        let group_ab = |texts: &[&str]| {
            texts
                .iter()
                .map(|text| {
                    DepEntry::parse(text)
                        .expect("parses")
                        .into_iter()
                        .next()
                        .expect("one atom")
                })
                .collect::<Vec<_>>()
        };
        let entries = vec![DepEntry::AllOf(vec![DepEntry::AnyOf(group_ab(&[
            "dev-libs/pg34-self",
            "dev-libs/pg34-other",
        ]))])];
        let stripped = super::strip_self_from_entries("dev-libs/pg34-self", "0", &[], &entries);
        let group = match &stripped[..] {
            [DepEntry::AllOf(children)] => match &children[..] {
                [DepEntry::AnyOf(branches)] => branches,
                other => panic!("the nested || survives: {other:?}"),
            },
            other => panic!("the plain group survives: {other:?}"),
        };
        assert_eq!(group.len(), 1, "the self branch is dropped");
        assert_eq!(atom_cpn(&group[0]), "dev-libs/pg34-other");
        // Behind a USE conditional the same rule applies.
        let entries = parse("f? ( || ( dev-libs/pg34-self dev-libs/pg34-other ) )");
        let stripped = super::strip_self_from_entries("dev-libs/pg34-self", "0", &[], &entries);
        let conditional = match &stripped[..] {
            [DepEntry::UseConditional { children, .. }] => children,
            other => panic!("the conditional survives: {other:?}"),
        };
        let group = match &conditional[..] {
            [DepEntry::AnyOf(children)] => children,
            other => panic!("the nested || survives: {other:?}"),
        };
        assert_eq!(group.len(), 1, "the gated self branch is dropped");
        assert_eq!(atom_cpn(&group[0]), "dev-libs/pg34-other");
    }

    /// `installed_satisfies_self` gates the self-cycle drop on slot and
    /// version: an unpinned self is satisfied by any instance, a slot pin
    /// must match, and an unmet version floor drops the branch.
    #[test]
    fn s6_installed_satisfies_self_checks_slot_and_version() {
        use super::super::InstalledPackage;
        let installed = vec![InstalledPackage {
            category: "dev-libs".to_string(),
            package: "pg34-self".to_string(),
            version: "1.0".to_string(),
            slot: "1".to_string(),
        }];
        // Unpinned: any installed instance satisfies.
        let dep = portage_solver::Dep::parse("dev-libs/pg34-self").expect("parses");
        assert!(
            super::installed_satisfies_self(&dep, "dev-libs/pg34-self", &installed),
            "an unpinned self builds against the installed instance"
        );
        assert!(
            !super::installed_satisfies_self(&dep, "dev-libs/pg34-self", &[]),
            "nothing installed satisfies nothing"
        );
        // A slot pin naming another slot does not satisfy.
        let pinned = portage_solver::Dep::parse("dev-libs/pg34-self:1").expect("parses");
        assert!(
            super::installed_satisfies_self(&pinned, "dev-libs/pg34-self", &installed),
            "a matching slot pin satisfies"
        );
        let other_slot = portage_solver::Dep::parse("dev-libs/pg34-self:2").expect("parses");
        assert!(
            !super::installed_satisfies_self(&other_slot, "dev-libs/pg34-self", &installed),
            "another slot's pin does not satisfy"
        );
        // An unmet version floor does not satisfy.
        let floored = portage_solver::Dep::parse(">=dev-libs/pg34-self-2.0").expect("parses");
        assert!(
            !super::installed_satisfies_self(&floored, "dev-libs/pg34-self", &installed),
            "an installed 1.0 does not satisfy >=2.0"
        );
    }

    /// `use_deps_satisfied` follows the PMS USE-dependency table, including
    /// the `(+/-)` fallback for flags absent from IUSE and the `[!flag?]`
    /// reading (Portage `dep_check.py`, engine `validate.rs`).
    #[test]
    fn s6_use_deps_satisfied_follows_the_pms_table() {
        let uds_of = |atom: &str| {
            let dep = portage_solver::Dep::parse(atom).expect("parses");
            dep.use_deps.clone().expect("USE deps parse")
        };
        let set = |flags: &[&str]| {
            flags
                .iter()
                .map(|flag| flag.to_string())
                .collect::<std::collections::HashSet<_>>()
        };
        // `[a]`: the target flag decides.
        let enabled = uds_of("dev-libs/pg34-x[a]");
        assert!(super::use_deps_satisfied(
            &enabled,
            &set(&[]),
            &set(&["a"]),
            &set(&["a"])
        ));
        assert!(!super::use_deps_satisfied(
            &enabled,
            &set(&[]),
            &set(&["a"]),
            &set(&[])
        ));
        // `(+/-)` fallback: a flag absent from IUSE reads its default.
        let defaulted = uds_of("dev-libs/pg34-x[a(+)]");
        assert!(super::use_deps_satisfied(
            &defaulted,
            &set(&[]),
            &set(&[]),
            &set(&[])
        ));
        let default_off = uds_of("dev-libs/pg34-x[a(-)]");
        assert!(!super::use_deps_satisfied(
            &default_off,
            &set(&[]),
            &set(&[]),
            &set(&[])
        ));
        // `[!a]`: the target flag must be off.
        let disabled = uds_of("dev-libs/pg34-x[-a]");
        assert!(super::use_deps_satisfied(
            &disabled,
            &set(&[]),
            &set(&["a"]),
            &set(&[])
        ));
        assert!(!super::use_deps_satisfied(
            &disabled,
            &set(&[]),
            &set(&["a"]),
            &set(&["a"])
        ));
        // `[a?]`: parent on implies target on.
        let conditional = uds_of("dev-libs/pg34-x[a?]");
        assert!(super::use_deps_satisfied(
            &conditional,
            &set(&["a"]),
            &set(&["a"]),
            &set(&["a"])
        ));
        assert!(!super::use_deps_satisfied(
            &conditional,
            &set(&["a"]),
            &set(&["a"]),
            &set(&[])
        ));
        assert!(super::use_deps_satisfied(
            &conditional,
            &set(&[]),
            &set(&["a"]),
            &set(&[])
        ));
        // `[!a?]`: parent off implies target off.
        let inverse = uds_of("dev-libs/pg34-x[!a?]");
        assert!(super::use_deps_satisfied(
            &inverse,
            &set(&[]),
            &set(&["a"]),
            &set(&[])
        ));
        assert!(!super::use_deps_satisfied(
            &inverse,
            &set(&[]),
            &set(&["a"]),
            &set(&["a"])
        ));
        assert!(super::use_deps_satisfied(
            &inverse,
            &set(&["a"]),
            &set(&["a"]),
            &set(&["a"])
        ));
        // `[a=]` / `[!a=]`: target and parent agree / disagree.
        let equal = uds_of("dev-libs/pg34-x[a=]");
        assert!(super::use_deps_satisfied(
            &equal,
            &set(&["a"]),
            &set(&["a"]),
            &set(&["a"])
        ));
        assert!(!super::use_deps_satisfied(
            &equal,
            &set(&[]),
            &set(&["a"]),
            &set(&["a"])
        ));
        let equal_inverse = uds_of("dev-libs/pg34-x[!a=]");
        assert!(super::use_deps_satisfied(
            &equal_inverse,
            &set(&[]),
            &set(&["a"]),
            &set(&["a"])
        ));
        assert!(!super::use_deps_satisfied(
            &equal_inverse,
            &set(&["a"]),
            &set(&["a"]),
            &set(&["a"])
        ));
    }

    /// `changed_use_dep_flags` reports exactly the required flags that flip
    /// between the installed USE and the ebuild's effective USE, per kind.
    #[test]
    fn s6_changed_use_dep_flags_reports_only_flips() {
        let uds_of = |atom: &str| {
            let dep = portage_solver::Dep::parse(atom).expect("parses");
            dep.use_deps.clone().expect("USE deps parse")
        };
        let set = |flags: &[&str]| {
            flags
                .iter()
                .map(|flag| flag.to_string())
                .collect::<std::collections::HashSet<_>>()
        };
        let flags = |atom: &str, parent: &[&str], vdb: &[&str], eff: &[&str]| {
            super::changed_use_dep_flags(&uds_of(atom), &set(parent), &set(vdb), &set(eff))
        };
        // Enabled: off in the vdb, on in the ebuild.
        assert_eq!(
            flags("dev-libs/pg34-x[a]", &[], &[], &["a"]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[a]", &[], &["a"], &["a"]).is_empty());
        assert!(flags("dev-libs/pg34-x[a]", &[], &[], &[]).is_empty());
        // Disabled: on in the vdb, off in the ebuild.
        assert_eq!(
            flags("dev-libs/pg34-x[-a]", &[], &["a"], &[]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[-a]", &[], &[], &[]).is_empty());
        // Conditional: parent on, vdb off, ebuild on.
        assert_eq!(
            flags("dev-libs/pg34-x[a?]", &["a"], &[], &["a"]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[a?]", &[], &[], &["a"]).is_empty());
        assert!(flags("dev-libs/pg34-x[a?]", &["a"], &["a"], &["a"]).is_empty());
        // Conditional-inverse: parent off, vdb on, ebuild off.
        assert_eq!(
            flags("dev-libs/pg34-x[!a?]", &[], &["a"], &[]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[!a?]", &["a"], &["a"], &[]).is_empty());
        assert!(flags("dev-libs/pg34-x[!a?]", &[], &["a"], &["a"]).is_empty());
        // Equal: drifted and landing on the parent state.
        assert_eq!(
            flags("dev-libs/pg34-x[a=]", &["a"], &[], &["a"]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[a=]", &["a"], &["a"], &["a"]).is_empty());
        assert!(flags("dev-libs/pg34-x[a=]", &[], &[], &[]).is_empty());
        // Equal-inverse: drifted and landing away from the parent state.
        assert_eq!(
            flags("dev-libs/pg34-x[!a=]", &["a"], &["a"], &[]),
            vec!["a".to_string()]
        );
        assert!(flags("dev-libs/pg34-x[!a=]", &["a"], &[], &[]).is_empty());
        assert!(flags("dev-libs/pg34-x[!a=]", &[], &[], &[]).is_empty());
    }

    /// `or_branch_key` separates a versioned branch from a bare one, so an
    /// override addresses the branch it was computed from (S5 fingerprint).
    #[test]
    fn s6_or_branch_key_separates_versioned_branches() {
        use portage_solver::DepEntry;
        let branch = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one branch")
        };
        let bare = branch("dev-libs/pg34-a");
        let floored = branch(">=dev-libs/pg34-a-1.0");
        assert_ne!(
            super::or_branch_key(&bare),
            super::or_branch_key(&floored),
            "a version floor keys differently from the bare atom"
        );
        assert_eq!(
            super::or_branch_key(&bare),
            super::or_branch_key(&branch("dev-libs/pg34-a"))
        );
    }

    /// `or_subtree_has_blocker` spots blockers behind plain groups, so a
    /// group carrying one is never reordered or trimmed.
    #[test]
    fn s6_or_subtree_has_blocker_spots_nested_blockers() {
        use portage_solver::DepEntry;
        let entries = DepEntry::parse("|| ( dev-libs/pg34-a dev-libs/pg34-b )").expect("parses");
        assert!(!super::or_subtree_has_blocker(&entries));
        let entries = DepEntry::parse("|| ( dev-libs/pg34-a !!dev-libs/pg34-b )").expect("parses");
        assert!(super::or_subtree_has_blocker(&entries));
        let entries = DepEntry::parse("f? ( !dev-libs/pg34-a )").expect("parses");
        assert!(super::or_subtree_has_blocker(&entries));
    }

    /// `visit_or_groups` numbers `||` nodes in pre-order, reaches through
    /// plain, conditional and choice groups, and gates activity on the
    /// member USE (the analysis pass shares this traversal with apply).
    #[test]
    fn s6_visit_or_groups_numbers_and_gates_groups() {
        use portage_solver::DepEntry;
        let visit = |text: &str, use_flags: &[&str]| {
            let entries = DepEntry::parse(text).expect("parses");
            let member_use = use_flags
                .iter()
                .map(|flag| flag.to_string())
                .collect::<std::collections::HashSet<_>>();
            let mut next_index = 0;
            let mut found = Vec::new();
            super::visit_or_groups(&entries, &member_use, true, &mut next_index, &mut found);
            found
        };
        // Two sibling groups take successive indices.
        let found = visit(
            "|| ( dev-libs/pg34-a dev-libs/pg34-b ) || ( dev-libs/pg34-c dev-libs/pg34-d )",
            &[],
        );
        let indices: Vec<usize> = found.iter().map(|(index, _, _)| *index).collect();
        assert_eq!(indices, vec![0, 1], "a stuck counter numbers every group 0");
        // A group behind a plain group is still visited.
        let nested_a = vec![DepEntry::AllOf(
            DepEntry::parse("|| ( dev-libs/pg34-a dev-libs/pg34-b )").expect("parses"),
        )];
        let mut next_index = 0;
        let mut found = Vec::new();
        super::visit_or_groups(
            &nested_a,
            &std::collections::HashSet::new(),
            true,
            &mut next_index,
            &mut found,
        );
        assert_eq!(found.len(), 1, "the nested || is visited: {found:?}");
        assert_eq!(found[0].0, 0);
        // A group behind an unsatisfied conditional is visited but inactive;
        // behind a satisfied one it is active.
        let off = visit("f? ( || ( dev-libs/pg34-a dev-libs/pg34-b ) )", &[]);
        assert_eq!(off.len(), 1);
        assert!(!off[0].2, "an unsatisfied gate deactivates the group");
        let on = visit("f? ( || ( dev-libs/pg34-a dev-libs/pg34-b ) )", &["f"]);
        assert_eq!(on.len(), 1);
        assert!(on[0].2, "a satisfied gate keeps the group active");
        // A group inside `^^` is traversed (it still takes its own
        // override) although never collected itself.
        let atoms = |texts: &[&str]| {
            texts
                .iter()
                .map(|text| {
                    DepEntry::parse(text)
                        .expect("parses")
                        .into_iter()
                        .next()
                        .expect("one atom")
                })
                .collect::<Vec<_>>()
        };
        let entries = vec![DepEntry::ExactlyOneOf(vec![DepEntry::AnyOf(atoms(&[
            "dev-libs/pg34-a",
            "dev-libs/pg34-b",
        ]))])];
        let mut next_index = 0;
        let mut found = Vec::new();
        super::visit_or_groups(
            &entries,
            &std::collections::HashSet::new(),
            true,
            &mut next_index,
            &mut found,
        );
        assert_eq!(found.len(), 1, "the || inside ^^ is visited");
        assert_eq!(found[0].0, 0);
    }

    /// `walk_plain_children` collects only active, non-blocker plain
    /// atoms in written order: inactive atoms never enter, blockers add
    /// no package, `||` branches wait for resolution time, and the USE
    /// gate follows `member_use` (negated for `!flag?`). Internal, no
    /// oracle: this pins the walk input filter, not an emerge row.
    #[test]
    fn s6_walk_plain_children_collects_only_active_plain_atoms() {
        use portage_solver::DepEntry;
        let collect = |text: &str, use_flags: &[&str], active: bool| {
            let parsed = DepEntry::parse(text).expect("parses");
            let member_use = use_flags
                .iter()
                .map(|flag| flag.to_string())
                .collect::<std::collections::HashSet<_>>();
            let mut out = Vec::new();
            super::walk_plain_children(&parsed, &member_use, active, &mut out);
            out
        };
        // Active plain atoms, in written order; the `||` branch waits
        // for resolution and the blocker adds no package.
        assert_eq!(
            collect(
                "dev-libs/pg34-a || ( dev-libs/pg34-b ) !dev-libs/pg34-c",
                &[],
                true
            ),
            vec!["dev-libs/pg34-a".to_string()],
            "only the active plain atom enters"
        );
        // An inactive atom never enters, even without a blocker.
        assert!(
            collect("dev-libs/pg34-a", &[], false).is_empty(),
            "an inactive atom is skipped"
        );
        // The USE gate follows the member USE ...
        assert_eq!(
            collect("f? ( dev-libs/pg34-a )", &["f"], true),
            vec!["dev-libs/pg34-a".to_string()],
            "a satisfied gate collects"
        );
        assert!(
            collect("f? ( dev-libs/pg34-a )", &[], true).is_empty(),
            "an unsatisfied gate drops"
        );
        // ... negated for `!flag?`.
        assert_eq!(
            collect("!f? ( dev-libs/pg34-a )", &[], true),
            vec!["dev-libs/pg34-a".to_string()],
            "a satisfied negated gate collects"
        );
        assert!(
            collect("!f? ( dev-libs/pg34-a )", &["f"], true).is_empty(),
            "an unsatisfied negated gate drops"
        );
    }

    /// `simulate_walk_order` seeds roots first, then resolves one frame
    /// per member and class in LIFO order: every group takes its own
    /// step, taken children enter after their frame's groups, and an
    /// inactive group leaves no resolution behind. Internal, no oracle:
    /// this pins the simulated step counts, not an emerge row.
    #[test]
    fn s6_simulate_walk_order_steps_roots_then_frames() {
        use portage_solver::DepEntry;
        let order = [
            ("dev-libs/wa-m".to_string(), "1".to_string()),
            ("dev-libs/wa-a".to_string(), "1".to_string()),
            ("dev-libs/wa-b".to_string(), "1".to_string()),
        ];
        let parents = [
            (
                "dev-libs/wa-a-1".to_string(),
                vec![("dev-libs".to_string(), "wa-m".to_string())],
            ),
            (
                "dev-libs/wa-b-1".to_string(),
                vec![("dev-libs".to_string(), "wa-m".to_string())],
            ),
        ]
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
        let mut classes: [Vec<DepEntry>; 5] = std::array::from_fn(|_| Vec::new());
        classes[1] =
            DepEntry::parse("z? ( || ( dev-libs/wa-a ) ) || ( dev-libs/wa-a dev-libs/wa-b )")
                .expect("parses");
        let views = [(
            "dev-libs/wa-m-1".to_string(),
            (classes, std::collections::HashSet::new()),
        )]
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
        let walk =
            super::simulate_walk_order(&order, &parents, &["dev-libs/wa-m".to_string()], &views);
        // The root seeds step 0; the inactive `z?` group (index 0)
        // resolves nothing, so the live group takes step 1 and the taken
        // children follow it.
        let added = [
            ("dev-libs/wa-m".to_string(), 0),
            ("dev-libs/wa-a".to_string(), 2),
            ("dev-libs/wa-b".to_string(), 3),
        ]
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(walk.added, added, "adds follow the frame: {:?}", walk.added);
        let resolved = [(("dev-libs/wa-m-1".to_string(), 1, 1), 1)]
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            walk.resolved, resolved,
            "only the live group resolves: {:?}",
            walk.resolved
        );
    }

    // --- #34 S6 throwaway-vdb scaffolding --------------------------------
    //
    // `parent_use_reinstalls` and the `||` analysis read installed deps
    // from the vdb, so G2-shape tests stage their own `var/db/pkg` next
    // to the fixture repo (pmtest owns `fixtures/`; these live in a
    // `TempDir`, like the slotbind test's).

    /// Write `(name, content)` files into `dir`.
    fn s6_write_files(dir: &std::path::Path, files: &[(&str, &str)]) {
        use std::io::Write as _;
        for (name, content) in files {
            let mut file = std::fs::File::create(dir.join(name)).expect("file creates");
            file.write_all(content.as_bytes()).expect("file writes");
        }
    }

    /// One vdb package dir with the metadata files the bridge readers
    /// need (`CATEGORY`, `SLOT`, `repository`, `IUSE`, `USE`, `RDEPEND`).
    #[allow(clippy::too_many_arguments)]
    fn s6_mk_vdb(
        root: &std::path::Path,
        name: &str,
        slot: &str,
        iuse: &str,
        use_flags: &str,
        rdepend: &str,
    ) {
        let dir = root.join("var/db/pkg/dev-libs").join(name);
        std::fs::create_dir_all(&dir).expect("vdb dir creates");
        s6_write_files(
            &dir,
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", &format!("{slot}\n")),
                ("repository", "testrepo\n"),
                ("IUSE", &format!("{iuse}\n")),
                ("USE", &format!("{use_flags}\n")),
                ("RDEPEND", &format!("{rdepend}\n")),
            ],
        );
    }

    /// A throwaway vdb root holding every S6 G2-shape package: repo-backed
    /// members (`capp-1`, `capp-99` resolve against the fixture repo) and
    /// the installed children they pressure. Tests select their actors
    /// through the member plan and the kept set.
    fn s6_temp_vdb() -> std::path::PathBuf {
        let dir = portage_util::TempDir::new("portage-bridge-s6").keep();
        let _ = std::fs::remove_dir_all(&dir);
        s6_mk_vdb(&dir, "pg34-capp-1", "0", "", "", "");
        s6_mk_vdb(&dir, "pg34-capp-99", "0", "", "", "");
        s6_mk_vdb(&dir, "pg34-clib-1", "0", "f", "", "");
        s6_mk_vdb(&dir, "pg34-ulib-1", "0", "x", "", "dev-libs/pg34-cdep");
        s6_mk_vdb(&dir, "pg34-ulib-2", "0", "x", "", "dev-libs/pg34-clib[f]");
        s6_mk_vdb(&dir, "pg34-ulib-99", "0", "x", "", "");
        s6_mk_vdb(&dir, "pg34-cdep-1", "0", "", "", "");
        s6_mk_vdb(&dir, "pg34-g2p-1", "0", "", "", "dev-libs/pg34-ulib[x]");
        s6_mk_vdb(&dir, "pg34-g2p2-1", "0", "", "", "!!dev-libs/pg34-clib[f]");
        s6_mk_vdb(
            &dir,
            "pg34-g2p3-1",
            "0",
            "",
            "",
            "y? ( dev-libs/pg34-clib[f] )",
        );
        s6_mk_vdb(&dir, "pg34-g2p5-1", "0", "", "", "!dev-libs/pg34-cdep");
        s6_mk_vdb(
            &dir,
            "pg34-g2phant-1",
            "0",
            "",
            "",
            "dev-libs/pg34-nosuchpkg",
        );
        dir
    }

    /// A hand-built kept set: the members the test selects, nothing else.
    fn s6_kept(installed: &[&str], rebuild: &[&str], deep: bool) -> super::KeptInstalled {
        super::KeptInstalled {
            installed: installed.iter().map(|cpv| cpv.to_string()).collect(),
            rebuild: rebuild.iter().map(|cpv| cpv.to_string()).collect(),
            downgrade: std::collections::HashSet::new(),
            use_drift: std::collections::HashMap::new(),
            deep,
            deep_allowed: None,
            with_bdeps: false,
        }
    }

    /// `LazyRepo::build` keeps the resolvo order (`seeds_first = false`)
    /// free of vdb expansion: without deep the installed cone never
    /// enters `closure` on that path (S2 `seeds_first`, S3 `vdb_expand`).
    #[test]
    fn s6_closure_skips_vdb_expansion_for_resolvo_order() {
        use super::super::{Deep, find_repos};
        let mut req = fixture_request(&["dev-libs/newpkg"]);
        req.deep = Deep::Unlimited;
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        assert!(shallow.contains(&"dev-libs/pg34-gone".to_string()));
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, false).expect("closure builds");
        let closure: std::collections::HashSet<String> = lazy
            .closure_cpns()
            .into_iter()
            .map(|cpn| format!("{cpn}"))
            .collect();
        // Installed `pg34-gone-1` points at `pg34-gextra` from its vdb,
        // which nothing else reaches: expanding it would pull the extra
        // package into the pool.
        assert!(
            !closure.contains("dev-libs/pg34-gextra"),
            "resolvo-order closure leaves vdb deps unexpanded: {closure:?}"
        );
    }

    /// `LazyRepo::build` never admits a vdb-referenced cp without a repo
    /// dir: the push gate needs both novelty and resolvability, so a
    /// removed package cannot enter `closure` (and the pop dedups the
    /// rest).
    #[test]
    fn s6_closure_ignores_vdb_refs_without_a_repo_dir() {
        use super::super::{Deep, find_repos};
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/newpkg"]);
        req.root = root.clone();
        req.deep = Deep::Unlimited;
        let repos = find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        assert!(shallow.contains(&"dev-libs/pg34-g2phant".to_string()));
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let closure: std::collections::HashSet<String> = lazy
            .closure_cpns()
            .into_iter()
            .map(|cpn| format!("{cpn}"))
            .collect();
        assert!(
            !closure.contains("dev-libs/pg34-nosuchpkg"),
            "a vdb-only reference never enters the closure: {closure:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `root_target_slots` matches installed instances by cp: only slots
    /// of the requested package join the update set (Portage
    /// `_greedy_slots`).
    #[test]
    fn s6_root_target_slots_matches_installed_cps() {
        use super::super::InstalledPackage;
        let (mut req, lazy, _) = pg34_s2_parts(&["dev-libs/pg34-multi"]);
        req.update = true;
        let installed = vec![
            InstalledPackage {
                category: "dev-libs".to_string(),
                package: "pg34-multi".to_string(),
                version: "1".to_string(),
                slot: "1".to_string(),
            },
            InstalledPackage {
                category: "dev-libs".to_string(),
                package: "pg34-unrelated".to_string(),
                version: "1".to_string(),
                slot: "9".to_string(),
            },
        ];
        let dep = portage_solver::Dep::parse("dev-libs/pg34-multi").expect("parses");
        let slots = super::root_target_slots(
            &req,
            &lazy,
            &installed,
            &dep,
            &portage_atom_pubgrub::PortageVersionSet::any(),
        );
        assert_eq!(
            slots,
            vec!["2".to_string(), "1".to_string()],
            "the best slot plus the installed lower slot, never the decoy"
        );
    }

    /// `or_member_view` hides kept leaves: without deep a kept
    /// non-rebuild member contributes no edges, so none of its `||`
    /// groups is evaluated (the S2 leaf rule, mirrored for analysis).
    #[test]
    fn s6_or_member_view_hides_kept_leaves() {
        let (mut req, lazy, installed) = pg34_s2_parts(&["dev-libs/pg34-arg"]);
        // Selective: the named target stays kept, never rebuilt.
        req.selective = true;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(!kept.deep);
        assert!(kept.installed.contains("dev-libs/pg34-arg-1"));
        assert!(!kept.rebuild.contains("dev-libs/pg34-arg-1"));
        assert!(
            super::or_member_view(
                &lazy,
                &kept,
                &installed,
                "dev-libs/pg34-arg",
                "1",
                &super::OrChoiceOverrides::new(),
            )
            .is_none(),
            "a kept leaf has no engine-visible edges"
        );
    }

    /// A version-pinned root atom resolves the pinned version and rebuilds
    /// it in place when installed (Portage selects the pinned instance,
    /// then reinstalls the named argument).
    #[test]
    fn s6_pinned_root_versions_rebuild_in_place() {
        let (mut req, lazy, installed) = pg34_s2_parts(&["=dev-libs/pg34-dlib-1"]);
        req.selective = false;
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-dlib-1"),
            "the pinned installed version rebuilds: {:?}",
            kept.rebuild
        );
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-dlib resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-dlib-1:0"]);
        let dlib = result
            .entries
            .iter()
            .find(|entry| entry.package == "pg34-dlib")
            .expect("pg34-dlib entry");
        assert!(
            matches!(dlib.outcome, super::super::PretendOutcome::Reinstall { .. }),
            "the pinned target reinstalls instead of upgrading: {:?}",
            dlib.outcome
        );
    }

    /// A version-pinned root atom constrains the solve targets: with
    /// nothing installed, `=pg34-sib-1` plans slot 1 even though slot 2
    /// holds the newer version (the target list carries the pin, not
    /// `any`).
    #[test]
    fn s6_pinned_root_versions_constrain_solve_targets() {
        let (mut req, _, _) = pg34_s2_parts(&["=dev-libs/pg34-sib-1"]);
        req.selective = false;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-sib resolves");
        assert_eq!(pg34_plan(&result), ["dev-libs/pg34-sib-1:1"]);
    }

    /// `set_selective_no_update` keeps a satisfied root target at its
    /// installed version when `--update` is off (S4 engine switch,
    /// ported from portage-cli): `-pvN` on `pg34-dlib` plans nothing.
    #[test]
    fn s6_selective_no_update_keeps_satisfied_roots() {
        let (mut req, _, _) = pg34_s2_parts(&["dev-libs/pg34-dlib"]);
        // Selective through `--noreplace`, no update, no deep.
        req.selective = true;
        req.update = false;
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-dlib resolves");
        assert!(
            pg34_plan(&result).is_empty(),
            "a satisfied root stays put without --update: {:?}",
            pg34_plan(&result)
        );
    }

    /// `versions_for` offers each installed version once: an installed
    /// version already in the repo is skipped by the installed-only arm
    /// instead of doubling the row (S3 offer rule).
    #[test]
    fn s6_versions_for_offers_no_duplicate_installed_rows() {
        use portage_atom_pubgrub::PackageRepository;
        let req = pg34_s3_request(&["dev-libs/pg34-arg"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(kept.deep, "the S3 request counts as deep");
        let bridge = super::BridgeRepo {
            lazy,
            kept,
            installed: installed.clone(),
            or_overrides: super::OrChoiceOverrides::new(),
        };
        let cpn = portage_solver::Cpn::parse("dev-libs/pg34-arg").expect("cpn parses");
        let rows = bridge.versions_for(&cpn);
        let mut cpvs: Vec<String> = rows.iter().map(|(cpv, _)| format!("{cpv}")).collect();
        cpvs.sort();
        let mut deduped = cpvs.clone();
        deduped.dedup();
        assert_eq!(
            cpvs, deduped,
            "no installed version is offered twice: {cpvs:?}"
        );
        assert_eq!(cpvs.len(), 1, "the one repo version stands alone: {cpvs:?}");
    }

    /// `or_branch_standing` accepts the branch Portage would take: every
    /// package independently selected, the atom matching the planned
    /// version, the USE deps holding under the member USE.
    #[test]
    fn s6_or_branch_standing_accepts_a_matched_in_graph_branch() {
        use portage_solver::DepEntry;
        // `package.use` enables `x`, like the S3 request below.
        let req = pg34_s3_request(&["dev-libs/pg34-uapp"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let branch = DepEntry::parse("dev-libs/pg34-ulib[x]")
            .expect("parses")
            .into_iter()
            .next()
            .expect("one branch");
        let member_use = ["x".to_string()].into_iter().collect();
        let plan_ver = [("dev-libs/pg34-ulib".to_string(), "1".to_string())]
            .into_iter()
            .collect();
        // The probe target itself requires `ulib`: in-graph trivially.
        let root_cps = ["dev-libs/pg34-ulib".to_string()].into_iter().collect();
        // Walk positions: the alternative is added (step 0, a seeded
        // root) before the member's group resolves (step 1).
        let walk = super::WalkOrder {
            added: [("dev-libs/pg34-ulib".to_string(), 0)]
                .into_iter()
                .collect(),
            resolved: [(("dev-libs/pg34-uapp-1".to_string(), 1, 0), 1)]
                .into_iter()
                .collect(),
        };
        let standing = super::or_branch_standing(
            &lazy,
            &installed,
            &branch,
            "dev-libs/pg34-uapp",
            "dev-libs/pg34-uapp-1",
            1,
            0,
            &walk,
            &member_use,
            &plan_ver,
            &std::collections::HashSet::new(),
            &root_cps,
        );
        assert!(
            standing.qualifies,
            "the matched in-graph branch with holding USE deps qualifies"
        );
    }

    /// `or_branch_standing` weighs the taken edges, not just the roots: a
    /// branch edged only from the member itself is merged solely through
    /// this group and does not qualify, while one edged from another
    /// parent qualifies even with an empty walk (no walk information
    /// keeps the old order-blind answer). Portage prefers the
    /// already-in-graph alternative (`depgraph.py:778-793`); the expected
    /// standings mirror that rule, never portuale's own output.
    #[test]
    fn s6_or_branch_standing_weighs_taken_edges_and_unreached_members() {
        use portage_solver::DepEntry;
        // Same member, branch and USE as the accepts test above.
        let req = pg34_s3_request(&["dev-libs/pg34-uapp"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let branch = DepEntry::parse("dev-libs/pg34-ulib[x]")
            .expect("parses")
            .into_iter()
            .next()
            .expect("one branch");
        let member_use = ["x".to_string()].into_iter().collect();
        let plan_ver = [("dev-libs/pg34-ulib".to_string(), "1".to_string())]
            .into_iter()
            .collect();
        // No roots, no walk entries: the fallback arm answers.
        let root_cps: std::collections::HashSet<String> = std::collections::HashSet::new();
        let walk = super::WalkOrder {
            added: std::collections::HashMap::new(),
            resolved: std::collections::HashMap::new(),
        };
        let standing_for = |taken: std::collections::HashSet<(String, String)>| {
            super::or_branch_standing(
                &lazy,
                &installed,
                &branch,
                "dev-libs/pg34-uapp",
                "dev-libs/pg34-uapp-1",
                1,
                0,
                &walk,
                &member_use,
                &plan_ver,
                &taken,
                &root_cps,
            )
        };
        // Edged only from the member itself: merged solely through this
        // group, so it does not qualify.
        let self_edged = [(
            "dev-libs/pg34-uapp".to_string(),
            "dev-libs/pg34-ulib".to_string(),
        )]
        .into_iter()
        .collect();
        assert!(
            !standing_for(self_edged).qualifies,
            "a self-edged branch is not independently selected"
        );
        // Edged from another parent: independently selected, and the
        // empty walk keeps the order-blind answer.
        let other_edged = [(
            "dev-libs/pg34-other".to_string(),
            "dev-libs/pg34-ulib".to_string(),
        )]
        .into_iter()
        .collect();
        assert!(
            standing_for(other_edged).qualifies,
            "an other-parent edge with no walk history still qualifies"
        );
    }

    /// `or_member_view` reads a deep installed member's classes from the
    /// vdb reader, not the repo md5-cache (S3 installed expansion).
    #[test]
    fn s6_or_member_view_reads_vdb_strings_under_deep() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-ulib"]);
        req.root = root.clone();
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        // Deep, kept, never rebuilt: the vdb supplies the classes.
        let kept = s6_kept(&["dev-libs/pg34-ulib-1"], &[], true);
        let (classes, _) = super::or_member_view(
            &lazy,
            &kept,
            &installed,
            "dev-libs/pg34-ulib",
            "1",
            &super::OrChoiceOverrides::new(),
        )
        .expect("a deep kept member has a view");
        // The staged vdb points at `pg34-cdep`; the repo ebuild gates
        // `pg34-uextra` behind `x?`. The view follows the vdb.
        let rdepend = &classes[1];
        assert!(
            rdepend
                .iter()
                .all(|entry| !matches!(entry, portage_solver::DepEntry::UseConditional { .. })),
            "the vdb classes carry no USE gate: {rdepend:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `or_member_view` answers a rebuild member's USE from the config,
    /// not the vdb: a package Portage rebuilds resolves its conditionals
    /// with the current flags (S4 `desired_use` exemption).
    #[test]
    fn s6_or_member_view_answers_config_use_for_rebuilds() {
        // `--newuse` rebuilds drifted `pg34-ulib` (S4 USE-drift arm).
        let mut req = pg34_s4_request(&["dev-libs/pg34-uapp"]);
        req.newuse = true;
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-ulib-1"),
            "newuse rebuilds the drifted member: {:?}",
            kept.rebuild
        );
        let (_, use_set) = super::or_member_view(
            &lazy,
            &kept,
            &installed,
            "dev-libs/pg34-ulib",
            "1",
            &super::OrChoiceOverrides::new(),
        )
        .expect("a rebuild member has a view");
        assert!(
            use_set.contains("x"),
            "a rebuild resolves conditionals with the config USE: {use_set:?}"
        );
    }

    /// One synthetic merge entry outside any resolve, for the mapping
    /// helpers below (`discovery` stamps input order).
    fn s6_test_entry(
        category: &str,
        package: &str,
        _version: &str,
        outcome: super::super::PretendOutcome,
        deps: Vec<super::super::DepEdge>,
    ) -> super::super::GraphEntry {
        super::super::GraphEntry {
            discovery: 0,
            category: category.to_string(),
            package: package.to_string(),
            outcome,
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: Some("testrepo".to_string()),
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
            source: super::super::CandidateSource::Ebuild,
            provenance: super::super::VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps,
        }
    }

    /// One synthetic dep edge onto `child`, with its buildtime bit.
    fn s6_test_edge(
        child_category: &str,
        child_package: &str,
        atom: &str,
        buildtime: bool,
        key: u8,
    ) -> super::super::DepEdge {
        super::super::DepEdge {
            atom: atom.to_string(),
            evaluated: atom.to_string(),
            category: child_category.to_string(),
            package: child_package.to_string(),
            priority: super::super::DepPriority {
                buildtime,
                runtime: !buildtime,
                runtime_post: false,
                buildtime_slot_op: false,
                runtime_slot_op: false,
                optional: false,
                satisfied: false,
            },
            disjunctive: false,
            alt: None,
            key,
        }
    }

    /// The G2 pass with a deep request expands an installed member from
    /// its vdb strings: `pg34-g2p-1` requires `pg34-ulib[x]` while the
    /// installed `pg34-ulib-1` was built without it, and the config
    /// enables it — the installed child reinstalls with the changed
    /// flag (Portage `_select_atoms` re-evaluation).
    #[test]
    fn s6_parent_use_reinstalls_deep_expands_installed_strings() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-g2p"]);
        req.root = root.clone();
        req.deep = super::super::Deep::Unlimited;
        req.config
            .package_use
            .push(("dev-libs/pg34-ulib".to_string(), vec!["x".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let mut kept = s6_kept(
            &["dev-libs/pg34-g2p-1", "dev-libs/pg34-ulib-1"],
            &["dev-libs/pg34-g2p-1"],
            true,
        );
        let plan = pg34_solved_plan(&[("dev-libs/pg34-g2p", "1")], &[]);
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-ulib-1"),
            "the USE-pressured installed child rebuilds: {:?}",
            kept.rebuild
        );
        assert_eq!(
            kept.use_drift.get("dev-libs/pg34-ulib-1"),
            Some(&vec!["x".to_string()]),
            "the R row carries the changed flag: {:?}",
            kept.use_drift
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Without deep a kept leaf contributes nothing, while a rebuild
    /// member still expands the repo strings (Portage `_ignored_deps`
    /// vs. the reinstalled ebuild node).
    #[test]
    fn s6_parent_use_reinstalls_leaf_members_stay_silent_without_deep() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-capp"]);
        req.root = root.clone();
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        // Rebuild member: the repo `clib[f]` atom pressures the
        // flag-less installed child.
        let mut kept = s6_kept(
            &["dev-libs/pg34-capp-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-capp-1"],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "a rebuild member still exerts USE pressure: {:?}",
            kept.rebuild
        );
        // Kept leaf: `_ignored_deps` means no pressure at all.
        let mut kept = s6_kept(
            &["dev-libs/pg34-capp-1", "dev-libs/pg34-clib-1"],
            &[],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "a kept leaf exerts no USE pressure: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The installed-member expansion follows the solved version's repo
    /// record: a member with no record (an unoffered version) expands
    /// nothing, while the exact record expands the repo strings.
    #[test]
    fn s6_parent_use_reinstalls_repo_record_selects_the_expansion_source() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-capp"]);
        req.root = root.clone();
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        // `capp-99` has no repo record: nothing expands from it.
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "99")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-capp-99", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-capp-99"],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "a record-less member exerts no USE pressure: {:?}",
            kept.rebuild
        );
        // `capp-1` matches its record: the repo `clib[f]` atom applies.
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-capp-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-capp-1"],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "the recorded member pressures its child: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A member version that merely resembles an installed one is still a
    /// fresh member: only the solved version's own installed instance
    /// expands from the vdb (a re-selected downgrade scans through the
    /// fresh branch instead).
    #[test]
    fn s6_parent_use_reinstalls_unmatched_installed_versions_do_not_expand() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-ulib"]);
        req.root = root.clone();
        req.deep = super::super::Deep::Unlimited;
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        // Solved `1` matches `ulib-1`, whose staged vdb strings point at
        // `pg34-cdep` (no USE deps): nothing is pressured even though a
        // `ulib-2` with a `clib[f]` requirement is installed too.
        let plan = pg34_solved_plan(&[("dev-libs/pg34-ulib", "1")], &[]);
        let mut kept = s6_kept(
            &[
                "dev-libs/pg34-ulib-1",
                "dev-libs/pg34-ulib-2",
                "dev-libs/pg34-clib-1",
            ],
            &["dev-libs/pg34-ulib-1", "dev-libs/pg34-ulib-2"],
            true,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "only the solved version's strings exert pressure: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A re-selected member version scans through the fresh branch: a
    /// solved version with no installed instance expands the solved
    /// version's own repo strings, never a same-cp installed sibling's
    /// (the G1-downgrade shape).
    #[test]
    fn s6_parent_use_reinstalls_fresh_versions_scan_their_own_strings() {
        let root = s6_temp_vdb();
        s6_mk_vdb(&root, "pg34-g2multi-1", "0", "", "", "");
        let mut req = fixture_request(&["dev-libs/pg34-g2multi"]);
        req.root = root.clone();
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        // Solved `2` is not installed (only `1` is): the pass reads the
        // `2` strings, whose `clib[f]` atom pressures the flag-less
        // installed child.
        let plan = pg34_solved_plan(&[("dev-libs/pg34-g2multi", "2")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-g2multi-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-g2multi-1"],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "the fresh member pressures its child: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Blocker atoms never exert USE pressure: a `!!` branch with USE
    /// deps is not a build edge (Portage diverts blockers away from the
    /// graph).
    #[test]
    fn s6_parent_use_reinstalls_blocker_atoms_exert_no_use_pressure() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-g2p2"]);
        req.root = root.clone();
        req.deep = super::super::Deep::Unlimited;
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-g2p2", "1")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-g2p2-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-g2p2-1"],
            true,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "a blocker with USE deps still exerts no pressure: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A `flag?` gate the parent USE does not satisfy contributes
    /// nothing: untaken conditionals never pressure a child.
    #[test]
    fn s6_parent_use_reinstalls_unsatisfied_conditionals_contribute_nothing() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-g2p3"]);
        req.root = root.clone();
        req.deep = super::super::Deep::Unlimited;
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-g2p3", "1")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-g2p3-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-g2p3-1"],
            true,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-clib-1"),
            "the untaken gate contributes nothing: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Children already rebuilding (or downgrading) are never
    /// re-pressured: the pass only adds new rebuilds.
    #[test]
    fn s6_parent_use_reinstalls_rebuilt_children_are_not_repressured() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-capp"]);
        req.root = root.clone();
        req.config
            .package_use
            .push(("dev-libs/pg34-clib".to_string(), vec!["f".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-capp", "1")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-capp-1", "dev-libs/pg34-clib-1"],
            &["dev-libs/pg34-capp-1", "dev-libs/pg34-clib-1"],
            false,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert_eq!(
            kept.use_drift.get("dev-libs/pg34-clib-1"),
            None,
            "a rebuilding child gains no new flags: {:?}",
            kept.use_drift
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same-version repo record must also match the slot: an
    /// installed `ulib-99` (slot 0, no `99` record) is an upgrade
    /// question, not a parent-USE reinstall.
    #[test]
    fn s6_parent_use_reinstalls_same_slot_wrong_version_records_do_not_satisfy() {
        let root = s6_temp_vdb();
        let mut req = fixture_request(&["dev-libs/pg34-g2p"]);
        req.root = root.clone();
        req.deep = super::super::Deep::Unlimited;
        req.config
            .package_use
            .push(("dev-libs/pg34-ulib".to_string(), vec!["x".to_string()]));
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let plan = pg34_solved_plan(&[("dev-libs/pg34-g2p", "1")], &[]);
        let mut kept = s6_kept(
            &["dev-libs/pg34-g2p-1", "dev-libs/pg34-ulib-99"],
            &["dev-libs/pg34-g2p-1"],
            true,
        );
        kept.apply_parent_use_reinstalls(&req, &lazy, &installed, &plan);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-ulib-99"),
            "a version without a same-version record never reinstalls: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `apply_or_overrides_to_entries` numbers `||` groups in visit
    /// order: an override addresses its own index, never its neighbour's.
    #[test]
    fn s6_apply_or_overrides_counts_groups_in_order() {
        use portage_solver::DepEntry;
        let group = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one group")
        };
        let key = |entry: &DepEntry| super::or_branch_key(entry);
        let entries = vec![
            group("|| ( dev-libs/pg34-a dev-libs/pg34-b )"),
            group("|| ( dev-libs/pg34-c dev-libs/pg34-d )"),
        ];
        let live_first: Vec<String> = match &entries[0] {
            DepEntry::AnyOf(children) => children.iter().map(key).collect(),
            other => panic!("expected a || group: {other:?}"),
        };
        let live: Vec<String> = match &entries[1] {
            DepEntry::AnyOf(children) => children.iter().map(key).collect(),
            other => panic!("expected a || group: {other:?}"),
        };
        let mut fingerprint = live.clone();
        fingerprint.sort();
        let mut ordered = live.clone();
        ordered.reverse();
        let overrides = super::OrChoiceOverrides {
            groups: [(
                super::OrGroupKey {
                    cpv: "dev-libs/pg34-t-1".to_string(),
                    class: 1,
                    index: 1,
                },
                super::OrGroupOrder {
                    fingerprint,
                    order: ordered.clone(),
                },
            )]
            .into_iter()
            .collect(),
        };
        let mut next_index = 0;
        let out = super::apply_or_overrides_to_entries(
            &entries,
            ("dev-libs/pg34-t-1", 1),
            &overrides,
            &mut next_index,
        );
        // The first group keeps its order; only the second is reordered.
        let order_of = |entry: &DepEntry| match entry {
            DepEntry::AnyOf(children) => children.iter().map(key).collect::<Vec<_>>(),
            other => panic!("expected a || group: {other:?}"),
        };
        assert_eq!(
            order_of(&out[0]),
            live_first,
            "the unaddressed group keeps its order"
        );
        assert_eq!(
            order_of(&out[1]),
            ordered,
            "the addressed group is reordered"
        );
    }

    /// `apply_or_overrides_to_entries` reaches a `||` nested in a plain,
    /// conditional or choice group, while `^^`/`??` groups themselves
    /// keep their kind (S5 task rule).
    #[test]
    fn s6_apply_or_overrides_reaches_nested_groups() {
        use portage_solver::DepEntry;
        let atom = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one atom")
        };
        let key = |entry: &DepEntry| super::or_branch_key(entry);
        let reorder_second = |live: Vec<String>| {
            let mut fingerprint = live.clone();
            fingerprint.sort();
            let mut order = live;
            order.reverse();
            super::OrChoiceOverrides {
                groups: [(
                    super::OrGroupKey {
                        cpv: "dev-libs/pg34-t-1".to_string(),
                        class: 1,
                        index: 0,
                    },
                    super::OrGroupOrder { fingerprint, order },
                )]
                .into_iter()
                .collect(),
            }
        };
        let nested_order = |entry: &DepEntry| match entry {
            DepEntry::AnyOf(children) => children.iter().map(key).collect::<Vec<_>>(),
            other => panic!("expected the nested ||: {other:?}"),
        };
        let live = vec![key(&atom("dev-libs/pg34-a")), key(&atom("dev-libs/pg34-b"))];
        let overrides = reorder_second(live.clone());
        let mut expected = live.clone();
        expected.reverse();
        // Through a plain group.
        let entries = vec![DepEntry::AllOf(vec![DepEntry::AnyOf(vec![
            atom("dev-libs/pg34-a"),
            atom("dev-libs/pg34-b"),
        ])])];
        let mut next_index = 0;
        let out = super::apply_or_overrides_to_entries(
            &entries,
            ("dev-libs/pg34-t-1", 1),
            &overrides,
            &mut next_index,
        );
        let inner = match &out[..] {
            [DepEntry::AllOf(children)] => children,
            other => panic!("the plain group survives: {other:?}"),
        };
        assert_eq!(nested_order(&inner[0]), expected);
        // Through a USE conditional.
        let entries =
            DepEntry::parse("f? ( || ( dev-libs/pg34-a dev-libs/pg34-b ) )").expect("parses");
        let mut next_index = 0;
        let out = super::apply_or_overrides_to_entries(
            &entries,
            ("dev-libs/pg34-t-1", 1),
            &overrides,
            &mut next_index,
        );
        let inner = match &out[..] {
            [DepEntry::UseConditional { children, .. }] => children,
            other => panic!("the conditional survives: {other:?}"),
        };
        assert_eq!(nested_order(&inner[0]), expected);
        // Through a choice group (traversed, never reordered itself).
        let entries = vec![DepEntry::ExactlyOneOf(vec![DepEntry::AnyOf(vec![
            atom("dev-libs/pg34-a"),
            atom("dev-libs/pg34-b"),
        ])])];
        let mut next_index = 0;
        let out = super::apply_or_overrides_to_entries(
            &entries,
            ("dev-libs/pg34-t-1", 1),
            &overrides,
            &mut next_index,
        );
        let inner = match &out[..] {
            [DepEntry::ExactlyOneOf(children)] => children,
            other => panic!("the choice group survives: {other:?}"),
        };
        assert_eq!(nested_order(&inner[0]), expected);
    }

    /// `apply_or_overrides_to_entries` never changes a `^^` group into a
    /// `??` one: choice groups keep their kind while nested `||` nodes
    /// take overrides.
    #[test]
    fn s6_apply_or_overrides_preserves_choice_group_kinds() {
        use portage_solver::DepEntry;
        let atom = DepEntry::parse("dev-libs/pg34-a")
            .expect("parses")
            .into_iter()
            .next()
            .expect("one atom");
        for entry in [
            DepEntry::ExactlyOneOf(vec![atom.clone()]),
            DepEntry::AtMostOneOf(vec![atom.clone()]),
        ] {
            let mut next_index = 0;
            let out = super::apply_or_overrides_to_entries(
                std::slice::from_ref(&entry),
                ("dev-libs/pg34-t-1", 1),
                &super::OrChoiceOverrides::new(),
                &mut next_index,
            );
            match (&entry, &out[..]) {
                (DepEntry::ExactlyOneOf(_), [DepEntry::ExactlyOneOf(_)]) => {}
                (DepEntry::AtMostOneOf(_), [DepEntry::AtMostOneOf(_)]) => {}
                _ => panic!("a choice group changed kind: {out:?}"),
            }
        }
    }

    /// `has_installed_parent` reads vdb edges only: blocker atoms are not
    /// parents, so a block the solution bypasses stays satisfied.
    #[test]
    fn s6_has_installed_parent_ignores_blocker_atoms() {
        let root = s6_temp_vdb();
        let req = fixture_request(&["dev-libs/pg34-g2p5"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let mut req = req;
        req.root = root.clone();
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        // The only edge onto `pg34-cdep` is the `!` block: no parent.
        assert!(
            !super::has_installed_parent(&root, &lazy, "dev-libs", "pg34-cdep", "1"),
            "a blocker atom never parents the blocked package"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `unsolvable_with_installed_parent` keeps a satisfied uninstall for
    /// an entry the run merges: a merge-bound entry for the same cp
    /// replaces rather than removes, so that row already stands.
    #[test]
    fn s6_unsolvable_skips_a_merge_bound_owner() {
        let req = fixture_request(&["dev-libs/pg34-snew"]);
        let repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        // Same package, another category, merged: the replacement row stands.
        let entries = vec![s6_test_entry(
            "sys-libs",
            "pg34-smid",
            "1",
            super::super::PretendOutcome::New {
                version: "1".to_string(),
            },
            Vec::new(),
        )];
        let mut conflicts = vec![super::super::FiledBlocker {
            owner: ("dev-libs".to_string(), "pg34-snew".to_string()),
            owner_version: "1".to_string(),
            conflict: super::super::BlockerConflict {
                atom_str: "!dev-libs/pg34-smid".to_string(),
                strong: false,
                matched_category: "dev-libs".to_string(),
                matched_package: "pg34-smid".to_string(),
                matched_version: "1".to_string(),
                unsolvable: false,
                satisfied_by: Some(super::super::BlockerSatisfiedBy::Uninstall {
                    cpv: "dev-libs/pg34-smid-1".to_string(),
                    anchor: ("dev-libs".to_string(), "pg34-snew".to_string()),
                }),
                tree_scheduled_uninstall: false,
            },
        }];
        super::unsolvable_with_installed_parent(&req.root, &lazy, &entries, &mut conflicts);
        assert!(
            conflicts[0].conflict.unsolvable && conflicts[0].conflict.satisfied_by.is_none(),
            "the installed parent keeps the block unresolved: {:?}",
            conflicts[0].conflict.satisfied_by
        );
    }

    /// `bridge_slot_operator_rebuilds` honours the ignore gate: with
    /// `ignore_built_slot_operator_deps` no stale `:=` consumer
    /// reinstalls, even over entries that rebuild without it.
    #[test]
    fn s6_bridge_slot_operator_rebuilds_honours_the_ignore_gate() {
        let dir = portage_util::TempDir::new("portage-bridge-s6slotop").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let mk = |name: &str, files: &[(&str, &str)]| {
            let d = dir.join("var/db/pkg/dev-libs").join(name);
            std::fs::create_dir_all(&d).unwrap();
            s6_write_files(&d, files);
        };
        mk(
            "slotbindtarget-1.0",
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", "2\n"),
                ("repository", "testrepo\n"),
            ],
        );
        mk(
            "slotbindconsumer-1.0",
            &[
                ("CATEGORY", "dev-libs\n"),
                ("SLOT", "0\n"),
                ("repository", "testrepo\n"),
                ("RDEPEND", "dev-libs/slotbindtarget:2/2=\n"),
            ],
        );
        let mut req = fixture_request(&["dev-libs/slotbindtarget"]);
        req.root = dir.to_path_buf();
        req.solver = super::super::SolverKind::PubGrub;
        req.rebuild_if_new_slot = true;
        req.ignore_built_slot_operator_deps = true;
        req.config.complete_seed_atoms = vec!["dev-libs/slotbindconsumer".to_string()];
        let result = super::super::active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("slotbindtarget resolves");
        assert!(
            result
                .entries
                .iter()
                .all(|e| e.package != "slotbindconsumer"),
            "the stale consumer never rebuilds under the ignore gate: {:?}",
            pg34_plan(&result)
        );
        assert!(
            result.abi_rebuilds.is_empty(),
            "no ABI rebuilds under the ignore gate: {:?}",
            result.abi_rebuilds
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `bridge_circular_display` only hardens build-time edges nothing
    /// installed satisfies: a runtime edge back into the cycle is soft,
    /// so no notice fires (Portage `circular_dependency.py`).
    #[test]
    fn s6_bridge_circular_display_ignores_a_satisfied_runtime_edge() {
        let req = fixture_request(&["dev-libs/newpkg"]);
        // `g2a` build-depends on `g2b`; `g2b` only runtime-depends on
        // `g2a`: the loop breaks at the soft edge.
        let entries = vec![
            s6_test_entry(
                "dev-libs",
                "pg34-g2a",
                "1",
                super::super::PretendOutcome::New {
                    version: "1".to_string(),
                },
                vec![s6_test_edge(
                    "dev-libs",
                    "pg34-g2b",
                    "dev-libs/pg34-g2b",
                    true,
                    0,
                )],
            ),
            s6_test_entry(
                "dev-libs",
                "pg34-g2b",
                "1",
                super::super::PretendOutcome::New {
                    version: "1".to_string(),
                },
                vec![s6_test_edge(
                    "dev-libs",
                    "pg34-g2a",
                    "dev-libs/pg34-g2a",
                    false,
                    1,
                )],
            ),
        ];
        let (circular_deps, large_cycle_count, _) =
            super::bridge_circular_display(&req, &entries, &[0, 1]);
        assert!(
            circular_deps.is_empty(),
            "a runtime back edge breaks the hard cycle: {circular_deps:?}"
        );
        assert!(!large_cycle_count);
    }

    // --- #34 S6 throwaway-repo scaffolding --------------------------------
    //
    // Slot ties and `||` shapes no fixture pins get a synthetic repo next
    // to the fixture one (same `TempDir` pattern as the vdb above): a
    // couple of ebuilds plus real md5-cache entries, so `load_versions`
    // serves them through the shared readers.

    /// One synthetic ebuild plus its md5-cache entry (`_md5_` over the
    /// exact ebuild bytes, like `egencache` writes).
    #[allow(clippy::too_many_arguments)]
    fn s6_repo_pkg(
        repo: &std::path::Path,
        package: &str,
        version: &str,
        slot: &str,
        iuse: &str,
        rdepend: &str,
    ) {
        use std::io::Write as _;
        let dir = repo.join("dev-libs").join(package);
        std::fs::create_dir_all(&dir).expect("pkg dir creates");
        let mut ebuild =
            format!("EAPI=8\nDESCRIPTION=\"s6 fixture\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !iuse.is_empty() {
            ebuild.push_str(&format!("IUSE=\"{iuse}\"\n"));
        }
        if !rdepend.is_empty() {
            ebuild.push_str(&format!("RDEPEND=\"{rdepend}\"\n"));
        }
        let pf = format!("{package}-{version}");
        let mut file =
            std::fs::File::create(dir.join(format!("{pf}.ebuild"))).expect("ebuild creates");
        file.write_all(ebuild.as_bytes()).expect("ebuild writes");
        use md5::Digest as _;
        let digest = format!("{:x}", md5::Md5::digest(ebuild.as_bytes()));
        let mut cache = format!("DESCRIPTION=s6 fixture\nEAPI=8\nKEYWORDS=amd64\nSLOT={slot}\n");
        if !iuse.is_empty() {
            cache.push_str(&format!("IUSE={iuse}\n"));
        }
        if !rdepend.is_empty() {
            cache.push_str(&format!("RDEPEND={rdepend}\n"));
        }
        cache.push_str(&format!("_md5_={digest}\n"));
        let cache_dir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cache_dir).expect("cache dir creates");
        let mut file = std::fs::File::create(cache_dir.join(&pf)).expect("cache creates");
        file.write_all(cache.as_bytes()).expect("cache writes");
    }

    /// A throwaway repo with the S6 slot/`||` shapes, appended after the
    /// fixture repos. Returns the directory (for cleanup) and the extra
    /// `RepoConfig` cloned off the fixture main repo.
    fn s6_temp_repo(
        repos: &[super::super::RepoConfig],
    ) -> (std::path::PathBuf, super::super::RepoConfig) {
        let dir = portage_util::TempDir::new("portage-bridge-s6repo").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        // Two slots racing at the top: slot 1 holds 1.0 and 2.0, slot 2
        // holds 1.5 (per-slot maximum decides the target slot).
        s6_repo_pkg(&repo, "pg34-g2max", "1.0", "1", "", "");
        s6_repo_pkg(&repo, "pg34-g2max", "2.0", "1", "", "");
        s6_repo_pkg(&repo, "pg34-g2max", "1.5", "2", "", "");
        // A `vercmp` tie across slots: `1.0-r0` equals `1.0`.
        s6_repo_pkg(&repo, "pg34-g2tie", "1.0", "1", "", "");
        s6_repo_pkg(&repo, "pg34-g2tie", "1.0-r0", "2", "", "");
        // Two versions with USE pressure only on the second.
        s6_repo_pkg(&repo, "pg34-g2multi", "1", "0", "", "");
        s6_repo_pkg(&repo, "pg34-g2multi", "2", "0", "", "dev-libs/pg34-clib[f]");
        // An in-graph-vs-installed `||` alternative (S5 G5 shape).
        s6_repo_pkg(
            &repo,
            "pg34-g2or",
            "1",
            "0",
            "",
            "|| ( dev-libs/pg34-ulib dev-libs/pg34-clib )",
        );
        // A USE-gated `||` (inactive without `y`).
        s6_repo_pkg(
            &repo,
            "pg34-g2cond",
            "1",
            "0",
            "y",
            "y? ( || ( dev-libs/pg34-ga dev-libs/pg34-gb ) )",
        );
        // #366 walk-order shapes (synthetic, no pmtest fixture): `a`
        // first-lists `x`, but `x` pulls `b` which pulls `y` — Portage
        // resolves the group with only `{r, a}` in the graph and takes
        // `x`, while a plan-order-blind test takes the later `y`.
        s6_repo_pkg(&repo, "pg366o-r", "1", "0", "", "dev-libs/pg366o-a");
        s6_repo_pkg(
            &repo,
            "pg366o-a",
            "1",
            "0",
            "",
            "|| ( dev-libs/pg366o-x dev-libs/pg366o-y )",
        );
        s6_repo_pkg(&repo, "pg366o-x", "1", "0", "", "dev-libs/pg366o-b");
        s6_repo_pkg(&repo, "pg366o-b", "1", "0", "", "dev-libs/pg366o-y");
        s6_repo_pkg(&repo, "pg366o-y", "1", "0", "", "");
        // #366 shape (a): the root pulls `b` (hence `y`) alongside `a`,
        // so `y` is in the graph when `a`'s disjunction resolves.
        s6_repo_pkg(
            &repo,
            "pg366o-t",
            "1",
            "0",
            "",
            "dev-libs/pg366o-a dev-libs/pg366o-b",
        );
        // #366 shape (b): `b` listed first; the disjunction still
        // resolves after `b` is walked, so `y` wins here too.
        s6_repo_pkg(
            &repo,
            "pg366o-tb",
            "1",
            "0",
            "",
            "dev-libs/pg366o-b dev-libs/pg366o-a",
        );
        let mut extra = repos[0].clone();
        extra.name = "s6test".to_string();
        extra.location = repo;
        (dir, extra)
    }

    /// `target_slots` breaks a cross-slot `vercmp` tie by slot: with
    /// `1.0-r0` equalling `1.0`, only the lower slot is the target, and an
    /// installed equal slot never rejoins under `--update` (Portage
    /// `_greedy_slots` keeps slots whose best is *lower* than the best).
    #[test]
    fn s6_target_slots_breaks_version_ties_by_slot() {
        let req = fixture_request(&["dev-libs/pg34-g2tie"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let slots = super::target_slots(
            &lazy,
            "dev-libs/pg34-g2tie",
            &portage_atom_pubgrub::PortageVersionSet::any(),
            &["2".to_string()],
            true,
        );
        assert_eq!(
            slots,
            vec!["1".to_string()],
            "the tied installed slot never rejoins: {slots:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `target_slots` takes each slot's own maximum: with slot 1 peaking
    /// at 2.0 over slot 2's 1.5, the bare target selects slot 1 alone.
    #[test]
    fn s6_target_slots_picks_the_greatest_per_slot_version() {
        let req = fixture_request(&["dev-libs/pg34-g2max"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let slots = super::target_slots(
            &lazy,
            "dev-libs/pg34-g2max",
            &portage_atom_pubgrub::PortageVersionSet::any(),
            &[],
            false,
        );
        assert_eq!(
            slots,
            vec!["1".to_string()],
            "slot 1 peaks at 2.0: {slots:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `KeptInstalled::of` keeps each slot's own maximum: the installed
    /// `g2max-1.0` is not the best accepted version of its slot (2.0
    /// is), so it never joins the rebuild set.
    #[test]
    fn s6_kept_of_picks_the_greatest_same_slot_version() {
        let root = s6_temp_vdb();
        s6_mk_vdb(&root, "pg34-g2max-1.0", "1", "", "", "");
        let mut req = fixture_request(&["dev-libs/pg34-g2max"]);
        req.root = root.clone();
        req.selective = false;
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = super::KeptInstalled::of(&req, &lazy, &installed);
        assert!(
            !kept.rebuild.contains("dev-libs/pg34-g2max-1.0"),
            "a shadowed installed version never rebuilds: {:?}",
            kept.rebuild
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `compute_or_overrides` moves the branch Portage's walk already holds
    /// first: the root pulls `b` (hence `y`) alongside `a`, so `y` is in
    /// the graph when `a`'s disjunction resolves (#366 shape (a)). The
    /// engine's final fallback is the first-listed alternative, and
    /// nothing here is installed.
    #[test]
    fn s6_compute_or_overrides_moves_the_desired_branch_first() {
        use portage_solver::DepEntry;
        let req = fixture_request(&["dev-libs/pg366o-t"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let kept = s6_kept(&[], &[], false);
        // Round 1: the engine takes first-listed `x`; `y` rides in via `b`.
        let order = vec![
            ("dev-libs/pg366o-t".to_string(), "1".to_string()),
            ("dev-libs/pg366o-a".to_string(), "1".to_string()),
            ("dev-libs/pg366o-x".to_string(), "1".to_string()),
            ("dev-libs/pg366o-b".to_string(), "1".to_string()),
            ("dev-libs/pg366o-y".to_string(), "1".to_string()),
        ];
        let parents = [
            (
                "dev-libs/pg366o-a-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-t".to_string())],
            ),
            (
                "dev-libs/pg366o-b-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-t".to_string())],
            ),
            (
                "dev-libs/pg366o-x-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-a".to_string())],
            ),
            (
                "dev-libs/pg366o-y-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-b".to_string())],
            ),
        ]
        .into_iter()
        .collect();
        let root_cps = ["dev-libs/pg366o-t".to_string()].into_iter().collect();
        let overrides = super::compute_or_overrides(
            &req,
            &lazy,
            &kept,
            &[],
            &order,
            &parents,
            &root_cps,
            &super::OrChoiceOverrides::new(),
        );
        let group = overrides
            .groups
            .get(&super::OrGroupKey {
                cpv: "dev-libs/pg366o-a-1".to_string(),
                class: 1,
                index: 0,
            })
            .expect("the member || group is addressed");
        // `y` is in the graph before the group resolves; it moves ahead
        // of first-listed `x` while every branch survives.
        let branch = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one branch")
        };
        assert_eq!(
            group.order,
            vec![
                super::or_branch_key(&branch("dev-libs/pg366o-y")),
                super::or_branch_key(&branch("dev-libs/pg366o-x")),
            ],
            "the in-graph branch moves first, nothing is dropped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `compute_or_overrides` keeps the written order when the other
    /// parent joins the graph only later: `x` pulls `b` which pulls `y`,
    /// so Portage resolves `a`'s group with only `{r, a}` present and
    /// takes first-listed `x` (#366 pager shape). A plan-order-blind
    /// "selected anywhere" test moves `y` first instead.
    #[test]
    fn s6_compute_or_overrides_ignores_an_alternative_pulled_later() {
        use portage_solver::DepEntry;
        let req = fixture_request(&["dev-libs/pg366o-r"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let kept = s6_kept(&[], &[], false);
        // Round 1: the engine takes first-listed `x`; `y` arrives three
        // edges later through `x` itself.
        let order = vec![
            ("dev-libs/pg366o-r".to_string(), "1".to_string()),
            ("dev-libs/pg366o-a".to_string(), "1".to_string()),
            ("dev-libs/pg366o-x".to_string(), "1".to_string()),
            ("dev-libs/pg366o-b".to_string(), "1".to_string()),
            ("dev-libs/pg366o-y".to_string(), "1".to_string()),
        ];
        let parents = [
            (
                "dev-libs/pg366o-a-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-r".to_string())],
            ),
            (
                "dev-libs/pg366o-x-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-a".to_string())],
            ),
            (
                "dev-libs/pg366o-b-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-x".to_string())],
            ),
            (
                "dev-libs/pg366o-y-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-b".to_string())],
            ),
        ]
        .into_iter()
        .collect();
        let root_cps = ["dev-libs/pg366o-r".to_string()].into_iter().collect();
        let overrides = super::compute_or_overrides(
            &req,
            &lazy,
            &kept,
            &[],
            &order,
            &parents,
            &root_cps,
            &super::OrChoiceOverrides::new(),
        );
        let group = overrides
            .groups
            .get(&super::OrGroupKey {
                cpv: "dev-libs/pg366o-a-1".to_string(),
                class: 1,
                index: 0,
            })
            .expect("the member || group is addressed");
        // Neither branch is in the graph at resolution: `x` is only the
        // group's own pick, `y` arrives later — the written order stands.
        let branch = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one branch")
        };
        assert_eq!(
            group.order,
            vec![
                super::or_branch_key(&branch("dev-libs/pg366o-x")),
                super::or_branch_key(&branch("dev-libs/pg366o-y")),
            ],
            "a later-pulled alternative never moves first"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `compute_or_overrides` resolves shape (b) like shape (a): with `b`
    /// listed first, `a`'s disjunction still waits for the plain stack to
    /// drain, so `y` is in the graph at resolution (#366).
    #[test]
    fn s6_compute_or_overrides_moves_the_desired_branch_first_reversed() {
        use portage_solver::DepEntry;
        let req = fixture_request(&["dev-libs/pg366o-tb"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let kept = s6_kept(&[], &[], false);
        let order = vec![
            ("dev-libs/pg366o-tb".to_string(), "1".to_string()),
            ("dev-libs/pg366o-a".to_string(), "1".to_string()),
            ("dev-libs/pg366o-x".to_string(), "1".to_string()),
            ("dev-libs/pg366o-b".to_string(), "1".to_string()),
            ("dev-libs/pg366o-y".to_string(), "1".to_string()),
        ];
        let parents = [
            (
                "dev-libs/pg366o-a-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-tb".to_string())],
            ),
            (
                "dev-libs/pg366o-b-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-tb".to_string())],
            ),
            (
                "dev-libs/pg366o-x-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-a".to_string())],
            ),
            (
                "dev-libs/pg366o-y-1".to_string(),
                vec![("dev-libs".to_string(), "pg366o-b".to_string())],
            ),
        ]
        .into_iter()
        .collect();
        let root_cps = ["dev-libs/pg366o-tb".to_string()].into_iter().collect();
        let overrides = super::compute_or_overrides(
            &req,
            &lazy,
            &kept,
            &[],
            &order,
            &parents,
            &root_cps,
            &super::OrChoiceOverrides::new(),
        );
        let group = overrides
            .groups
            .get(&super::OrGroupKey {
                cpv: "dev-libs/pg366o-a-1".to_string(),
                class: 1,
                index: 0,
            })
            .expect("the member || group is addressed");
        let branch = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one branch")
        };
        assert_eq!(
            group.order,
            vec![
                super::or_branch_key(&branch("dev-libs/pg366o-y")),
                super::or_branch_key(&branch("dev-libs/pg366o-x")),
            ],
            "dep order never decides the winner: the in-graph branch moves first"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `compute_or_overrides` keeps a group with no independently-selected
    /// branch verbatim: without a desired pick the engine and Portage
    /// already agree, and dropping the installed branch would force the
    /// untaken alternative instead (S5 G5 `xapp` guard).
    #[test]
    fn s6_compute_or_overrides_keeps_groups_without_a_desired_pick() {
        use portage_solver::DepEntry;
        let req = fixture_request(&["dev-libs/pg34-g2or"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let installed = super::super::all_installed_packages(&req.root);
        let kept = s6_kept(&[], &[], false);
        // Neither branch is selected anywhere: no desired pick, while the
        // installed `clib-1` satisfies its branch (installed-blocking).
        let order = vec![("dev-libs/pg34-g2or".to_string(), "1".to_string())];
        let parents = std::collections::HashMap::new();
        let root_cps = std::collections::HashSet::new();
        let overrides = super::compute_or_overrides(
            &req,
            &lazy,
            &kept,
            &installed,
            &order,
            &parents,
            &root_cps,
            &super::OrChoiceOverrides::new(),
        );
        let group = overrides
            .groups
            .get(&super::OrGroupKey {
                cpv: "dev-libs/pg34-g2or-1".to_string(),
                class: 1,
                index: 0,
            })
            .expect("the member || group is addressed");
        let branch = |text: &str| {
            DepEntry::parse(text)
                .expect("parses")
                .into_iter()
                .next()
                .expect("one branch")
        };
        assert_eq!(
            group.order,
            vec![
                super::or_branch_key(&branch("dev-libs/pg34-ulib")),
                super::or_branch_key(&branch("dev-libs/pg34-clib")),
            ],
            "without a desired pick every branch survives in place"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `compute_or_overrides` skips groups behind an unsatisfied USE
    /// gate: an inactive group emits no facts, so later passes cannot
    /// chase a branch Portage never evaluates.
    #[test]
    fn s6_compute_or_overrides_skips_inactive_groups() {
        let req = fixture_request(&["dev-libs/pg34-g2cond"]);
        let mut repos =
            super::super::find_repos(&req.config_root).expect("fixture repos.conf must resolve");
        let (_dir, extra) = s6_temp_repo(&repos);
        let dir = extra.location.parent().expect("repo parent").to_path_buf();
        repos.push(extra);
        let (seeds, shallow) = super::closure_seeds(&req).expect("seeds parse");
        let lazy =
            super::LazyRepo::build(&req, repos, &seeds, &shallow, true).expect("closure builds");
        let kept = s6_kept(&[], &[], false);
        let order = vec![("dev-libs/pg34-g2cond".to_string(), "1".to_string())];
        let parents = std::collections::HashMap::new();
        let root_cps = std::collections::HashSet::new();
        let overrides = super::compute_or_overrides(
            &req,
            &lazy,
            &kept,
            &[],
            &order,
            &parents,
            &root_cps,
            &super::OrChoiceOverrides::new(),
        );
        assert!(
            overrides.groups.is_empty(),
            "the gated group emits no facts: {:?}",
            overrides.groups
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
