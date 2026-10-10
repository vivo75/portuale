//! Restart when the merge-order walk found no leaf: record the cycle's edges.

use crate::resolve::feedback::*;

/// Restart when the merge-order walk found no leaf: record the cycle's edges.
pub(crate) fn step_no_leaf_restart(
    ctx: &ResolveCtx<'_>,
    has_nvc: bool,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _serialize_tasks (no-leaf restart). A pass that settled
    // everywhere else but stranded the merge-order walk on a dependency cycle
    // records the cycle's edges and re-runs, on which `||` demotion breaks it.
    // Placed after every other feedback: Portage only serializes once graph
    // creation settled. Gated on a live search, no unsatisfiable dep, and no
    // surviving slot conflict or orphan blocker. An already-recorded cycle means
    // the demotion did not break it -- settle into the report instead of retrying.
    // Budget-free; every retry strictly grows the map, so the search terminates.
    if ctx.backtrack_max > 0
        && !has_nvc
        && pass.slot_conflicts.is_empty()
        && pass.orphan_blockers.is_empty()
    {
        let cycles = find_restart_cycles(
            &pass.entries,
            &pass.edge_kind_map,
            ctx.root,
            &grown.circular_dependency,
        );
        if !cycles.is_empty() {
            // Node-identity keys (Portage's package-object keying): a both-roots cp
            // reusing an already-recorded node in the *other* root is a new edge,
            // not an unsolved cycle.
            let unsolved = cycles.iter().flatten().any(|&idx| {
                grown.circular_dependency.contains_key(&(
                    pass.entries[idx].targets_running_root,
                    pass.entries[idx].category.clone(),
                    pass.entries[idx].package.clone(),
                ))
            });
            if !unsolved {
                for cycle in &cycles {
                    for (i, &idx) in cycle.iter().enumerate() {
                        // Portage's rotation: each node's in-cycle dependency is
                        // `cycle[-1]` for index 0, `cycle[index-1]` otherwise.
                        let child = &pass.entries[cycle[(i + cycle.len() - 1) % cycle.len()]];
                        let parent = &pass.entries[idx];
                        // Merge-bound by construction, so the version is never empty in
                        // practice; skip belt-and-braces (M1): an empty version would
                        // build a `cat/pkg-` candidate no atom matches.
                        let Some(child_version) = merge_bound_version(&child.outcome).cloned()
                        else {
                            continue;
                        };
                        if child_version.is_empty() {
                            continue;
                        }
                        let record = CircularDepChild {
                            category: child.category.clone(),
                            package: child.package.clone(),
                            version: child_version,
                            slot: child.slot.clone(),
                            sub_slot: child.sub_slot.clone(),
                        };
                        let bucket = grown
                            .circular_dependency
                            .entry((
                                parent.targets_running_root,
                                parent.category.clone(),
                                parent.package.clone(),
                            ))
                            .or_default();
                        if !bucket.contains(&record) {
                            bucket.push(record);
                        }
                    }
                }
                return Some(PassDecision::Feedback(BacktrackFeedback::Circular {
                    params: Box::new(std::mem::take(grown)),
                }));
            }
        }
    }
    None
}
