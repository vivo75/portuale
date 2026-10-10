//! Restart when the merge-order walk found no leaf: record the cycle's edges.

use crate::resolve::feedback::*;

/// Restart when the merge-order walk found no leaf: record the cycle's edges.
pub(crate) fn step_no_leaf_restart(
    ctx: &ResolveCtx<'_>,
    has_nvc: bool,
    pass: &mut PassResult,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // Backlog #221: real `_serialize_tasks`' no-leaf restart
    // (`_emerge/depgraph.py:10264-10296`, bugs 703440, 705986). A pass
    // that settled everywhere else but stranded the merge-order walk on
    // a dependency cycle records the cycle's edges into
    // `circular_dependency` and re-runs the whole walk, on which
    // `disjunction_preference` demotes the cycle-closing `||` branches
    // (see its `circular_atom` site). Placed after every other feedback:
    // real only reaches serialization once graph creation (slot
    // conflicts, missing deps, autounmask growth, reverse-dep pins, the
    // slot-operator trigger) settled. Gated on `--backtrack=0` (real's
    // `_allow_backtracking` -- the `--backtrack=0` run reports the
    // circular error exactly as before), on no unsatisfiable dependency
    // (real never serializes a failed `_create_graph`), and on no
    // surviving slot conflict or orphan blocker (real backtracks those
    // first; a conflict-only fall-through still settles below).
    // `unsolved_cycle`: a cycle re-using an already-recorded node means
    // the demotion did not break it -- settle into the circular report
    // instead of retrying, exactly like real's `_skip_restart`. Every
    // retry therefore strictly grows the map, so the search terminates.
    // `Circular` feedback is budget-free (the driver treats it like
    // `Config`: no mask step, and under `--backtrack=0` the pass
    // settles into the circular report) -- shared with backlog #216's
    // fresh-self-loop arm above, which emits the same variant.
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
            // Track X Slice C (#242): node-identity keys (real's
            // package-object keying) -- a both-roots cp reusing an
            // already-recorded node in the *other* root is a new edge,
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
                        // Real's rotation: `circular_child = cycle[-1]`
                        // for index 0, `cycle[index - 1]` otherwise --
                        // each node's in-cycle dependency.
                        let child = &pass.entries[cycle[(i + cycle.len() - 1) % cycle.len()]];
                        let parent = &pass.entries[idx];
                        // Merge-bound by construction (both trigger arms
                        // only link merge-bound entries), so the version
                        // is never empty in practice; skip belt-and-braces
                        // (M1): an empty version would build a `cat/pkg-`
                        // candidate no atom matches.
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
