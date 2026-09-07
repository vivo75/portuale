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
//! and hands it in; the engines only pick versions. Concretely each
//! backend populates the bridge's repository shape from
//! [`super::list_candidates`] + [`super::read_md5_cache`] (dep strings
//! parsed with `portage_atom::DepEntry::parse`, the same crate version the
//! bridges themselves use), resolves, and translates the plan back into
//! `GraphEntry`s in the plan's install order.
//!
//! Deliberate v1 cuts (each would be its own slice):
//! - No visibility filtering: every md5-cache version is offered
//!   (keyword/license/mask/`package.*` acceptance is the backtracking
//!   walk's own `is_visible`; the bridges get the unfiltered pool).
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

use std::collections::{HashMap, HashSet};

use super::{
    GraphEntry, GraphResult, InstalledRef, PretendOutcome, ResolveRequest,
    Resolver as PortualeResolver, all_cp, all_installed_packages, effective_use_flags, find_repos,
    installed_pkg_iuse_and_use, list_candidates, read_md5_cache,
};

// --- Shared fact collection ------------------------------------------------

/// One repo version with everything a bridge needs: parsed dep trees per
/// class, resolved USE, and identity back into `GraphEntry` fields.
struct VersionRecord {
    category: String,
    package: String,
    version: String,
    slot: String,
    sub_slot: String,
    repo_name: String,
    iuse: String,
    use_set: HashSet<String>,
    /// DEPEND, RDEPEND, BDEPEND, PDEPEND, IDEPEND in that order.
    deps: [Vec<portage_solver::DepEntry>; 5],
}

impl VersionRecord {
    /// `category/package` and `category/package-version` spellings.
    fn cp(&self) -> String {
        format!("{}/{}", self.category, self.package)
    }
    fn cpv(&self) -> String {
        format!("{}/{}-{}", self.category, self.package, self.version)
    }
}

/// Read every md5-cache version of every `cat/pkg` into records. Versions
/// whose `Cpv` `portage_atom` cannot parse are skipped (they can never be
/// offered to a bridge); dep classes that fail `DepEntry::parse` feed
/// empty (see the module doc comment).
fn collect_records(req: &ResolveRequest) -> Result<Vec<VersionRecord>, super::Error> {
    let repos = find_repos(&req.config_root)?;
    let mut records = Vec::new();
    for cp in all_cp(&repos) {
        let Some((category, package)) = cp.split_once('/') else {
            continue;
        };
        let candidates = list_candidates(&repos, category, package)?;
        for candidate in candidates {
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
            let candidate_str = format!("{cp}-{}::{}", candidate.version, candidate.repo_name);
            let use_set = effective_use_flags(
                &req.config,
                metadata.get("IUSE").map(String::as_str).unwrap_or(""),
                &candidate.keywords,
                &candidate_str,
                category,
                package,
            );
            records.push(VersionRecord {
                category: category.to_string(),
                package: package.to_string(),
                version: candidate.version.clone(),
                slot: candidate.slot.clone(),
                sub_slot: candidate.sub_slot.clone(),
                repo_name: candidate.repo_name.clone(),
                iuse: metadata.get("IUSE").cloned().unwrap_or_default(),
                use_set,
                deps,
            });
        }
    }
    Ok(records)
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
fn graph_result_from_order(
    req: &ResolveRequest,
    records: &[VersionRecord],
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
) -> GraphResult {
    let by_cpv: HashMap<String, &VersionRecord> = records.iter().map(|r| (r.cpv(), r)).collect();
    let installed = InstalledView::of(req);
    let mut entries = Vec::new();
    for (cp, version) in order {
        let Some(record) = by_cpv.get(&format!("{cp}-{version}")) else {
            // Not one of the offered facts (should not happen: the order
            // came from the plan over these same facts).
            continue;
        };
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
                let on = record.use_set.contains(&flag);
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

/// Precomputed facts implementing the bridge's own `PackageRepository`
/// trait: every offered version plus its resolved-USE config. Precomputing
/// (rather than reading md5-cache per query) keeps `versions_for` cheap --
/// the provider calls it fresh for every package on every rebuild.
struct BridgeRepo {
    versions: HashMap<String, Vec<(portage_solver::Cpv, portage_atom_pubgrub::PackageVersions)>>,
    uses: HashMap<String, portage_atom_pubgrub::UseConfig>,
}

impl portage_atom_pubgrub::PackageRepository for BridgeRepo {
    fn all_packages(&self) -> Vec<portage_solver::Cpn> {
        let mut out: Vec<portage_solver::Cpn> = self
            .versions
            .keys()
            .filter_map(|cp| portage_solver::Cpn::parse(cp).ok())
            .collect();
        out.sort_by(|a, b| format!("{a}").cmp(&format!("{b}")));
        out
    }

    fn versions_for(
        &self,
        cpn: &portage_solver::Cpn,
    ) -> Vec<(portage_solver::Cpv, portage_atom_pubgrub::PackageVersions)> {
        self.versions
            .get(&format!("{cpn}"))
            .cloned()
            .unwrap_or_default()
    }

    fn desired_use(&self, cpv: &portage_solver::Cpv) -> portage_atom_pubgrub::UseConfig {
        self.uses
            .get(&format!("{cpv}"))
            .cloned()
            .unwrap_or_default()
    }
}

fn bridge_repo(records: &[VersionRecord]) -> Result<BridgeRepo, String> {
    use portage_atom_pubgrub::{IUseDefault, PackageDeps, PackageVersions, UseConfig};
    let mut versions: HashMap<String, Vec<(portage_solver::Cpv, PackageVersions)>> = HashMap::new();
    let mut uses: HashMap<String, UseConfig> = HashMap::new();
    for record in records {
        let cpv = portage_solver::Cpv::parse(&record.cpv())
            .map_err(|e| format!("unparsable cpv {}: {e:?}", record.cpv()))?;
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
        let deps = PackageDeps::new(
            record.deps[0].clone(),
            record.deps[1].clone(),
            record.deps[2].clone(),
            record.deps[3].clone(),
            record.deps[4].clone(),
        );
        versions.entry(record.cp()).or_default().push((
            cpv.clone(),
            PackageVersions {
                slot,
                subslot,
                repo: Some(interned(&record.repo_name)),
                iuse,
                iuse_defaults,
                deps,
                required_use: None,
            },
        ));
        let mut use_config = UseConfig::new();
        for flag in &record.use_set {
            use_config.enable(interned(flag));
        }
        uses.insert(format!("{cpv}"), use_config);
    }
    Ok(BridgeRepo { versions, uses })
}

fn resolve_pubgrub(req: &ResolveRequest) -> Result<GraphResult, String> {
    use portage_atom_pubgrub::{
        InstalledPackage as BridgeInstalled, InstalledPolicy, PortageDependencyProvider,
        PortagePackage, PortageVersionSet,
    };
    let records = collect_records(req).map_err(|e| format!("reading repo facts: {e}"))?;
    let repo = bridge_repo(&records)?;
    let mut provider = PortageDependencyProvider::new(repo);
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
    Ok(graph_result_from_order(req, &records, &order, &parents))
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
    let records = collect_records(req).map_err(|e| format!("reading repo facts: {e}"))?;
    let mut repo = InMemoryRepository::new();
    for record in &records {
        let cpv = portage_solver::Cpv::parse(&record.cpv())
            .map_err(|e| format!("unparsable cpv {}: {e:?}", record.cpv()))?;
        let iuse: Vec<_> = parse_iuse(&record.iuse)
            .into_iter()
            .map(|flag| interned(&flag))
            .collect();
        let use_flags: HashSet<_> = record.use_set.iter().map(|f| interned(f)).collect();
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
    // `iter().copied().collect()` (not `to_vec()`: `iter()` is
    // `slice::Iter`, whose items are `&SolvableId`, so `to_vec` would
    // collect references).
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
    Ok(graph_result_from_order(req, &records, &order, &parents))
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
}
