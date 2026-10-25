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
    pub(crate) buildtime_hard: bool,
}

/// One `resolve_pretend` call against the pass-constant constraints.
///
/// Shared by the initial outcome and the forced-upgrade flip, which pass each
/// sibling's own running flag while the single resolve uses the popped item's.
pub(crate) fn resolve_queue_atom(args: ResolveQueueAtomArgs<'_>) -> Result<PretendOutcome, Error> {
    let ResolveQueueAtomArgs {
        ctx,
        config,
        bp,
        depth,
        union_constraints,
        key,
        atom_text,
        running,
        buildtime_hard,
    } = args;
    let empty_constraints: Vec<String> = Vec::new();
    let extra_constraints = union_constraints.get(key).unwrap_or(&empty_constraints);
    let select_root: &Path = if running {
        ctx.cross_root()
            .expect("running-root atom without a cross root")
    } else {
        ctx.root
    };
    resolve_pretend(
        &SelectSource {
            root: select_root,
            config,
            local_binpkg: &ctx.local_binpkg,
            gate: ctx.gate(),
        },
        atom_text,
        &SelectOptions {
            is_top_level: depth == 0,
            depth,
            deep: ctx.deep,
            complete_mode: ctx.complete,
            arg_atoms: ctx.atoms,
            buildtime_hard,
            extra_constraints,
            ..ctx.select_options(bp)
        },
    )
}
