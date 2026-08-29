//! Queueing one package's dependencies.

use crate::*;

/// The installed or newly selected package whose dependencies
/// [`enqueue_dependencies`] queues.
#[derive(Clone, Copy)]
pub(crate) struct DepOwner<'a> {
    /// The walk's target root, so an installed parent's own deps resolve per
    /// group like every other owner. `root` stays the vdb root for metadata.
    pub target_root: &'a Path,
    pub category: &'a str,
    pub package: &'a str,
    pub version: &'a str,
    pub child_depth: u32,
}

/// The emerge options [`enqueue_dependencies`] consults.
#[derive(Clone, Copy)]
pub(crate) struct EnqueueOptions<'a> {
    pub dynamic_deps: bool,
    pub ignore_built_slot_operator_deps: bool,
    pub with_bdeps: bool,
    pub root_deps_running_root: Option<&'a Path>,
    /// No `circular_dependency` map: this deep-walk's parents are
    /// `AlreadyInstalled` entries only, which Portage's node-identity-keyed
    /// lookup always misses, so the empty map is exactly faithful here.
    /// `update` (`--update`): the downgrade-guard's queued-update lookahead.
    pub update: bool,
}

/// The pass state [`enqueue_dependencies`] appends to.
pub(crate) struct EnqueueSinks<'a> {
    pub queue: &'a mut VecDeque<QueueItem>,
    pub pending_blockers: &'a mut Vec<PendingBlocker>,
    pub entries: &'a mut Vec<GraphEntry>,
    pub root_deps_build_seen: &'a mut HashSet<(String, String)>,
    /// Per-pass `installed_dep_string` memo: keyed
    /// `(category, package, version, key)`, Effective results only.
    pub installed_meta_memo: &'a mut HashMap<(String, String, String, String), String>,
    /// This installed package's own flattened non-blocker atoms become
    /// slot-conflict pullers, like the main loop's own recording.
    pub slot_pullers: &'a mut SlotPullers,
}
