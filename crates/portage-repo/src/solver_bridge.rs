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
