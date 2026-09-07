//! Process one queued atom through resolve, graph and enqueue.

use crate::resolve::pass::*;
use portage_profile::Config;

/// The values [`step_queue_item`] reads from its caller.
pub(crate) struct StepQueueItemArgs<'a> {
    pub(crate) current_atom: String,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) depth: u32,
    pub(crate) item_owner_targets_running_root: bool,
    pub(crate) item_targets_running_root: bool,
    pub(crate) buildtime_hard: bool,
    pub(crate) from_disjunction: bool,
    pub(crate) first_pass: bool,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) pprovided_refs: &'a [&'a str],
    pub(crate) state: &'a mut PassState,
}
