//! The initial outcome for one queued atom: complete-mode pin, sibling collapse, resolve.

use crate::resolve::pass::*;
use portage_profile::Config;

/// The values [`resolve_initial_outcome`] reads from its caller.
pub(crate) struct ResolveInitialOutcomeArgs<'a> {
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) depth: u32,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) from_disjunction: bool,
    pub(crate) state: &'a mut PassState,
    pub(crate) current_atom: &'a str,
    pub(crate) buildtime_hard: bool,
    pub(crate) item_targets_running_root: bool,
    pub(crate) atom_root: &'a Path,
}
