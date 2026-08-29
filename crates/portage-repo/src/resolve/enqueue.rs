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
