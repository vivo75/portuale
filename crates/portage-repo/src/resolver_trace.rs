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
