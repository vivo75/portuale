//! Parse and dedup one queued atom, recording its edges.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// What [`prepare_queued_atom`] hands back to its caller.
pub(crate) struct PrepareQueuedAtomOut<'a> {
    pub(crate) key: (String, String),
    pub(crate) atom_root: &'a Path,
    pub(crate) atom: Atom,
}

/// The values [`prepare_queued_atom`] reads from its caller.
pub(crate) struct PrepareQueuedAtomArgs<'a, 's> {
    pub(crate) current_atom: &'s str,
    pub(crate) owner: &'s Option<(String, String)>,
    pub(crate) pprovided_refs: &'s [&'s str],
    pub(crate) depth: u32,
    pub(crate) state: &'s mut PassState,
    pub(crate) item_targets_running_root: bool,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) buildtime_hard: bool,
    pub(crate) item_owner_targets_running_root: bool,
    pub(crate) config: &'s Config,
    pub(crate) unevaluated_atom: &'s Option<String>,
    pub(crate) first_pass: bool,
}

/// Parse and dedup one queued atom, recording its edges.
pub(crate) fn prepare_queued_atom<'a, 's>(
    args: PrepareQueuedAtomArgs<'a, 's>,
) -> ControlFlow<(), PrepareQueuedAtomOut<'a>> {
    let PrepareQueuedAtomArgs {
        current_atom,
        owner,
        pprovided_refs,
        depth,
        state,
        item_targets_running_root,
        ctx,
        buildtime_hard,
        item_owner_targets_running_root,
        config,
        unevaluated_atom,
        first_pass,
    } = args;
    let Some(atom) = portage_dep::parse_atom(current_atom) else {
        if owner.is_some() {
            note_unparsed_dep_token(current_atom, "resolver queue");
        }
        return ControlFlow::Break(());
    };
    if atom.blocker != portage_dep::Blocker::None {
        return ControlFlow::Break(());
    }
    // `package.provided`: an atom satisfied by a listed CPV is already
    // installed (deps drop silently; top-level atoms record for the
    // `WARNING:` block). Portage: dep_check.py; depgraph.py
    if !pprovided_refs.is_empty()
        && portage_dep::match_from_list(current_atom, pprovided_refs).is_some_and(|m| !m.is_empty())
    {
        if depth == 0 && !state.pprovided_atoms.iter().any(|s| s == current_atom) {
            state.pprovided_atoms.push(current_atom.to_string());
        }
        return ControlFlow::Break(());
    }
    let key = (atom.category.clone(), atom.package.clone());
    // This atom's resolving root (Portage `_dep_expand`'s `root_config`); the
    // flag implies a cross root (set only when roots differ).
    let atom_root: &Path = if item_targets_running_root {
        ctx.cross_root()
            .expect("running-root atom without a cross root")
    } else {
        ctx.root
    };
    // A soft running edge back into a visited atom (a running-`BDEPEND`
    // bootstrap cycle) cuts with no edge and no entry; a hard running edge
    // back still records its edge so the cycle reports, as does a same-root
    // soft revisit (both owners list in `--json` `required_by`).
    if owner.is_some()
        && state.visited_atoms.contains(current_atom)
        && item_targets_running_root
        && !buildtime_hard
    {
        return ControlFlow::Break(());
    }
    if let Some(owner) = owner.clone() {
        state
            .required_by_map
            .entry(key.clone())
            .or_default()
            .insert(owner.clone());
        // Edge ends are root-qualified (Portage's `(root, cp)` keys); the child
        // root is the atom's own resolving root.
        let kinds = state
            .edge_kind_map
            .entry((
                (item_targets_running_root, key.0.clone(), key.1.clone()),
                (
                    item_owner_targets_running_root,
                    owner.0.clone(),
                    owner.1.clone(),
                ),
            ))
            .or_insert((false, false));
        // Only a genuinely unsatisfied build-time dep counts as `has_hard`: a
        // satisfied one (`DepPrioritySatisfiedRange` can ignore it) is not
        // unbreakable. Satisfaction honours use-deps against the installed
        // instance's USE and reads this atom's own resolving root.
        // Portage: DepPriority.satisfied (vardb.match_pkgs).
        if buildtime_hard && best_installed_matching(atom_root, current_atom, config).is_none() {
            kinds.0 = true;
        } else {
            kinds.1 = true;
        }
    }
    if !state.visited_atoms.insert(current_atom.to_string()) {
        // Already resolved, but Portage's live `--debug` trace still narrates
        // the re-visit: re-record the edge for the dump's bounded
        // re-narration. `--debug` only; production runs are bit-identical.
        if resolver_debug()
            && let Some(prior) = state
                .parent_atoms
                .iter()
                .find(|r| r.atom == current_atom)
                .map(|r| {
                    (
                        r.child_category.clone(),
                        r.child_package.clone(),
                        r.child_version.clone(),
                        r.child_installed,
                    )
                })
        {
            let row = resolver_trace::ParentAtom {
                child_category: prior.0,
                child_package: prior.1,
                child_version: prior.2,
                child_installed: prior.3,
                parent: owner.clone(),
                atom: current_atom.to_string(),
                unevaluated: unevaluated_atom.clone(),
            };
            if !state.parent_atoms.contains(&row) {
                state.parent_atoms.push(row);
            }
        }
        return ControlFlow::Break(());
    }
    // `--debug` Stage 3: every candidate matching this atom, in Portage's
    // `dbs` order. First pass only. Portage: depgraph.py,
    // _wrapped_select_pkg_highest_available_imp
    if resolver_debug() && first_pass {
        resolver_trace::dump_atom_candidates(&ctx.repos, ctx.root, current_atom, &key.0, &key.1);
    }
    // Record this atom as a constraint pulling `cat/pkg` (Portage sees the
    // whole atom set, not just the first).
    state
        .slot_want
        .entry(key.clone())
        .or_default()
        .push(current_atom.to_string());
    // A top-level atom is an "Argument" puller for the slot-collision block;
    // `depth == 0` excludes the auto-replace seeds (an internal set, never
    // an argument). Portage: slot_collision_handler.
    if owner.is_none() && depth == 0 {
        state.slot_pullers.entry(key.clone()).or_default().push((
            String::new(),
            String::new(),
            String::new(),
            current_atom.to_string(),
        ));
    }
    ControlFlow::Continue(PrepareQueuedAtomOut {
        key,
        atom_root,
        atom,
    })
}
