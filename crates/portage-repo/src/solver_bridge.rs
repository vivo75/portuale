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
use std::fmt;
use std::io::Write;

use super::{
    Deep, EffectiveUseFlagsArgs, GraphEntry, GraphResult, InstalledDepStringArgs,
    InstalledMetaLayer, InstalledRef, InstalledUseState, PretendOutcome,
    ReinstallFlagsForUseChangeArgs, RepoConfig, ResolveRequest, Resolver as PortualeResolver,
    all_installed_packages, effective_use_flags, find_repos, installed_candidates,
    installed_dep_string, installed_pkg_iuse_and_use, is_visible, list_candidates,
    live_metadata_for_installed, read_vdb_flag_set, read_vdb_string,
    reinstall_flags_for_use_change, repo_aux_metadata, split_slot,
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
    root: std::path::PathBuf,
    dynamic_deps: bool,
    ignore_built_slot_operator_deps: bool,
    /// The closure: every CPN the solver may see (`all_packages`).
    closure: HashSet<String>,
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
// S6 owns the `graph_result_from_order` split; until then the sixth parameter
// rides an explicit allow instead of a one-use args struct.
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
        let Some(record) = record else {
            continue;
        };
        let use_set = repo.use_of(&record);
        let slot_versions = installed.by_slot.get(&(
            record.category.clone(),
            record.package.clone(),
            record.slot.clone(),
        ));
        let (outcome, oldbest, new_slot) = match slot_versions {
            Some(versions) if versions.iter().any(|v| v == version) => {
                // #34 S2: a same-version pick the bridge asked to rebuild is a
                // bare, reasonless `Reinstall` (precedent `resolve/pretend.rs:170-179`).
                // #34 S4: a USE-drift rebuild carries its changed flags.
                let cpv = format!("{cp}-{version}");
                let outcome = match kept {
                    Some(kept) if kept.rebuild.contains(&cpv) => PretendOutcome::Reinstall {
                        version: version.clone(),
                        changed_flags: kept.use_drift.get(&cpv).cloned().unwrap_or_default(),
                        deps_changed: false,
                        slot_changed: false,
                        rebuilt_binary: false,
                        new_repo: false,
                        slot_operator_rebuild: false,
                    },
                    _ => PretendOutcome::AlreadyInstalled {
                        version: version.clone(),
                    },
                };
                (outcome, Vec::new(), false)
            }
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
        // #34 S4: an `R` row diffs against the vdb USE like the walk's own
        // display (`InstalledUseState`, `USE` intersected with `IUSE`), so a
        // drifted flag renders `x*`; anything else keeps no markers. Gated
        // on the outcome, so resolvo (which never yields `Reinstall`) and
        // every non-`R` row do not move.
        let (installed_state, reinst_flags): (Option<InstalledUseState>, HashSet<String>) =
            match &outcome {
                PretendOutcome::Reinstall { changed_flags, .. } => {
                    let old_iuse = read_vdb_flag_set(
                        &req.root,
                        &record.category,
                        &record.package,
                        version,
                        "IUSE",
                    );
                    let mut old_use = read_vdb_flag_set(
                        &req.root,
                        &record.category,
                        &record.package,
                        version,
                        "USE",
                    );
                    old_use.retain(|flag| old_iuse.contains(flag));
                    (
                        Some(InstalledUseState { old_use, old_iuse }),
                        changed_flags.iter().cloned().collect(),
                    )
                }
                _ => (None, HashSet::new()),
            };
        let use_expand_display =
            super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
                use_flags_display: &use_flags_display,
                config: &req.config,
                installed: installed_state.as_ref(),
                forced: &forced,
                all_flags: true,
                reinst_flags: &reinst_flags,
            });
        let use_expand_display_p =
            super::build_use_expand_display(super::BuildUseExpandDisplayArgs {
                use_flags_display: &use_flags_display,
                config: &req.config,
                installed: installed_state.as_ref(),
                forced: &forced,
                all_flags: false,
                reinst_flags: &reinst_flags,
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
    let mut slots = vec![top_slot];
    if update {
        for slot in installed_slots {
            if !slots.contains(slot) && best.get(slot.as_str()).is_some_and(|v| *v < top) {
                slots.push(slot.clone());
            }
        }
    }
    slots
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
        Self {
            installed: all,
            rebuild,
            use_drift,
            deep,
            downgrade: Self::downgrade_set(req, lazy, installed, deep),
        }
    }

    /// Installed cpvs with no visible same-slot version `>=` the installed one
    /// that Portage would re-select away from: a named target (`arg_atoms`),
    /// under emptytree (complete mode re-resolves everything from the repo),
    /// or under `--update` with deep (the deep walk reaches the node with
    /// `want_update` true). Without update/deep Portage keeps the installed
    /// instance (`_want_update_pkg` false → the installed early return), so
    /// the set stays empty there and registration/offering are unchanged.
    /// Portage: depgraph.py:2946-2957 (`_downgrade_probe`), :3805-3809
    /// (`want_update`), :7304-7333 (`_want_update_pkg`).
    fn downgrade_set(
        req: &ResolveRequest,
        lazy: &LazyRepo,
        installed: &[super::InstalledPackage],
        deep: bool,
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
        let mut downgrade = HashSet::new();
        for inst in installed {
            let cp = format!("{}/{}", inst.category, inst.package);
            if !named.contains(&cp) && !update_walk {
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
            // expanding them. The trace then shows no edges for it, like the engine.
            // Portage: depgraph.py:3815-3824, :7359-7375
            let leaf = !self.kept.deep
                && self.kept.installed.contains(&record.cpv())
                && !self.kept.rebuild.contains(&record.cpv());
            // #34 S3: under deep, an installed cpv's deps come from the vdb
            // (the default resolver's dynamic-deps reader), not the repo
            // md5-cache: Portage expands the installed node with the flags it
            // was built with. Portage: FakeVartree.py, _apply_dynamic_deps.
            let vdb_classes;
            let classes: &[Vec<portage_solver::DepEntry>; 5] = match (
                self.kept.deep && self.kept.installed.contains(&record.cpv()),
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
            let deps = if leaf { empty } else { classes.clone() };
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
                if let Some(row) = version_row(&record, record.deps.clone()) {
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
        // `R pg34-ulib` pulling `N pg34-uextra`).
        if self.kept.deep
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
fn pubgrub_provider(
    req: &ResolveRequest,
    lazy: &LazyRepo,
    kept: &KeptInstalled,
    installed: &[super::InstalledPackage],
    seed_cpns: &[portage_solver::Cpn],
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
    provider.set_prefer_newest_slot(deep || req.empty);
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

fn resolve_pubgrub(req: &ResolveRequest) -> Result<GraphResult, String> {
    use portage_atom_pubgrub::{PortagePackage, PortageVersionSet};
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
    let kept = KeptInstalled::of(req, &lazy, &installed);
    let mut provider = pubgrub_provider(req, &lazy, &kept, &installed, &seed_cpns);
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
                let slots = root_target_slots(req, &lazy, &installed, &dep, &versions);
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
    let solution = provider
        .resolve_targets(targets)
        .map_err(portage_atom_pubgrub::format_solve_error)?;
    let mut order = Vec::new();
    for (pkg, ver) in provider.install_order(&solution) {
        if let portage_atom_pubgrub::PortagePackage::Real { cpn, .. } = &pkg {
            pg_trace(&mut std::io::stderr(), format_args!("pg-plan {cpn}-{ver}"));
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
        Some(&kept),
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
        let provider = super::pubgrub_provider(&req, &lazy, &kept, &installed, &seed_cpns);
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
        let provider = super::pubgrub_provider(&req, &lazy, &kept, &installed, &seed_cpns);
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
        let bridge = super::BridgeRepo { lazy, kept };
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
        let bridge = super::BridgeRepo { lazy, kept };
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
        let bridge = super::BridgeRepo { lazy, kept };
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

    /// #34 S3 non-regression: `-pvuD` on pg34-blib plans nothing (Portage
    /// `Total: 0`): an installed, already-built parent's BDEPEND is not
    /// traversed for update when the parent itself is not merged. The
    /// installed-BDEPEND fold stays cut pending bed evidence (code note).
    #[test]
    fn pg34_blib_pubgrub_pvud_plans_nothing() {
        let req = pg34_s3_request(&["dev-libs/pg34-blib"]);
        let result = active_resolver_for(super::super::SolverKind::PubGrub)
            .resolve(&req)
            .expect("pg34-blib resolves");
        assert_eq!(pg34_plan(&result), Vec::<String>::new());
        assert!(
            result
                .entries
                .iter()
                .all(|e| !format!("{}/{}", e.category, e.package).contains("pg34-btool")),
            "no btool row in any outcome: {:?}",
            result
                .entries
                .iter()
                .map(|e| format!("{}/{} {:?}", e.category, e.package, e.outcome))
                .collect::<Vec<_>>()
        );
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
}
