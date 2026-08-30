//! Re-expanding a resolved slot when USE changes.

use crate::*;

/// Arguments of [`delta_atom_cleanly_reuses_resolved_slot`].
pub(crate) struct DeltaAtomCleanlyReusesResolvedSlotArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) state: &'a PassState,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) evaluated: &'a str,
    /// This delta atom's own resolving root (its queue tag): selection and slot
    /// lookup both run against it, so a running-root atom never reuses a
    /// target-root slot (and vice versa).
    pub(crate) atom_running: bool,
}
