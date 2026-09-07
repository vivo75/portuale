//! Read a graphed entry's metadata and enqueue its dependencies.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`enqueue_graphed_entry`] reads from its caller.
pub(crate) struct EnqueueGraphedEntryArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) state: &'a mut PassState,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) depth: u32,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) repo_candidates: &'a [Candidate],
    pub(crate) graphed: GraphResolvedEntryOut<'a>,
}
