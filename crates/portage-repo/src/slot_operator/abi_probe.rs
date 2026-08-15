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
