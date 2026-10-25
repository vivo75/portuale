//! Decides when complete mode switches itself on.

use crate::*;

/// Arguments of [`complete_graph_auto_enable`].
pub struct CompleteGraphAutoEnableArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub if_new_use: bool,
    pub if_new_ver: bool,
    pub if_new_slot: bool,
    pub root: &'a Path,
    pub config: &'a portage_profile::Config,
    pub gate: CategoryGate<'a>,
}

/// Whether a first pass that merged or reinstalled anything installed
/// requires a second, complete-mode resolve.
// Portage: depgraph.py, _complete_graph
pub fn complete_graph_auto_enable(args: CompleteGraphAutoEnableArgs<'_>) -> bool {
    let CompleteGraphAutoEnableArgs {
        entries,
        if_new_use,
        if_new_ver,
        if_new_slot,
        root,
        config,
        gate,
    } = args;
    entries.iter().any(|e| match &e.outcome {
        PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. } => if_new_ver,
        PretendOutcome::New { .. } if e.new_slot => if_new_ver || if_new_slot,
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            slot_changed,
            ..
        } => {
            (if_new_ver && *slot_changed)
                || (if_new_use
                    && (!changed_flags.is_empty()
                        || merge_use_changed_vs_installed(MergeUseChangedVsInstalledArgs {
                            root,
                            config,
                            category: &e.category,
                            package: &e.package,
                            version,
                            gate,
                        })
                        .unwrap_or(false)))
        }
        _ => false,
    })
}
