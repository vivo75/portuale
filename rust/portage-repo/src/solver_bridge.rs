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
//! - Notices, wired where the bridge result admits them: blockers are
//!   resolved through the walk's own `resolve_blockers` (H.15c), ABI
//!   rebuilds through its `slot_operator_rebuild_entries` fixpoint
//!   (H.15), circular deps through its `find_hard_cycles` over the
//!   entry `deps` edges (J), and USE display carries the real
//!   forced/masked `( )` markers (J). `slot_conflicts` and the
//!   `autounmask_*` change lists stay empty: an engine solution picks a
//!   single version per CPN, so a successful plan admits no same-slot
//!   divergence, and no relaxation loop ran whose flips could be
//!   listed -- a bridge failure is one `Error::Detail` line (engine-
//!   native text), not a real `depgraph.py` notice.
//! - Merge order is the walk's own `topological_merge_order` over real
//!   `deps` edges rebuilt from each version's raw dep strings plus
//!   resolved USE (H.15b) -- engine install order only seeds positions.
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
    /// The raw md5-cache strings those trees parsed from, same key
    /// order -- kept so merge-order edge building can reuse the exact
    /// walk-path helper (`merge_order::dep_edges_from_metadata`, which
    /// needs the unreduced strings plus resolved USE).
    raw_deps: [String; 5],
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
/// plus `(to_cpv -> [(from_cat, from_pkg)])` parent edges become entries;
/// each entry's `deps` come from its raw dep strings plus resolved USE
/// (H.15b) and the whole list is re-sorted through the walk's own
/// `topological_merge_order` below. USE display resolves on demand
/// through `repo` (memoized).
fn graph_result_from_order(
    req: &ResolveRequest,
    repos: &[RepoConfig],
    repo: &LazyRepo,
    order: &[(String, String)],
    parents: &HashMap<String, Vec<(String, String)>>,
) -> GraphResult {
    let installed = InstalledView::of(req);
    let mut entries = Vec::new();
    // Blocker atoms met in plan entries' dep strings, resolved after the
    // loop by the shared walk-path helper (see below).
    let mut pending_blockers: Vec<super::PendingBlocker> = Vec::new();
    let mut seen_blockers: HashSet<(String, String, String)> = HashSet::new();
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
        // Forced/masked-flag `( )` markers (J): the walk wraps every
        // profile-forced or masked IUSE flag via `forced_or_masked_flags`
        // (`refresh_entry_use_display`); the bridge passed an empty set,
        // so real `pkg_use_display`'s `( )` wraps never rendered. Same
        // call, same `cat/pkg-ver:slot/sub::repo` candidate spelling.
        let candidate_str = format!(
            "{cp}-{version}:{}/{sub}::{repo}",
            record.slot,
            sub = record.sub_slot,
            repo = record.repo_name,
        );
        let forced = super::forced_or_masked_flags(
            &record.iuse,
            &record.keywords,
            &candidate_str,
            &record.category,
            &record.package,
            &req.config,
        );
        // Same `all_flags` split `pretend.rs` uses: enabled-first full
        // form for `-pv`, changed-only form for plain `-p`.
        let use_expand_display = super::build_use_expand_display(
            &use_flags_display,
            &req.config,
            None,
            &forced,
            true,
            &empty,
        );
        let use_expand_display_p = super::build_use_expand_display(
            &use_flags_display,
            &req.config,
            None,
            &forced,
            false,
            &empty,
        );
        let cpv = format!("{cp}-{version}");
        let mut required_by: Vec<(String, String)> = parents.get(&cpv).cloned().unwrap_or_default();
        required_by.sort();
        required_by.dedup();
        // Merge-order fidelity (H.15b): the walk path builds every
        // entry's `deps` (real `_add_pkg_dep_string` order/priorities)
        // and sorts the whole list through `topological_merge_order`
        // (real `_serialize_tasks`); bridge entries used to carry no
        // edges at all, so engine install order leaked straight into
        // the display. Reuse the exact walk-path helper over the raw
        // dep strings (`raw_deps`: DEPEND, RDEPEND, BDEPEND, PDEPEND,
        // IDEPEND) plus this version's resolved USE -- bridge plans are
        // ebuild (source) candidates, hence the ebuild key list and
        // `built = false`, exactly the walk's own call for one.
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
        // Blocker collection (H.15c): the walk queues every USE-evaluated
        // dep token and diverts blocker atoms into `pending_blockers`
        // instead of the graph. Mirror it over the same raw strings and
        // resolved USE: flatten each key with USE applied, and record
        // every blocker atom against this owner (deduped per owner+atom;
        // one owner can name the same block in several keys).
        for raw in record.raw_deps.iter() {
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
                        target_category: dep_atom.category,
                        target_package: dep_atom.package,
                        owner_key: (record.category.clone(), record.package.clone()),
                        owner_version: version.clone(),
                    });
                }
            }
        }
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
            deps,
        });
    }
    // Blocker reporting (H.15c): match every collected blocker atom
    // against the installed db plus this plan's own resolved entries via
    // the shared walk-path `resolve_blockers` (same USE-aware `[use]`
    // gating, same `match_from_list` semantics), and file each conflict
    // on its owner entry -- mirroring the walk, which resolves blockers
    // after the graph settles and before the merge-order sort.
    for (owner_key, conflict) in super::resolve_blockers(&req.root, &pending_blockers, &entries) {
        if let Some(entry) = entries
            .iter_mut()
            .find(|e| (e.category.clone(), e.package.clone()) == owner_key)
        {
            entry.blockers.push(conflict);
        }
    }
    // ABI rebuilds (H.15, last slice): an installed consumer whose
    // built `cat/pkg:S/SS=` dep no longer matches how this plan leaves
    // that slot is scheduled for a reinstall -- the walk's own
    // `slot_operator_rebuild_entries` fixpoint, called with the walk's
    // own gate (`rebuild_if_new_slot`, not
    // `ignore_built_slot_operator_deps`) and the walk's own
    // complete-mode reachability (empty outside complete mode
    // suppresses the scan entirely, exactly like a plain non-complete
    // walk). Entries are extended before the merge-order sort so
    // rebuilds land dependency-first like every other entry; the
    // `(provider, consumer)` pairs feed `_show_abi_rebuild_info`.
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
            super::slot_operator_rebuild_entries(&req.root, repos, &entries, &slot_op_reachable)
        };
    entries.extend(slot_op_rebuilds);
    // Same merge-order sort the walk path applies (real portage's
    // `mylist` is dependency-first): engine install order only seeds
    // array positions now; `serialize_merge_order` re-sorts over the
    // `deps` edges above plus the `required_by` fallback, with the same
    // top-level atoms, profile config, root, and `--implicit-system-deps`
    // bias the walk resolves under.
    let entries = super::topological_merge_order(
        entries,
        &req.atoms,
        &req.config,
        &req.root,
        req.implicit_system_deps,
    );
    // Circular-dep notice (J): the walk records every dependency edge's
    // hard/soft kind while draining its queue and reports the shortest
    // unbreakable build-time cycle via `find_hard_cycles`, which
    // `pretend.rs` renders as the fatal `* Error: circular
    // dependencies:` block (exit 1). Bridge entries carry the same
    // `deps` edges (`dep_edges_from_metadata`, H.15b), so rebuild the
    // same kind map here: a build-time edge no installed package
    // satisfies is hard (`best_installed_for_atom`, the walk's own
    // gate), anything else soft -- blockers never reach the map on
    // either side (both skip them before recording). `slot_conflicts`
    // and the `autounmask_*` lists stay empty on purpose: an engine
    // solution picks a single version per CPN, so a successful plan
    // admits no same-slot divergence to report, and no relaxation loop
    // ran whose flips could be listed (a candidate needing a flip
    // fails instead -- the remaining cut, same as the module doc).
    let mut edge_kinds: super::EdgeKindMap = HashMap::new();
    for e in &entries {
        let owner = (e.category.clone(), e.package.clone());
        for dep in &e.deps {
            let kinds = edge_kinds
                .entry(((dep.category.clone(), dep.package.clone()), owner.clone()))
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
    let circular_deps = super::find_hard_cycles(&entries, &edge_kinds);
    // Elementary-cycle enumeration for the `large_cycle_count` trailer
    // and cycle-only re-display, same as the walk path: only a reported
    // hard cycle pays for the report build.
    let (large_cycle_count, cycle_display) = if circular_deps.is_empty() {
        (false, Vec::new())
    } else {
        let (cycles, display) = super::merge_order::cycle_report(&entries, &req.atoms, &req.root);
        let display = display
            .into_iter()
            .filter_map(|i| super::merge_bound_cpv(&entries[i]))
            .collect();
        (cycles.len() > 3, display)
    };
    GraphResult {
        entries,
        // A solved engine plan admits no same-slot divergence and no
        // relaxation loop ran, so it cannot abort either (no
        // `_create_graph` 0-return, no `_serialize_tasks` give-up, no
        // backtrack loop to exhaust) — always `Complete`. See
        // `ResolveOutcome`'s doc comment; the abort path is a property
        // of the BFS walk + backtrack loop in `lib.rs`, not the bridge.
        outcome: super::ResolveOutcome::Complete,
        slot_conflicts: Vec::new(),
        changed_deps_report: Vec::new(),
        buildpkgonly_deps_unsatisfied: false,
        pprovided_atoms: Vec::new(),
        autounmask_keyword_changes: Vec::new(),
        autounmask_use_changes: Vec::new(),
        autounmask_license_changes: Vec::new(),
        autounmask_mask_changes: Vec::new(),
        abi_rebuilds,
        circular_deps,
        large_cycle_count,
        cycle_display,
        // The engine backends never leave an installed-consumer pin
        // behind (a solved plan admits no same-slot divergence and no
        // relaxation loop ran), so there is nothing to disclose here --
        // same standing empty as slot_conflicts above.
        masked_deps: Vec::new(),
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
    // Engine-native failure text (the H.15a half of the solver V1 depth
    // cuts): an unsolvable request renders resolvo's own conflict
    // explanation (`Conflict::display_user_friendly`: "The following
    // packages are incompatible" plus the conflict chains, named through
    // the portage provider's solvable display) instead of the raw
    // `UnsolvableOrCancelled` debug dump (`Unsolvable(Conflict {
    // clauses: ... })`), which names no package at all. This mirrors the
    // pubgrub backend, whose `format_solve_error` already renders its
    // `NoSolution` derivation tree. A cancelled solve (not reachable
    // through this synchronous call path) stays a plain message.
    let solution = match solver.solve(problem) {
        Ok(solution) => solution,
        Err(resolvo::UnsolvableOrCancelled::Unsolvable(conflict)) => {
            return Err(format!("{}", conflict.display_user_friendly(&solver)));
        }
        Err(resolvo::UnsolvableOrCancelled::Cancelled(_)) => {
            return Err("dependency resolution was cancelled".to_string());
        }
    };
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
            implicit_system_deps: true,
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

    /// The resolvo backend reports engine-native failure text: an
    /// unsatisfiable target renders resolvo's own conflict explanation
    /// (`Conflict::display_user_friendly`) naming the involved packages
    /// -- never the raw `UnsolvableOrCancelled` debug dump (which names
    /// no package at all: `Unsolvable(Conflict { clauses: ... })`).
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

    /// Blocker reporting (H.15c): blocker atoms met in a solved bridge
    /// plan's dep strings are matched by the shared walk-path
    /// `resolve_blockers` and filed on their owner entries -- a weak
    /// in-graph block (`weakblockerpkg` vs `blockerpartnerpkg`) and a
    /// strong block against an installed package (`blockerpkg` vs
    /// installed `samepkg-1.0`) alike, exactly the walk's own shapes.
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
                unsolvable: false,
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
                    unsolvable: true,
                }]
            );
        }
    }

    /// ABI rebuilds (H.15, last slice): a plan that moves a slot's
    /// sub-slot schedules installed `:=`-bound consumers for reinstall
    /// via the walk's own `slot_operator_rebuild_entries` fixpoint --
    /// same gate, same complete-mode reachability. A throwaway vdb holds
    /// `slotbindtarget-1.0` at `SLOT=2` plus a stale (`:2/2=`) and a
    /// fresh (`:2/9=`) consumer, both in the seed set; resolving the
    /// target (whose `-2.0` ebuild is `SLOT=2/9`) must reinstall only the
    /// stale consumer and record the `(provider, consumer)` pair.
    #[test]
    fn bridge_plans_schedule_stale_equals_consumer_reinstalls() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!(
            "portage-bridge-slotop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
        req.root = dir.clone();
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
        // The target upgrades across the sub-slot; only the stale
        // consumer reinstalls, flagged as a slot-operator rebuild.
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

    /// Merge-order fidelity (H.15b): bridge entries carry real
    /// `deps` edges (same helper, same key order/priorities as the
    /// walk path) instead of an empty vec, so the shared
    /// `serialize_merge_order` sort -- not raw engine install order --
    /// decides the display order. `dev-libs/diamond`'s own entry must
    /// name both `shared-a` and `shared-b` as dependencies.
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

    /// Forced/masked-flag `( )` markers (J): with `forceflag` forced
    /// and `maskflag` masked for the package, both bridge backends
    /// render the walk's own `(forceflag)` / `(-maskflag)` wraps --
    /// the bridge used to pass an empty forced set, so real
    /// `pkg_use_display`'s markers never rendered. Fixture
    /// `dev-libs/pkgusemaskforcepkg`, the same package the walk-path
    /// contract pins (`USE="(forceflag) (-maskflag) -specflag"`).
    #[test]
    fn bridge_use_display_wraps_forced_and_masked_flags() {
        for kind in [
            super::super::SolverKind::PubGrub,
            super::super::SolverKind::Resolvo,
        ] {
            let mut req = fixture_request(&["dev-libs/pkgusemaskforcepkg"]);
            req.solver = kind;
            // `forced_or_masked_flags` reads the per-level stack, not
            // the flat fields: one synthetic repo level forcing
            // `forceflag` and masking `maskflag` for the package.
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

    /// Circular-dep notice (J): the bridge rebuilds the walk's own
    /// hard/soft edge-kind map over its entry `deps` edges and reports
    /// the shortest unbreakable build-time cycle via `find_hard_cycles`
    /// -- `pretend.rs` renders the fatal block from it, exactly like
    /// the walk (`dev-libs/hardcyclea` <-> `dev-libs/hardcycleb`, both
    /// unbuilt DEPEND edges). PubGrub linearises the cyclic closure,
    /// so the notice lands; resolvo's `install_order` still cannot
    /// linearise any cycle (Tier-4 `--solver=resolvo` item,
    /// `scope-backlog.md` §J) and errors before the mapping runs.
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
}
