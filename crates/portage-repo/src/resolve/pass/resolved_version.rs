//! The resolved version for a merge-bound outcome, or the unresolved record.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// What [`select_resolved_version`] hands back to its caller.
pub(crate) struct SelectResolvedVersionOut {
    pub(crate) version: String,
    pub(crate) outcome: PretendOutcome,
    pub(crate) key: (String, String),
}

/// The values [`select_resolved_version`] reads from its caller.
pub(crate) struct SelectResolvedVersionArgs<'a> {
    pub(crate) current_atom: &'a str,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) key: (String, String),
    pub(crate) outcome: PretendOutcome,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) state: &'a mut PassState,
    pub(crate) config: &'a Config,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) atom_root: &'a Path,
    pub(crate) item_targets_running_root: bool,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) depth: u32,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) atom: &'a Atom,
}

/// The resolved version for a merge-bound outcome, or the unresolved record.
///
/// A merge-bound outcome names its version; anything else records the
/// unresolved dependency and drops the node.
pub(crate) fn select_resolved_version(
    args: SelectResolvedVersionArgs<'_>,
) -> ControlFlow<(), SelectResolvedVersionOut> {
    let SelectResolvedVersionArgs {
        current_atom,
        bp,
        key,
        outcome,
        ctx,
        state,
        config,
        owner,
        atom_root,
        item_targets_running_root,
        unevaluated_atom,
        depth,
        union_constraints,
        atom,
    } = args;
    let resolved_version = match &outcome {
        PretendOutcome::New { version } => Some(version.clone()),
        PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
        PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
        PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
        _ => None,
    };

    let Some(version) = resolved_version else {
        record_unresolved_dep(RecordUnresolvedDepArgs {
            current_atom,
            bp,
            key,
            outcome,
            ctx,
            state,
            config,
            owner,
            atom_root,
            item_targets_running_root,
            unevaluated_atom,
            depth,
            union_constraints,
            atom,
        });
        return ControlFlow::Break(());
    };
    ControlFlow::Continue(SelectResolvedVersionOut {
        version,
        outcome,
        key,
    })
}
