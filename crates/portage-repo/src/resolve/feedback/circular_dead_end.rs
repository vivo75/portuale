//! A hard build-time cycle the merge order cannot serialize.

use crate::resolve::feedback::*;

/// A hard build-time cycle the merge order cannot serialize.
pub(crate) fn step_circular_dead_end(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // Portage: depgraph.py, _serialize_tasks (circular dead-end). The walk finished
    // but the graph does not serialize; record every (node -> in-cycle predecessor)
    // edge into the circular map and retry, on which `||` demotion breaks the cycle.
    // A cycle adding nothing new is unsolved: a dead end reported from the best run.
    //
    // Only the fresh self-pick shape retries here; every other hard cycle falls
    // through to the later arms, so their reports are byte-identical either way.
    //
    // Placed before the slot-conflict arms: a deadlocked graph pre-empts conflict
    // repair (Portage never reaches conflict processing with a failed serialize).
    let hard_cycles = find_hard_cycles(
        &pass.entries,
        &pass.edge_kind_map,
        ctx.root,
        &grown.circular_dependency,
        // Only the single-node `cycle_restartable` gate reads the result, so the
        // insertion rank (a post-sort notion) is meaningless here; `None` falls
        // back to `discovery` order.
        None,
    );
    if hard_cycles.len() == 1
        && cycle_restartable(
            &pass.entries,
            &kept_alt_branches(&pass.entries, ctx.root, &grown.circular_dependency),
            &hard_cycles[0],
            ctx.root,
        )
    {
        let mut new_info = false;
        for cycle in &hard_cycles {
            for (index, node_cpv) in cycle.iter().enumerate() {
                let Some((node_cat, node_pkg, _)) = split_cpv(node_cpv) else {
                    continue;
                };
                // The cycle cpv may name two merge-bound instances (one per root):
                // record the one with the hard self edge in the root-qualified walk
                // map. With no hard self pair, keep the legacy target-root key.
                let cycler_running = pass
                    .edge_kind_map
                    .get(&(
                        (true, node_cat.clone(), node_pkg.clone()),
                        (true, node_cat.clone(), node_pkg.clone()),
                    ))
                    .is_some_and(|(has_hard, _)| *has_hard);
                // Portage's own predecessor formula: `cycle[-1]` for index 0,
                // `cycle[index-1]` otherwise.
                let child_cpv = if index == 0 {
                    &cycle[cycle.len() - 1]
                } else {
                    &cycle[index - 1]
                };
                let Some((child_cat, child_pkg, child_ver)) = split_cpv(child_cpv) else {
                    continue;
                };
                // An empty version builds a `cat/pkg-` candidate that no atom ever
                // matches (M1): skip instead of recording a dead edge that would
                // still count as new info and force a futile retry.
                if child_ver.is_empty() {
                    continue;
                }
                // The merge-bound entry behind the child cpv carries the slot/sub-slot
                // an atom's `:slot` pin matches against.
                let (child_slot, child_sub_slot) = pass
                    .entries
                    .iter()
                    .find(|e| merge_bound_cpv(e).as_deref() == Some(child_cpv.as_str()))
                    .map(|e| (e.slot.clone(), e.sub_slot.clone()))
                    .unwrap_or((None, None));
                let record = CircularDepChild {
                    category: child_cat,
                    package: child_pkg,
                    version: child_ver,
                    slot: child_slot,
                    sub_slot: child_sub_slot,
                };
                let bucket = grown
                    .circular_dependency
                    .entry((cycler_running, node_cat, node_pkg))
                    .or_default();
                if !bucket.contains(&record) {
                    bucket.push(record);
                    new_info = true;
                }
            }
        }
        if new_info {
            return Some(PassDecision::Feedback(BacktrackFeedback::Circular {
                params: Box::new(std::mem::take(grown)),
            }));
        }
        return Some(PassDecision::DeadEnd {
            params: Box::new(std::mem::take(grown)),
        });
    }
    None
}
