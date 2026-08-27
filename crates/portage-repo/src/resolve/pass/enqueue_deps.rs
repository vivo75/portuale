//! Fold the entry's dependency keys and queue the atoms they name.

use crate::resolve::pass::*;
use portage_profile::Config;
use std::ops::ControlFlow;
use std::sync::Arc;

/// The values [`filter_root_deps`] reads from its caller.
pub(crate) struct FilterRootDepsArgs<'a> {
    pub(crate) flat_deps: Vec<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) key: &'a (String, String),
    pub(crate) state: &'a mut PassState,
    pub(crate) cross_running_root: Option<&'a Path>,
}

/// The values [`enqueue_test_deps`] reads from its caller.
pub(crate) struct EnqueueTestDepsArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) depth: u32,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) metadata: &'a Arc<HashMap<String, String>>,
    pub(crate) config: &'a Config,
    pub(crate) candidate_str: &'a str,
    pub(crate) key: &'a (String, String),
    pub(crate) version: &'a str,
    pub(crate) tokens: &'a Vec<String>,
    pub(crate) state: &'a mut PassState,
    pub(crate) entry_idx: usize,
}

/// Queue the `test?`-gated deps too under `--with-test-deps`.
///
/// Additive, never a replacement. A `test?` dep is never hard (optional),
/// never from `BDEPEND`, never a `||` choice, never running.
pub(crate) fn enqueue_test_deps(args: EnqueueTestDepsArgs<'_>) {
    let EnqueueTestDepsArgs {
        ctx,
        depth,
        use_flags,
        metadata,
        config,
        candidate_str,
        key,
        version,
        tokens,
        state,
        entry_idx,
    } = args;
    // `--with-test-deps` is additive, never a replacement; see
    // `resolve_pretend_graph` for the gating. A `test?` dep is never hard
    // (optional), never from `BDEPEND`, never a `||` choice, never running.
    if ctx.with_test_deps && depth == 0 && !use_flags.contains("test") {
        let iuse_flags: HashSet<String> = metadata
            .get("IUSE")
            .map(String::as_str)
            .unwrap_or_default()
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        let test_masked = config.use_mask.contains("test")
            || specificity_ordered_flags(
                &config.package_use_mask,
                candidate_str,
                &key.0,
                &key.1,
                HashSet::new(),
            )
            .contains("test");
        if iuse_flags.contains("test") && !test_masked {
            let mut test_uselist = use_flags.clone();
            test_uselist.insert("test".to_string());
            let subset: HashSet<String> = ["test".to_string()].into_iter().collect();
            if let Ok(test_deps) = portage_use_reduce::use_reduce_flat_subset(
                tokens,
                &test_uselist,
                portage_use_reduce::MatchMode::Normal,
                &subset,
            ) {
                enqueue_flat_deps(EnqueueFlatDepsArgs {
                    flat_deps: test_deps,
                    key,
                    version,
                    depth,
                    parent_use: &test_uselist,
                    queue: &mut state.queue,
                    pending_blockers: &mut state.pending_blockers,
                    buildtime_atoms: &HashSet::new(),
                    runtime_atoms: &HashSet::new(),
                    bdepend_atoms: &HashSet::new(),
                    or_universe: &HashSet::new(),
                    running_atoms: &HashSet::new(),
                    owner_targets_running_root: state.entries[entry_idx].targets_running_root,
                });
            }
        }
    }
}
