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

/// Read a graphed entry's metadata and enqueue its dependencies.
///
/// Binary metadata comes from any local or remote instance; ebuild metadata
/// from the repo. `--nodeps` pops "recurse" before anything enqueues.
pub(crate) fn enqueue_graphed_entry(
    args: EnqueueGraphedEntryArgs<'_>,
) -> Result<ControlFlow<()>, Error> {
    let EnqueueGraphedEntryArgs {
        ctx,
        config,
        bp,
        state,
        key,
        version,
        current_atom,
        atom,
        unevaluated_atom,
        owner,
        depth,
        union_constraints,
        repo_candidates,
        graphed,
    } = args;
    let GraphResolvedEntryOut {
        resolved,
        entry_idx,
        candidate_source,
        candidate_remote,
        candidate_build_id,
        already_installed_outcome,
        candidate_str,
        repo_location,
        keywords,
    } = graphed;
    let metadata = if candidate_source == CandidateSource::Binary {
        let Some(metadata) = read_binary_metadata_any_instance(ReadBinaryMetadataAnyInstanceArgs {
            config,
            root: ctx.root,
            local: &ctx.local_binpkg,
            category: &key.0,
            package: &key.1,
            version,
            build_id: candidate_build_id.as_deref(),
        }) else {
            return Ok(ControlFlow::Break(()));
        };
        std::sync::Arc::new(metadata)
    } else {
        let pf = format!("{}-{version}", key.1);
        let Ok(metadata) = repo_aux_metadata(&repo_location, &key.0, &pf) else {
            return Ok(ControlFlow::Break(()));
        };
        metadata
    };
    let use_flags = entry_use_flags(EntryUseFlagsArgs {
        candidate_source,
        resolved,
        config,
        metadata: &metadata,
        keywords: &keywords,
        candidate_str: &candidate_str,
        key,
        bp,
        atom,
        state: &mut *state,
        repo_candidates,
        owner,
    });

    record_entry_fetch(RecordEntryFetchArgs {
        candidate_source,
        resolved,
        key,
        candidate_str: &candidate_str,
        config,
        state: &mut *state,
        entry_idx,
        metadata: &metadata,
        use_flags: &use_flags,
        repo_location: &repo_location,
        ctx,
        candidate_remote,
        version,
    });

    if record_entry_use_display(RecordEntryUseDisplayArgs {
        already_installed_outcome,
        metadata: &metadata,
        config,
        use_flags: &use_flags,
        ctx,
        key,
        version,
        resolved,
        owner,
        state: &mut *state,
        unevaluated_atom,
        current_atom,
        entry_idx,
        keywords: &keywords,
        candidate_str: &candidate_str,
    })?
    .is_break()
    {
        return Ok(ControlFlow::Break(()));
    }

    // `--nodeps` pops "recurse": no dep string is ever read, so no blockers
    // are ever collected either. Portage: create_depgraph_params.py; depgraph.py
    if ctx.nodeps {
        return Ok(ControlFlow::Break(()));
    }

    enqueue_entry_deps(EnqueueEntryDepsArgs {
        ctx,
        depth,
        metadata: &metadata,
        candidate_source,
        state: &mut *state,
        entry_idx,
        use_flags: &use_flags,
        bp,
        config,
        union_constraints,
        key,
        version,
        candidate_str: &candidate_str,
    });
    Ok(ControlFlow::Continue(()))
}
