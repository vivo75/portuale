//! Queueing one package's dependencies.

use crate::*;

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
