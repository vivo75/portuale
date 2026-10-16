//! The initial outcome for one queued atom: complete-mode pin, sibling collapse, resolve.

use crate::resolve::pass::*;
use portage_profile::Config;

/// The values [`resolve_initial_outcome`] reads from its caller.
pub(crate) struct ResolveInitialOutcomeArgs<'a> {
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) config: &'a Config,
    pub(crate) depth: u32,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) from_disjunction: bool,
    pub(crate) state: &'a mut PassState,
    pub(crate) current_atom: &'a str,
    pub(crate) buildtime_hard: bool,
    pub(crate) item_targets_running_root: bool,
    pub(crate) atom_root: &'a Path,
}

/// The initial outcome for one queued atom: complete-mode pin, sibling collapse, resolve.
///
/// Complete mode is checked before `resolve_pretend`, not after: resolving
/// every installed package fully is the work Portage's cheap check avoids.
pub(crate) fn resolve_initial_outcome(
    args: ResolveInitialOutcomeArgs<'_>,
) -> Result<ControlFlow<(), PretendOutcome>, Error> {
    let ResolveInitialOutcomeArgs {
        union_constraints,
        key,
        ctx,
        config,
        depth,
        bp,
        owner,
        from_disjunction,
        state,
        current_atom,
        buildtime_hard,
        item_targets_running_root,
        atom_root,
    } = args;
    // Complete mode: the deep re-walk may only pick a package already in the
    // graph or installed, never anything new (installed match settles
    // `AlreadyInstalled`; nothing installed drops the node). Before
    // `resolve_pretend`, not after: resolving every installed package fully
    // is the work Portage's cheap check avoids. No-op unless the CLI's
    // complete pass populated `complete_locked_merges`; top-level atoms
    // always resolve (their miss is fatal). Portage: depgraph.py,
    // _complete_graph (_select_pkg_from_graph).
    let complete_locked = ctx.complete
        && !ctx.complete_locked_merges.is_empty()
        && owner.is_some()
        && !ctx.complete_locked_merges.contains(key);
    // Portage `_minimize_children`: per cp with two or more distinct picks,
    // re-point an atom whose pick another selection also satisfies at that
    // one (installed first, ascending). The collapse point: the owner's
    // contiguous same-cp siblings resolve as a group and pin every member.
    // `||`-chosen atoms stay out, like the per-disjunction call.
    // Portage: depgraph.py, _minimize_children
    if let Some(o) = owner.as_ref()
        && !from_disjunction
        && !state
            .minimize_pins
            .contains_key(&(o.clone(), current_atom.to_string()))
    {
        let mut group: Vec<(String, bool, bool)> = vec![(
            current_atom.to_string(),
            buildtime_hard,
            item_targets_running_root,
        )];
        for it in state.queue.iter() {
            if it.owner.as_ref() != Some(o) || it.depth != depth || it.from_disjunction {
                break;
            }
            if let Some(a) = portage_dep::parse_atom(&it.atom)
                && a.blocker == portage_dep::Blocker::None
                && a.category == key.0
                && a.package == key.1
                && !group.iter().any(|(g, _, _)| *g == it.atom)
            {
                group.push((it.atom.clone(), it.buildtime_hard, it.targets_running_root));
            }
        }
        if group.len() >= 2 {
            let mut picks: Vec<(String, PretendOutcome)> = Vec::new();
            for (a, bh, running) in &group {
                let mut out = if complete_locked {
                    match best_installed_for_atom(ctx.root, a, &key.0, &key.1) {
                        Some(v) => PretendOutcome::AlreadyInstalled { version: v },
                        None => continue,
                    }
                } else {
                    resolve_queue_atom(ResolveQueueAtomArgs {
                        ctx,
                        config,
                        bp,
                        depth,
                        union_constraints,
                        key,
                        atom_text: a,
                        running: *running,
                        buildtime_hard: *bh,
                    })?
                };
                // The installed fallback below, applied to the prediction too
                // (each sibling resolves against its own root): a slot atom
                // with an installed match, or no tree or binary candidate
                // at all with an installed satisfier.
                if matches!(out, PretendOutcome::NoVisibleCandidate)
                    && depth != 0
                    && !ctx.empty
                    && (portage_dep::parse_atom(a)
                        .is_some_and(|p| p.slot.is_some() || p.slot_operator.is_some())
                        || !cp_has_tree_or_binary_candidate(
                            &ctx.repos,
                            &ctx.local_binpkg,
                            &key.0,
                            &key.1,
                        ))
                    && let Some(version) = best_installed_matching(
                        if *running {
                            ctx.cross_root()
                                .expect("running-root atom without a cross root")
                        } else {
                            ctx.root
                        },
                        a,
                        config,
                    )
                {
                    out = PretendOutcome::AlreadyInstalled { version };
                }
                picks.push((a.clone(), out));
            }
            // `minimize_children` reads installed slots out of the group's own
            // resolving root.
            for (a, out) in minimize_children(atom_root, &ctx.repos, config, key, picks) {
                state.minimize_pins.insert((o.clone(), a), out);
            }
        }
    }
    let pinned = owner.as_ref().and_then(|o| {
        state
            .minimize_pins
            .remove(&(o.clone(), current_atom.to_string()))
    });
    let outcome = if let Some(pinned) = pinned {
        pinned
    } else if complete_locked {
        match best_installed_for_atom(ctx.root, current_atom, &key.0, &key.1) {
            Some(v) => PretendOutcome::AlreadyInstalled { version: v },
            None => return Ok(ControlFlow::Break(())),
        }
    } else {
        resolve_queue_atom(ResolveQueueAtomArgs {
            ctx,
            config,
            bp,
            depth,
            union_constraints,
            key,
            atom_text: current_atom,
            running: item_targets_running_root,
            buildtime_hard,
        })?
    };
    Ok(ControlFlow::Continue(outcome))
}
