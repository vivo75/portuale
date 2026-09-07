//! A top-level atom with no visible candidate: Portage's drop-or-abort for arguments.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`step_top_level_unsatisfied`] reads from its caller.
pub(crate) struct StepTopLevelUnsatisfiedArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) current_atom: &'a str,
    pub(crate) outcome: &'a mut PretendOutcome,
    pub(crate) state: &'a mut PassState,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) config: &'a Config,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) atom: &'a Atom,
}
