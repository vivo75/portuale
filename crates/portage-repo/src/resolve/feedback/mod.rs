//! Turning a pass's problems into feedback for the next pass.

use crate::*;
use std::ops::ControlFlow;
mod solvable_slot_conflicts;
pub(crate) use solvable_slot_conflicts::*;

/// Why one `cat/pkg` version is masked on retry. Readers see the union across
/// reasons; the backtracker consumes them. Portage: backtracking.py, `runtime_pkg_mask`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskReason {
    /// The masked version collided in its slot. `parents` are the pulling
    /// `((cat, pkg, ver), atom)` triples whose atom does NOT match it.
    SlotConflict {
        parents: Vec<((String, String, String), String)>,
    },
    /// The masked parent's subtree contained an unsatisfiable dep; `atom` is
    /// that dep's text. Portage also records the root; portuale resolves one root.
    MissingDependency {
        parent: (String, String),
        atom: String,
    },
    /// A non-installed binary whose built slot-operator dep cannot be satisfied the
    /// way it was built. The rendered `neg` carries a `[binary]` marker, so only the
    /// binary-candidate pool filter honours it; every string-constraint reader skips
    /// marked entries. Only the slot-change arm is ported; the update and
    /// unsatisfied probe arms steer a parent the graph already replaces or fails on.
    SlotOperatorBuilt,
}

/// One `!=cat/pkg-ver` negative with its reason. `neg` renders exactly as the string
/// used to live in `slot_constraints`, so every reader matching on it behaves
/// identically. The masked version is recoverable without parsing: strip the
/// `!=cat/pkg-` prefix formed from the bucket key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaskEntry {
    pub(crate) neg: String,
    pub(crate) reason: MaskReason,
}

/// The settle tail of `backtracking_resolve`. USE changes start their chain at the
/// forcing parent, not the first requirer: the flip site records its forcing owner in
/// `AutounmaskChange::trigger`; the fill starts there when it is still a requirer,
/// else the legacy first-requirer start stands. The ascent above that start is
/// unchanged (single-branch, first-requirer hops). Keyword/license/mask changes keep
/// the legacy start (see `fill_non_use_change_chains`).
pub(crate) fn fill_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| {
                    change
                        .trigger
                        .clone()
                        .filter(|t| e.required_by.contains(t))
                        .or_else(|| e.required_by.first().cloned())
                });
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}

/// Keyword/license/mask twin of `fill_use_change_chains`. Re-derive from the change's
/// own atom: its left-hand `=<cpv>` / `>=<cpv>` names the package the change is on.
/// Same no-start-node rule: a change on an argument package passes `None`, yielding
/// just the `(argument)` line.
pub(crate) fn fill_non_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| e.required_by.first().cloned());
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}
