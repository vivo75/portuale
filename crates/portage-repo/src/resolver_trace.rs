//! `emerge --pretend --debug` resolver trace: the DEBUG-level per-package
//! narration (`tr!`, stdout) and the digraph/candidate dumps (`tr_err!`,
//! stderr). Emitting Portage's shapes makes the two implementations directly
//! diffable.
//!
//! Deliberate divergences: plain-text node labels (no ANSI colour);
//! portuale's post-prune merge closure as the node set, with top-level atoms
//! as pseudo-arg nodes; and `Child:`/`Parent Dep:` interleaving in portuale's
//! BFS queue order, not Portage's LIFO stack order (the `digraph:` dump
//! matches; the narration order does not).
//!
//! Design note and the full message inventory:
//! `docs/history/emerge-pretend-debug.md`.
// Portage: _emerge/depgraph.py, portage/util/__init__.py (writemsg_level)

use std::fmt;
use std::path::Path;

use crate::merge_order::{DepPriority, entry_version};
use crate::{
    CandidateSource, GraphEntry, PretendOutcome, RepoConfig, installed_candidates,
    installed_pkg_repo, list_candidates, read_vdb_slot,
};

/// Emit `args` to stderr only when the trace is active (the `digraph:` /
/// candidate-list dumps).
pub(crate) fn err(args: fmt::Arguments) {
    if crate::resolver_debug() {
        eprint!("{args}");
    }
}

/// The single highest priority classification, for `Priority:` lines and
/// `digraph:` edge labels.
// Portage: _emerge/DepPriority.py, DepPriority.__str__
pub(crate) fn dep_priority_str(p: &DepPriority) -> &'static str {
    // No `ignored` field modelled here; `optional` is the widest portuale
    // expresses.
    if p.optional {
        "optional"
    } else if p.buildtime_slot_op {
        "buildtime_slot_op"
    } else if p.buildtime {
        "buildtime"
    } else if p.runtime_slot_op {
        "runtime_slot_op"
    } else if p.runtime {
        "runtime"
    } else if p.runtime_post {
        "runtime_post"
    } else {
        "soft"
    }
}

/// The hardness measure a per-edge priority list is ordered by, so the label
/// is the max. `buildtime_slot_op` 0 down to `optional` -5, none -6.
// Portage: _emerge/DepPriority.py, DepPriority.__int__
pub(crate) fn dep_priority_rank(p: &DepPriority) -> i8 {
    if p.optional {
        -5
    } else if p.buildtime_slot_op {
        0
    } else if p.buildtime {
        -1
    } else if p.runtime_slot_op {
        -2
    } else if p.runtime {
        -3
    } else if p.runtime_post {
        -4
    } else {
        -6
    }
}

/// The `DepPriority` with the greatest [`dep_priority_rank`] in `prios`.
/// `prios` is never empty at a digraph edge.
pub(crate) fn max_priority(prios: &[DepPriority]) -> DepPriority {
    prios
        .iter()
        .copied()
        .max_by_key(dep_priority_rank)
        .unwrap_or_default()
}

/// `(cat/pkg-ver[-build_id]:slot/sub_slot::repo, <state>)` for a graph node:
/// `installed` for a nomerge node, `<type> scheduled for merge` otherwise
/// (minus Portage's ANSI colour).
// Portage: _emerge/Package.py, Package.__str__
pub(crate) fn node_label(e: &GraphEntry, root: &Path, installed: bool) -> String {
    let ver = entry_version(e).unwrap_or("");
    let (slot, sub_slot, repo) = if installed {
        let (s, ss) = read_vdb_slot(root, &e.category, &e.package, ver);
        (
            s,
            ss,
            installed_pkg_repo(root, &e.category, &e.package, ver),
        )
    } else {
        (
            e.slot.clone().unwrap_or_else(|| "0".to_string()),
            e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
            e.repo_name
                .clone()
                .unwrap_or_else(|| "__unknown__".to_string()),
        )
    };
    // Only a merge-bound multi-instance binary carries a build id here.
    let build_id_str = if installed {
        String::new()
    } else {
        e.build_id
            .as_deref()
            .map(|b| format!("-{b}"))
            .unwrap_or_default()
    };
    let state = if installed {
        "installed".to_string()
    } else {
        let ty = match e.source {
            CandidateSource::Binary => "binary",
            CandidateSource::Ebuild => "ebuild",
        };
        format!("{ty} scheduled for merge")
    };
    format!(
        "({}/{}-{ver}{build_id_str}:{slot}/{sub_slot}::{repo}, {state})",
        e.category, e.package
    )
}

/// Whether a `GraphEntry` is a nomerge node (installed, no-visible-candidate,
/// or removal): never selected as a merge.
// Portage: depgraph.py, pkg.operation == "nomerge"
pub(crate) fn is_nomerge(e: &GraphEntry) -> bool {
    matches!(
        e.outcome,
        PretendOutcome::AlreadyInstalled { .. }
            | PretendOutcome::NoVisibleCandidate
            | PretendOutcome::Uninstall { .. }
    )
}

/// `USE="flag -flag …"`, bare-name-sorted the way Portage opens its string.
fn use_str(e: &GraphEntry) -> String {
    e.use_flags_display
        .iter()
        .map(|(f, on)| if *on { f.clone() } else { format!("-{f}") })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stage 3: the per-atom candidate list, in ebuild-then-installed order, to
/// stderr. Binary candidates are not listed (no `BinaryIndex` at this call
/// site); for a fixture with no binaries the list is identical to Portage's.
// Portage: _emerge/depgraph.py, _wrapped_select_pkg_highest_available_imp
pub(crate) fn dump_atom_candidates(
    repos: &[RepoConfig],
    root: &Path,
    atom: &str,
    category: &str,
    package: &str,
) {
    if !crate::resolver_debug() {
        return;
    }
    let line = |ty: &str, cpv: &str, repo: &str| {
        err(format_args!("{:>10} {cpv}::{repo}\n", format!("{ty}:")));
    };
    // ebuild candidates -- filter by the atom.
    if let Ok(cands) = list_candidates(repos, category, package) {
        let strs: Vec<String> = cands
            .iter()
            .map(|c| {
                format!(
                    "{category}/{package}-{}:{}/{}::{}",
                    c.version, c.slot, c.sub_slot, c.repo_name
                )
            })
            .collect();
        let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
        let matched = portage_dep::match_from_list(atom, &refs).unwrap_or_default();
        for c in cands.iter() {
            let s = format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            );
            if matched.iter().any(|m| *m == s) {
                line(
                    "ebuild",
                    &format!("{category}/{package}-{}", c.version),
                    &c.repo_name,
                );
            }
        }
    }
    // installed candidates -- filter by the atom.
    let inst = installed_candidates(root, category, package);
    let strs: Vec<String> = inst
        .iter()
        .map(|(v, s, ss)| format!("{category}/{package}-{v}:{s}/{ss}"))
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom, &refs).unwrap_or_default();
    for (v, s, ss) in &inst {
        let key = format!("{category}/{package}-{v}:{s}/{ss}");
        if matched.iter().any(|m| *m == key) {
            line(
                "installed",
                &format!("{category}/{package}-{v}"),
                &installed_pkg_repo(root, category, package, v),
            );
        }
    }
}

/// One `Parent Dep:` narration row, collected during the walk where the child
/// instance an atom actually resolved to is known.
// Portage: _emerge/depgraph.py, _add_parent_atom
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParentAtom {
    pub child_category: String,
    pub child_package: String,
    pub child_version: String,
    pub child_installed: bool,
    /// `None` for a top-level argument / internal set seed.
    pub parent: Option<(String, String)>,
    pub atom: String,
    /// Present only when evaluating the parent's conditional use-deps rewrote
    /// the atom.
    pub unevaluated: Option<String>,
}

/// `USE="…"` for an installed-only child, read from the vdb.
fn installed_use_str(root: &Path, cat: &str, pkg: &str, ver: &str) -> String {
    let iuse = crate::read_vdb_flag_set(root, cat, pkg, ver, "IUSE");
    let enabled = crate::read_vdb_flag_set(root, cat, pkg, ver, "USE");
    let mut names: Vec<&String> = iuse.iter().collect();
    names.sort();
    names
        .iter()
        .map(|f| {
            if enabled.contains(*f) {
                (*f).clone()
            } else {
                format!("-{f}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `Child:` row's `USE="…"` column: an installed keep renders its
/// vdb-recorded USE, every other row the entry's own display flags.
pub(crate) fn child_use_str(e: &GraphEntry, root: &Path) -> String {
    if is_nomerge(e) {
        let ver = entry_version(e).unwrap_or("");
        installed_use_str(root, &e.category, &e.package, ver)
    } else {
        use_str(e)
    }
}

/// Emission order for the bounded inner-ring re-narration (see
/// [`dump_resolution_walk`]): for every recorded `(parent, child, atom)` edge
/// whose child stanza was already emitted (a back edge, i.e. a revisit or cycle
/// step), the compact two-line re-narration Portage prints there.
/// Returns `(parent_entry_idx, parent_atom_idx)` pairs in emission order.
/// Pure and total: re-visits never re-expand, so no visited set is needed;
/// `budget` caps the volume instead.
pub(crate) fn renarration_order(
    entry_cps: &[(String, String)],
    entry_keys: &[(String, String, String, bool)],
    parent_atoms: &[ParentAtom],
    budget: usize,
) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut remaining = budget;
    if remaining == 0 {
        return out;
    }
    for (pi, pa) in parent_atoms.iter().enumerate() {
        let Some((pc, pp)) = pa.parent.as_ref() else {
            continue;
        };
        // The walk records the parent without a version, so a cp with two
        // entries hosts under the first.
        let Some(i) = entry_cps.iter().position(|(c, p)| c == pc && p == pp) else {
            continue;
        };
        // Only back edges re-narrate: forward edges are covered by the
        // child's own later stanza, so DAG walks schedule nothing.
        let child_key = (
            pa.child_category.clone(),
            pa.child_package.clone(),
            pa.child_version.clone(),
            pa.child_installed,
        );
        if !entry_keys[..=i].contains(&child_key) {
            continue;
        }
        out.push((i, pi));
        remaining -= 1;
        if remaining == 0 {
            break;
        }
    }
    // Emission order: hosts in entries order, each host's edges in walk
    // order. The caller re-sorts each host's edges into dependency-text
    // order.
    out.sort_by_key(|(i, _)| *i);
    out
}

/// Re-sorts one host's scheduled re-narration edges into the host's
/// dependency-text order. Edges matching no dep keep walk order at the end
/// (stable). Pure.
pub(crate) fn sort_by_dep_position(
    deps: &[(String, String)],
    order: Vec<usize>,
    atom_of: impl Fn(usize) -> String,
) -> Vec<usize> {
    let mut indexed: Vec<(usize, usize)> = order
        .into_iter()
        .map(|pi| {
            let atom = atom_of(pi);
            let pos = deps
                .iter()
                .position(|(raw, evaluated)| *raw == atom || *evaluated == atom)
                .unwrap_or(usize::MAX);
            (pos, pi)
        })
        .collect();
    indexed.sort_by_key(|(pos, _)| *pos);
    indexed.into_iter().map(|(_, pi)| pi).collect()
}
