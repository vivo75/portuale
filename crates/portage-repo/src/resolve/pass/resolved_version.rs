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
