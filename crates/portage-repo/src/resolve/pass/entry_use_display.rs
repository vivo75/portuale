//! Record the entry's USE display: changed flags, IUSE and USE_EXPAND groups.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::sync::Arc;

/// The values [`record_entry_use_display`] reads from its caller.
pub(crate) struct RecordEntryUseDisplayArgs<'a> {
    pub(crate) already_installed_outcome: bool,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) config: &'a Config,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) resolved: &'a Candidate,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) state: &'a mut PassState,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) current_atom: &'a str,
    pub(crate) entry_idx: usize,
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
}
