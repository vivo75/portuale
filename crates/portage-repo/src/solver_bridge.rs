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
