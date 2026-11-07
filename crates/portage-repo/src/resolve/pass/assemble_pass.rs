//! The pass result from the finished walk state.

use crate::resolve::pass::*;
use std::collections::BTreeMap;

/// The pass result from the finished walk state.
///
/// REQUIRED_USE violations fail here, after the whole walk, so every
/// reachable candidate still resolves on its own terms.
pub(crate) fn assemble_pass_result(
    ctx: &ResolveCtx<'_>,
    state: PassState,
    orphan_blockers: Vec<OrphanBlocker>,
) -> Result<PassResult, Error> {
    if !state.required_use_violations.is_empty() {
        // Each block is self-delimiting (leading + trailing newline).
        return Err(Error::Detail(state.required_use_violations.join("")));
    }

    let mut entries = state.entries;
    mark_walk_or_choices(&mut entries, &state.walk_or_choices);
    let pass = PassResult {
        entries,
        suppressed_nvc: state.suppressed_nvc,
        slot_conflicts: state.slot_conflicts,
        skipped_updates: state.skipped_updates,
        orphan_blockers,
        slot_want: state.slot_want,
        slot_pullers: state.slot_pullers,
        masked_deps: state.masked_deps,
        use_unsat_deps: state.use_unsat_deps,
        use_only_masked_misses: state.use_only_masked_misses,
        plain_miss_deps: state.plain_miss_deps,
        nvc_dep_atoms: state.nvc_dep_atoms,
        missing_dep_trigger: state.missing_dep_trigger,
        missing_args: state.missing_args,
        autounmask_grew: state.autounmask_grew,
        parent_flip_rescued: state.parent_flip_rescued,
        edge_kind_map: state.edge_kind_map,
        changed_deps_report_entries: state.changed_deps_report_entries,
        // The `WARNING: … package.provided:` block lists requested atoms in requested
        // order (not walk order); atoms absent from argv sort last, stably.
        pprovided_atoms: {
            let mut v = state.pprovided_atoms;
            let pos = |a: &String| ctx.atoms.iter().position(|x| x == a).unwrap_or(usize::MAX);
            v.sort_by_key(pos);
            v
        },
        autounmask_keyword_changes: state.autounmask_keyword_changes,
        autounmask_use_changes: state.autounmask_use_changes,
        autounmask_license_changes: state.autounmask_license_changes,
        autounmask_mask_changes: state.autounmask_mask_changes,
        use_overlay: state.use_overlay,
        use_change_overlay: state.use_change_overlay,
        use_broke: state.use_broke,
        abi_rebuilds: None,
        parent_atoms: state.parent_atoms,
        installed_walked: state.installed_walked,
    };
    Ok(pass)
}

/// Mark, per entry and `||` group, the first branch whose atoms are all among
/// the atoms the walk chose for that owner (#625).
// Portage: depgraph.py, _add_pkg_dep_string (edges only for selected atoms)
fn mark_walk_or_choices(
    entries: &mut [GraphEntry],
    choices: &HashMap<(String, String, String, bool), HashSet<String>>,
) {
    for e in entries.iter_mut() {
        let version = match &e.outcome {
            PretendOutcome::AlreadyInstalled { version }
            | PretendOutcome::New { version }
            | PretendOutcome::Reinstall { version, .. } => version,
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => continue,
        };
        let Some(chosen) = choices.get(&(
            e.category.clone(),
            e.package.clone(),
            version.clone(),
            e.targets_running_root,
        )) else {
            continue;
        };
        let mut branches: BTreeMap<(u8, u32), BTreeMap<u32, bool>> = BTreeMap::new();
        for edge in &e.deps {
            if let Some((g, b)) = edge.alt
                && g != u32::MAX
            {
                let hit = chosen.contains(&edge.atom) || chosen.contains(&edge.evaluated);
                *branches
                    .entry((edge.key, g))
                    .or_default()
                    .entry(b)
                    .or_insert(true) &= hit;
            }
        }
        let picks: HashSet<(u8, u32, u32)> = branches
            .into_iter()
            .filter_map(|((k, g), bs)| bs.into_iter().find(|(_, all)| *all).map(|(b, _)| (k, g, b)))
            .collect();
        for edge in &mut e.deps {
            if let Some((g, b)) = edge.alt {
                edge.walk_chosen = picks.contains(&(edge.key, g, b));
            }
        }
    }
}
