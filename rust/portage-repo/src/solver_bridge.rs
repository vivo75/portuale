//! Alternate `--solver=` backends: PubGrub and resolvo over the same repo facts.
//!
//! Portuale-only (real `emerge` has no `--solver`): [`super::SolverKind`]'s
//! `PubGrub`/`Resolvo` variants land here. Both backends drive lu-zero's
//! `portage-atom-pubgrub` / `portage-atom-resolvo` bridges (see
//! `docs/solver-backends-analysis.md` and the `3rdparty/portage-cli`
//! checkout) instead of the backtracking walk -- the same md5-cache facts
//! in, a [`super::GraphResult`] out, so `pretend.rs` never knows which
//! algorithm resolved the graph.
//!
//! The feeding direction is exactly the bridges' documented boundary ("a
//! solver over facts"): portuale computes all policy (per-version resolved
//! USE via [`super::effective_use_flags`], target atoms, installed set)
//! and hands it in; the engines only pick versions.
//!
//! Performance shape (measured: the engines themselves take ~20ms; the
//! adapter must not dominate): [`LazyRepo`] loads md5-cache facts **on
//! demand per `cat/pkg`**, memoized, over a **reachability closure**
//! BFS-seeded from the target + installed CPNs. md5 reads, dep-string
//! parses and (the dominant cost, ~1ms each) `effective_use_flags` runs
//! therefore scale with the request's closure, not the tree -- the same
//! complexity shape as the backtracking walk's own BFS. The closure
//! over-approximates (both sides of every `flag?()` conditional are
//! followed, USE unevaluated), so reachability never drops a package the
//! solver could select; `all_packages` exposes exactly the closure, which
//! keeps dropped-dependency filtering sound. The PubGrub provider is built
//! with `new_for_targets` (reachable conversion, not whole-tree); the
//! resolvo pool is populated from closure records only (its construction
//! is inherently whole-input).
//!
//! Deliberate v1 cuts (each would be its own slice):
//! - No autounmask relaxation levels: the pool *is* visibility-filtered
//!   with the walk's own `is_visible` (keyword/license/mask/PROPERTIES/
//!   RESTRICT acceptance), so an invisible version is never offered --
//!   but unlike the walk, a candidate that would need a `--autounmask*`
//!   flip (a `~arch` keyword, a license, a `package.mask`) simply fails
//!   to resolve rather than producing the flip suggestion.
//! - No notices: slot conflicts, autounmask change lists, `:=` rebuilds,
//!   blockers and circular deps come back empty -- a bridge failure is
//!   one `Error::Detail` line, not a real `depgraph.py` notice.
//! - Merge order is the plan's install order (`GraphEntry::deps` empty).
//! - A dep-string class that `DepEntry::parse` rejects feeds empty deps
//!   for that class; a version `Cpv`/`Version`-unparsable by
//!   `portage_atom` is skipped; top-level blocker atoms are skipped.
//! - USE-dep / `::repo` / slot-operator target forms resolve the way the
//!   bridges resolve them, which may differ from the walk.
//! - `oldbest` carries the resolved candidate's own sub-slot/repo (an
//!   approximation of the installed one's) and new-slot `New` entries
//!   list installed versions with empty sub-slot/repo.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::{
    GraphEntry, GraphResult, InstalledRef, PretendOutcome, RepoConfig, ResolveRequest,
    Resolver as PortualeResolver, all_installed_packages, effective_use_flags, find_repos,
    installed_pkg_iuse_and_use, is_visible, list_candidates, read_md5_cache,
};

// --- Lazy fact loading -----------------------------------------------------

/// One repo version with everything a bridge needs except resolved USE
/// (computed on demand by [`LazyRepo::use_of`]): parsed dep trees per
/// class plus identity back into `GraphEntry` fields.
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
}

impl LoadedVersion {
    /// `category/package-version` spelling (the plan/map join key).
    fn cpv(&self) -> String {
        format!("{}/{}-{}", self.category, self.package, self.version)
    }
}

/// Collect every CPN referenced by parsed dep trees (both conditional
/// branches, blockers included): the over-approximating closure edge.
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

/// Whether `cat/pkg` names a real repo directory (any repo): the closure
/// admits only resolvable names, so a dropped dependency stays genuinely
/// absent downstream.
fn cp_exists(repos: &[RepoConfig], cp: &str) -> bool {
    let Some((category, package)) = cp.split_once('/') else {
        return false;
    };
    repos
        .iter()
        .any(|repo| repo.location.join(category).join(package).is_dir())
}

/// Repo facts loaded on demand per `cat/pkg` and memoized, over a
/// reachability closure BFS-seeded from the target + installed CPNs.
///
/// The closure is computed from parsed dep trees *without* USE evaluation
/// (both conditional branches followed), so it is a superset of whatever
/// the solver can reach; `effective_use_flags` then runs only for versions
/// the solver actually queries via `use_of`. Everything scales with the
/// request's closure, never the tree.
///
/// `Clone` shares nothing but copies the memoized maps (cheap: the
/// closure is fully loaded at `build` time): the provider takes one copy
/// while the `Plan -> GraphResult` mapping keeps the other, so USE
/// computed during the solve stays cached provider-side and only the
/// selected versions re-resolve mapping-side.
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
    /// Build the closure from `seeds` (target CPNs, fully expanded) and
    /// `shallow` (installed CPNs: versions loaded for favored-version
    /// registration and upgrade comparison, but their own dep cones NOT
    /// expanded -- nothing explores them unless the target cone reaches
    /// them, in which case the target expansion loads them anyway):
    /// BFS over parsed dep trees, loading (md5 read + parse, no USE) per
    /// visited `cat/pkg`.
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
                // Shallow: versions must be present for the solver, but
                // the cone stops here.
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
                // The walk resolves from a visibility-filtered pool; the
                // bridges get the same pool (keyword/license/mask/PROPERTIES/
                // RESTRICT acceptance via the walk's own `is_visible`), so an
                // invisible version is never offered to either engine.
                if !is_visible(candidate, category, package, &self.config) {
                    continue;
                }
                let pf = format!("{package}-{}", candidate.version);
                let Ok(metadata) = read_md5_cache(&candidate.repo_location, category, &pf) else {
                    continue;
                };
                // Skip versions outside `portage_atom`'s own version grammar.
                if portage_solver::Cpv::parse(&format!("{cp}-{}", candidate.version)).is_err() {
                    continue;
                }
                let mut deps: [Vec<portage_solver::DepEntry>; 5] = Default::default();
                for (i, key) in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
                    .iter()
                    .enumerate()
                {
                    if let Some(text) = metadata.get(*key)
                        && let Ok(parsed) = portage_solver::DepEntry::parse(text)
                    {
                        deps[i] = parsed;
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
        let use_set = effective_use_flags(
            &self.config,
            &version.iuse,
            &version.keywords,
            &candidate_str,
            &version.category,
            &version.package,
        );
        self.uses.borrow_mut().insert(cpv, use_set.clone());
        use_set
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

/// Target + installed CPN seeds for the closure: every target atom's CPN
/// (blockers excluded -- an unmerge action, not a merge target) plus every
/// installed `cat/pkg` (favored-version registration and upgrade
/// comparison need their repo versions present).
/// Target + installed CPN seeds for the closure: every target atom's CPN
/// (blockers excluded -- an unmerge action, not a merge target) expands
/// fully; every installed `cat/pkg` loads shallowly (favored-version
/// registration and upgrade comparison need its repo versions present,
/// not its dep cone).
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

/// Split an `IUSE` string into flag names, stripping the `+`/`-` default
/// markers the same way the walk's USE display does.
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

/// Newest installed version in a slot by real `vercmp` ordering.
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

/// Shared `Plan -> GraphResult` mapping: ordered `(cp, version)` selections
/// plus `(to_cpv -> [(from_cat, from_pkg)])` parent edges become entries in
/// plan order (the plan's install order *is* the merge order -- v1 cut).
/// USE display resolves on demand through `repo` (memoized).
fn graph_result_from_order(
    req: &ResolveRequest,
    repo: &LazyRepo,
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
) -> GraphResult {
    let installed = InstalledView::of(req);
    let mut entries = Vec::new();
    for (cp, version) in order {
        let Some(record) = repo.record(cp, version) else {
            // Not one of the offered facts (should not happen: the order
            // came from the plan over these same facts).
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
        // Same `all_flags` split `pretend.rs` uses: enabled-first full
        // form for `-pv`, changed-only form for plain `-p`.
        let use_expand_display = super::build_use_expand_display(
            &use_flags_display,
            &req.config,
            None,
            &empty,
            true,
            &empty,
        );
        let use_expand_display_p = super::build_use_expand_display(
            &use_flags_display,
            &req.config,
            None,
            &empty,
            false,
            &empty,
        );
        let cpv = format!("{cp}-{version}");
        let mut required_by: Vec<(String, String)> = parents.get(&cpv).cloned().unwrap_or_default();
        required_by.sort();
        required_by.dedup();
        entries.push(GraphEntry {
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
            deps: Vec::new(),
        });
    }
    GraphResult {
        entries,
        slot_conflicts: Vec::new(),
        changed_deps_report: Vec::new(),
        buildpkgonly_deps_unsatisfied: false,
        pprovided_atoms: Vec::new(),
        autounmask_keyword_changes: Vec::new(),
        autounmask_use_changes: Vec::new(),
        autounmask_license_changes: Vec::new(),
        autounmask_mask_changes: Vec::new(),
        abi_rebuilds: Vec::new(),
        circular_deps: Vec::new(),
    }
}

/// Named slot from a target atom's `:slot` part, if it names one
/// (`SlotDep`'s fields are private, so this reads its `Display`: `0`,
/// `0/1.2`, `0=`, `:=`, `:*`).
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

/// Facts over a [`LazyRepo`]: the bridge's own `PackageRepository`
/// answers from memoized per-CPN loads, so the provider only ever pays
/// for the closure (see `new_for_targets` below).
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
    // Reachable conversion, not whole-tree (`new` converts every CPN;
    // `new_for_targets` converts the seeds' closure -- the same
    // `with_bdeps = false` the default path uses, so behaviour is
    // unchanged, only the converted set shrinks). The provider takes a
    // clone; the mapping below keeps the original (memoized maps make
    // the copy O(closure), no recompute).
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
        // Same expansion the bridge's own `Solver` impl uses
        // (`to_portage_targets`): a named `:slot` pins one slotted node,
        // otherwise every slotted node for the CPN (an unslotted target
        // node carries no versions when every version is slotted).
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
    Ok(graph_result_from_order(req, &lazy, &order, &parents))
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
    // The pool construction is inherently whole-input, so the input is
    // the closure (not the tree): populate only closure CPNs, USE
    // resolved per version (memoized, so each runs once).
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
    // Per-version USE rides in each `PackageMetadata::use_flags`; the
    // provider-level config stays default (no globally-forced flags).
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
    let solution = solver.solve(problem).map_err(|e| format!("{e:?}"))?;
    // `iter()` is `slice::Iter` (items are `&SolvableId`), so `to_vec`
    // would collect references -- the explicit copy stands.
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
    Ok(graph_result_from_order(req, &lazy, &order, &parents))
}

#[cfg(test)]
mod tests {
    use super::super::{SolverKind, active_resolver_for};

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
        // Selection itself is infallible and total: every kind yields a
        // resolver. Behaviour is pinned by fixture-driven tests elsewhere;
        // here only the wiring (no panic per kind).
        let _portage = active_resolver_for(SolverKind::Portage);
        let _pubgrub = active_resolver_for(SolverKind::PubGrub);
        let _resolvo = active_resolver_for(SolverKind::Resolvo);
    }

    fn fixture_request(atoms: &[&str]) -> super::super::ResolveRequest {
        use super::super::{Deep, ResolveRequest, SolverKind};
        use std::path::PathBuf;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("fixtures must exist");
        // The fixture profile's own ACCEPT_KEYWORDS (arch/amd64/make.defaults
        // `ACCEPT_KEYWORDS="${ARCH}"` -> amd64), so `is_visible` in the pool
        // sees the same keyword set a real resolve does.
        let mut config = portage_profile::Config::default();
        config.accept_keywords.insert("amd64".to_string());
        ResolveRequest {
            config_root: root.clone(),
            root,
            atoms: atoms.iter().map(|s| s.to_string()).collect(),
            config,
            newuse: false,
            changed_use: false,
            nodeps: false,
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
            complete: false,
            solver: SolverKind::Portage,
        }
    }

    /// The closure for a narrow target is a strict subset of the tree:
    /// `dev-libs/newpkg` pulls nothing else, so only its own CPN (plus
    /// installed CPNs, if any resolve) is loaded -- md5 reads, dep parses
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
        // ...but the diamond target pulls its whole (over-approximated)
        // dependency cone.
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

    /// The bridges resolve from a visibility-filtered pool: `dev-libs/
    /// maskedpkg` is `~amd64`-only (invisible under `ACCEPT_KEYWORDS=amd64`)
    /// so `load_versions` offers it nothing, while `dev-libs/newpkg`
    /// (`amd64`) keeps its one visible version.
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
