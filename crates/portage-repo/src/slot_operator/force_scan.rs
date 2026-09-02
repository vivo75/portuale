//! Slot-operator update-force and slot-change probes.

use crate::*;

/// Arguments of [`slot_operator_slot_change_probe`].
pub(crate) struct SlotOperatorSlotChangeProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) undone: &'a BTreeSet<(String, String)>,
    pub(crate) scheduled: &'a mut BTreeSet<(String, String)>,
    pub(crate) excluded: &'a [String],
    /// Every merge-bound binary the arm below finds stale, for
    /// `collect_feedback`'s `slot_operator_mask_built` records.
    pub(crate) masked: &'a mut BTreeSet<(String, String, String)>,
    pub(crate) config: &'a portage_profile::Config,
}

/// Portage's `_slot_change_probe`: a slot move without a revbump. For an
/// unbuilt slot-operator dep (`:=` or `:S=`) whose parent is a merging ebuild
/// and whose child resolved to an installed instance, the tree ebuild at the
/// child's own cpv may carry a different `(slot, sub_slot)` than the vdb
/// record. A hit joins `scheduled`; the binary arm reports stale binaries
/// into `masked` instead.
// Portage: _emerge/depgraph.py, _slot_change_probe, _slot_change_backtrack
pub(crate) fn slot_operator_slot_change_probe(args: SlotOperatorSlotChangeProbeArgs<'_>) {
    let SlotOperatorSlotChangeProbeArgs {
        root,
        repos,
        entries,
        undone,
        scheduled,
        excluded,
        masked,
        config,
    } = args;
    const DEP_KEYS: [&str; 5] = ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"];
    for e in entries {
        // Only a merging ebuild is a probing parent.
        if e.source != CandidateSource::Ebuild {
            continue;
        }
        let version = match &e.outcome {
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => version,
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
            _ => continue,
        };
        let cp = (e.category.clone(), e.package.clone());
        let Some(metadata) = tree_metadata_for(repos, &cp, version) else {
            continue;
        };
        let use_flags: HashSet<String> = e
            .use_flags_display
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| f.clone())
            .collect();
        for key in DEP_KEYS {
            let Some(tokens) = flat_dep_atoms(
                metadata.get(key).map(String::as_str).unwrap_or_default(),
                &use_flags,
            ) else {
                continue;
            };
            for token in tokens {
                let Some(atom) = portage_dep::parse_atom(&token) else {
                    continue;
                };
                if atom.slot_operator != Some(portage_dep::SlotOperator::Equals)
                    || atom.sub_slot.is_some()
                {
                    continue;
                }
                let child_cp = (atom.category.clone(), atom.package.clone());
                if undone.contains(&child_cp) || scheduled.contains(&child_cp) {
                    continue;
                }
                // The graph must have resolved this dep to that installed
                // instance; a merge-bound entry means the walk chose a
                // different candidate. (A merge-bound binary is the arm
                // below, which runs independently.)
                if let Some(installed_version) =
                    best_installed_for_atom(root, &token, &atom.category, &atom.package)
                {
                    let resolved_installed = entries.iter().any(|x| {
                        x.category == child_cp.0
                            && x.package == child_cp.1
                            && matches!(
                                &x.outcome,
                                PretendOutcome::AlreadyInstalled { version } if *version == installed_version
                            )
                    });
                    // The probe itself: the tree ebuild at the child's own
                    // cpv moved (slot, sub_slot) vs the installed instance.
                    if resolved_installed
                        && slot_changed(
                            root,
                            repos,
                            &atom.category,
                            &atom.package,
                            &installed_version,
                        )
                    {
                        scheduled.insert(child_cp.clone());
                    }
                }
                // Binary arm: the graph resolved this dep to a merge-bound
                // binary whose own cpv moved in the tree. Mask it so the
                // restart resolves the dep to the tree ebuild instead.
                // The tree ebuild must exist (else masking dead-ends the
                // re-pass), must not be `--exclude`d, and must be visible:
                // masking a binary whose ebuild is hidden dead-ends the
                // re-pass. No `runtime_pkg_mask` check on the tree ebuild
                // (standing cut, same as the installed arm).
                for child_entry in entries
                    .iter()
                    .filter(|x| x.category == child_cp.0 && x.package == child_cp.1)
                {
                    if child_entry.source != CandidateSource::Binary {
                        continue;
                    }
                    let binary_version = match &child_entry.outcome {
                        PretendOutcome::New { version }
                        | PretendOutcome::Reinstall { version, .. } => version,
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } => to,
                        _ => continue,
                    };
                    // Without a resolved slot there is nothing to compare
                    // against the tree.
                    let (Some(bin_slot), Some(bin_sub)) =
                        (child_entry.slot.as_deref(), child_entry.sub_slot.as_deref())
                    else {
                        continue;
                    };
                    // The replacement gate and the slot compare read one
                    // tree lookup: the highest-priority repo's candidate
                    // at the binary's own version.
                    let Ok(tree_cands) = list_candidates(repos, &atom.category, &atom.package)
                    else {
                        continue;
                    };
                    let Some(tree) = tree_cands
                        .iter()
                        .filter(|c| c.version == *binary_version)
                        .max_by_key(|c| c.repo_priority)
                    else {
                        continue;
                    };
                    let tree_str = format!(
                        "{}/{}-{}:{}/{}::{}",
                        atom.category,
                        atom.package,
                        tree.version,
                        tree.slot,
                        tree.sub_slot,
                        tree.repo_name
                    );
                    if excluded.iter().any(|ex| {
                        matches_config_entry(ex, &tree_str, &atom.category, &atom.package)
                    }) {
                        continue;
                    }
                    // Mask the binary only when the same-version ebuild is
                    // visible; an ebuild a keyword or package mask hides
                    // leaves the binary alone.
                    if !is_visible(tree, &atom.category, &atom.package, config) {
                        continue;
                    }
                    if (bin_slot, bin_sub) != (tree.slot.as_str(), tree.sub_slot.as_str()) {
                        masked.insert((
                            child_cp.0.clone(),
                            child_cp.1.clone(),
                            binary_version.clone(),
                        ));
                    }
                }
            }
        }
    }
}
