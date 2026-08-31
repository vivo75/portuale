//! The installed fallbacks and reinstall flips applied to a resolved outcome.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`apply_outcome_fallbacks`] reads from its caller.
pub(crate) struct ApplyOutcomeFallbacksArgs<'a> {
    pub(crate) outcome: PretendOutcome,
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) key: &'a (String, String),
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) state: &'a mut PassState,
    pub(crate) atom_root: &'a Path,
    pub(crate) item_targets_running_root: bool,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) depth: u32,
    pub(crate) buildtime_hard: bool,
    pub(crate) from_disjunction: bool,
}
