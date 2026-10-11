//! Fill entry required_by and drop shadowed installed entries.

use crate::resolve::pass::*;

/// Fill entry required_by and drop shadowed installed entries.
pub(crate) fn finish_required_by(state: &mut PassState, ctx: &ResolveCtx<'_>) {
    for entry in &mut state.entries {
        if let Some(owners) = state
            .required_by_map
            .get(&(entry.category.clone(), entry.package.clone()))
        {
            let mut owners: Vec<(String, String)> = owners.iter().cloned().collect();
            owners.sort();
            entry.required_by = owners;
        }
    }

    // An installed package also merged in the same slot is one node, not two:
    // drop the redundant `AlreadyInstalled` entry (the merge already carries
    // the union `required_by`, filled just above).
    let mergebound_cp_slots: HashSet<(String, String, String)> = state
        .entries
        .iter()
        .filter_map(|e| {
            let ver = match &e.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
                _ => return None,
            };
            let slot = e
                .slot
                .clone()
                .unwrap_or_else(|| read_vdb_slot(ctx.root, &e.category, &e.package, ver).0);
            Some((e.category.clone(), e.package.clone(), slot))
        })
        .collect();
    state.entries.retain(|e| {
        let PretendOutcome::AlreadyInstalled { version } = &e.outcome else {
            return true;
        };
        let (slot, _) = read_vdb_slot(ctx.root, &e.category, &e.package, version);
        !mergebound_cp_slots.contains(&(e.category.clone(), e.package.clone(), slot))
    });
}
