//! Slot-operator update-force and slot-change probes.

use crate::*;

/// [`slot_operator_update_force_scan`]'s result: the consumers joining
/// the replace set and the `(category, package, version)` children whose
/// update the restart must select.
pub(crate) type SlotOpForceScan = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// Arguments of [`slot_operator_update_force_scan`].
pub(crate) struct SlotOperatorUpdateForceScanArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) undone: &'a BTreeSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) excluded: &'a [String],
}

/// The forcing half of Portage's slot-operator update probe: the walk bound
/// a built consumer's `:S/SS=` dep to the installed child and put no update
/// of that child in the graph, but a visible same-slot higher-version
/// candidate exists that the consumer's replacement ebuild and every other
/// parent accept. A hit schedules the consumer rebuild plus the forced child
/// upgrade.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls, _slot_operator_update_probe, _slot_operator_update_backtrack
pub(crate) fn slot_operator_update_force_scan(
    args: SlotOperatorUpdateForceScanArgs<'_>,
) -> SlotOpForceScan {
    let SlotOperatorUpdateForceScanArgs {
        root,
        repos,
        config,
        entries,
        reachable,
        already,
        undone,
        with_bdeps,
        update,
        top_level_cps,
        excluded,
    } = args;
    let mut consumers: BTreeSet<(String, String)> = BTreeSet::new();
    let mut forced: BTreeSet<(String, String, String)> = BTreeSet::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    // cp -> version of every walked installed node.
    let mut installed_nodes: HashMap<(String, String), Vec<String>> = HashMap::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        match &e.outcome {
            PretendOutcome::AlreadyInstalled { version } => {
                installed_nodes.entry(cp).or_default().push(version.clone());
            }
            PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => {}
            _ => {
                in_graph.insert(cp);
            }
        }
    }
    if installed_nodes.is_empty() {
        return (consumers, forced);
    }
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let is_excluded = |cat: &str, pkg: &str, ver: &str| {
        let cpv = format!("{cat}/{pkg}-{ver}");
        excluded.iter().any(|a| matches_cli_option_atom(a, &cpv))
    };
    let probe_parents = collect_probe_parents(CollectProbeParentsArgs {
        root,
        entries,
        reachable,
        walked: &walked,
        in_graph: &in_graph,
        already,
        with_bdeps,
        excluded,
    });
    let mut scheduled: BTreeSet<(String, String)> = already.clone();
    let mut consumer_cps: Vec<&(String, String)> = installed_nodes.keys().collect();
    consumer_cps.sort();
    for cp in consumer_cps {
        if undone.contains(cp) || in_graph.contains(cp) || scheduled.contains(cp) {
            continue;
        }
        for version in &installed_nodes[cp] {
            if is_excluded(&cp.0, &cp.1, version) {
                continue;
            }
            let Some(live) = live_metadata_for_installed(repos, root, &cp.0, &cp.1, version) else {
                continue;
            };
            if !ebuild_visible_at(repos, &cp.0, &cp.1, version, config) {
                continue;
            }
            let dep_keys: &[&str] = if with_bdeps {
                &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"]
            } else {
                &["RDEPEND", "PDEPEND", "IDEPEND"]
            };
            let use_flags = read_vdb_flag_set(root, &cp.0, &cp.1, version, "USE");
            let mut live_atoms: Vec<String> = Vec::new();
            for key in dep_keys {
                if let Some(toks) = flat_dep_atoms(
                    live.get(*key).map(String::as_str).unwrap_or_default(),
                    &use_flags,
                ) {
                    live_atoms.extend(toks);
                }
            }
            let mut built: Vec<portage_dep::Atom> = dep_keys
                .iter()
                .flat_map(|key| {
                    read_vdb_string(root, &cp.0, &cp.1, version, key)
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .filter_map(|tok| portage_dep::parse_atom(&tok))
                .filter(|a| {
                    a.blocker == portage_dep::Blocker::None
                        && a.slot_operator == Some(portage_dep::SlotOperator::Equals)
                        && a.slot.is_some()
                        && a.sub_slot.is_some()
                })
                .collect();
            built.sort_by(|a, b| (&a.category, &a.package).cmp(&(&b.category, &b.package)));
            for atom in built {
                let child_cp = (atom.category.clone(), atom.package.clone());
                if in_graph.contains(&child_cp)
                    || forced
                        .iter()
                        .any(|(c, p, _)| (c, p) == (&child_cp.0, &child_cp.1))
                {
                    continue;
                }
                if !update && !top_level_cps.contains(&child_cp) {
                    continue;
                }
                let (a_slot, a_sub) = (atom.slot.clone().unwrap(), atom.sub_slot.clone().unwrap());
                // The graph's child: the walked installed instance in the
                // bound slot.
                let Some(child_ver) = installed_nodes.get(&child_cp).and_then(|vs| {
                    vs.iter()
                        .find(|v| read_vdb_slot(root, &child_cp.0, &child_cp.1, v).0 == a_slot)
                        .cloned()
                }) else {
                    continue;
                };
                if is_excluded(&child_cp.0, &child_cp.1, &child_ver) {
                    continue;
                }
                let (_, child_sub) = read_vdb_slot(root, &child_cp.0, &child_cp.1, &child_ver);
                let child_sub = if child_sub.is_empty() {
                    a_slot.clone()
                } else {
                    child_sub
                };
                // The replacement parent's atoms on the child's cp.
                let parent_atoms: Vec<&str> = live_atoms
                    .iter()
                    .filter_map(|t| {
                        let a = portage_dep::parse_atom(t)?;
                        (a.blocker == portage_dep::Blocker::None
                            && a.category == child_cp.0
                            && a.package == child_cp.1)
                            .then(|| portage_dep::without_use(t))
                    })
                    .collect();
                if parent_atoms.is_empty() {
                    continue;
                }
                let Ok(cands) = list_candidates(repos, &child_cp.0, &child_cp.1) else {
                    continue;
                };
                let mut best: Option<(&Candidate, String)> = None;
                for c in cands.iter() {
                    if c.slot != a_slot
                        || c.sub_slot == child_sub
                        || c.sub_slot == a_sub
                        || vercmp_ordering(&c.version, &child_ver) != Ordering::Greater
                        || !is_visible(c, &child_cp.0, &child_cp.1, config)
                    {
                        continue;
                    }
                    let s = format!(
                        "{}/{}-{}:{}/{}::{}",
                        child_cp.0, child_cp.1, c.version, c.slot, c.sub_slot, c.repo_name
                    );
                    let ok = parent_atoms.iter().all(|a| {
                        portage_dep::match_from_list(a, &[s.as_str()])
                            .is_some_and(|m| !m.is_empty())
                    });
                    if !ok {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(b, _)| {
                        vercmp_ordering(&c.version, &b.version) == Ordering::Greater
                    }) {
                        best = Some((c, s));
                    }
                }
                let Some((cand, _)) = best else { continue };
                let cand_str = format!(
                    "{}/{}-{}:{}/{}",
                    child_cp.0, child_cp.1, cand.version, cand.slot, cand.sub_slot
                );
                if probe_refused(&probe_parents, &scheduled, &child_cp, &cand_str, cp) {
                    continue;
                }
                consumers.insert(cp.clone());
                scheduled.insert(cp.clone());
                forced.insert((child_cp.0.clone(), child_cp.1.clone(), cand.version.clone()));
            }
        }
    }
    (consumers, forced)
}

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
                    if excluded
                        .iter()
                        .any(|ex| matches_cli_option_atom(ex, &tree_str))
                    {
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
