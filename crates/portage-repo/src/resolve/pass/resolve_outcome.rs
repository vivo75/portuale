//! One `resolve_pretend` call against the pass-constant constraints.

use crate::resolve::pass::*;
use portage_profile::Config;

/// The values [`resolve_queue_atom`] reads from its caller.
pub(crate) struct ResolveQueueAtomArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) depth: u32,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) atom_text: &'a str,
    pub(crate) running: bool,
}
