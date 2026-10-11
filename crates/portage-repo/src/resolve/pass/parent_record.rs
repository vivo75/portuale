//! Record this atom's per-instance parent-atom row.

use crate::resolve::pass::*;

/// The values [`record_parent_atom`] reads from its caller.
pub(crate) struct RecordParentAtomArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) key: &'a (String, String),
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) current_atom: &'a str,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) state: &'a mut PassState,
}

/// Record this atom's per-instance parent-atom row.
pub(crate) fn record_parent_atom(args: RecordParentAtomArgs<'_>) {
    let RecordParentAtomArgs {
        outcome,
        key,
        owner,
        current_atom,
        unevaluated_atom,
        state,
    } = args;
    if resolver_debug()
        && let Some((child_version, child_installed)) = match &outcome {
            PretendOutcome::AlreadyInstalled { version } => Some((version.clone(), true)),
            PretendOutcome::New { version } => Some((version.clone(), false)),
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some((to.clone(), false))
            }
            PretendOutcome::Reinstall { version, .. } => Some((version.clone(), false)),
            // A removal never resolves a dependency atom.
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
        }
    {
        let row = resolver_trace::ParentAtom {
            child_category: key.0.clone(),
            child_package: key.1.clone(),
            child_version,
            child_installed,
            parent: owner.clone(),
            atom: current_atom.to_string(),
            unevaluated: unevaluated_atom.clone(),
        };
        if !state.parent_atoms.contains(&row) {
            state.parent_atoms.push(row);
        }
    }
}
