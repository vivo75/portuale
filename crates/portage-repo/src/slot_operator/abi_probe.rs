//! The slot-conflict ABI probe.

use crate::*;

/// One installed parent's atom on a provider, for the update-probe refusal:
/// the parent's identity plus the effective atom text to match the fresh
/// candidate against.
#[derive(Debug, Clone)]
pub(crate) struct ProbeParent {
    /// The installed parent `(category, package)`.
    pub(crate) parent: (String, String),
    /// Effective atom: [`relax_built_slot_operator_atom`] for built
    /// slot-operator atoms, verbatim otherwise.
    pub(crate) atom: String,
}

/// The same-slot vdb scan (Portage's `_slot_operator_trigger_reinstalls`):
/// an installed consumer whose built `:S/SS=` atom no longer matches how this
/// run leaves that slot is rebuilt. Returns the consumer set plus display
/// pairs and builds no `GraphEntry`: the consumer becomes a walked node on the
/// next pass. Only a provider entry that replaces an installed version in a
/// slot counts, and only for a consumer bound to that slot at a different
/// sub-slot. The parameter gates (`reachable`, `already`, `undone`,
/// `with_bdeps`) are recorded in the notes.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls
/// Highest visible `(slot, version)` for a cp: the slot a `--update` request
/// keeps unpinned while every lower installed slot gains a `cat/pkg:S` pin.
pub(crate) fn highest_visible_slot(
    repos: &[RepoConfig],
    cp: &(String, String),
) -> Option<(String, String)> {
    list_candidates(repos, &cp.0, &cp.1)
        .ok()?
        .iter()
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
        .map(|c| (c.slot.clone(), c.version.clone()))
}
