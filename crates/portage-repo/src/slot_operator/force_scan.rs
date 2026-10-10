//! Slot-operator update-force and slot-change probes.

use crate::*;

/// [`slot_operator_update_force_scan`]'s result: the consumers joining
/// the replace set and the `(category, package, version)` children whose
/// update the restart must select.
pub(crate) type SlotOpForceScan = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// #236 A: the forcing half of real's slot-operator update probe
/// (`_slot_operator_trigger_reinstalls` -> `_slot_operator_update_probe`
/// -> `_slot_operator_update_backtrack`, `_emerge/depgraph.py:3089-3132`,
/// `:2576-2800`, `:2400-2452`) for the case [`slot_operator_rebuild_scan`]
/// cannot see: the walk bound a built consumer's `:S/SS=` dep to the
/// *installed* child and put no update of that child in the graph at all
/// (the `_minimize_children` collapse makes this the normal shape -- the
/// live `cat/pkg:=` and the appended built atom both bind the installed
/// instance). Real's probe then looks for a visible candidate of the
/// child in the same slot with a different sub-slot and a higher version
/// (downgrades need `_downgrade_probe`, not modelled), which the
/// consumer's replacement ebuild's own atoms on that cp accept
/// (`_iter_similar_available(dep.child, atom)` intersected over the
/// atoms, highest wins), and which every other parent of the child
/// accepts (`_slot_operator_check_reverse_dependencies`, the #211 R2
/// [`probe_refused`] helper). A hit writes both halves of real's
/// backtrack: the consumer into the replace set, the child's candidate
/// into `slot_operator_forced_upgrade`, and the restart selects them.
///
/// Gates, as the scan's #211 arms: the consumer is a walked installed
/// node (real registers `_slot_operator_deps` for walked parents only)
/// that is not merge-bound, neither end is `--exclude`d (`:2589-2603`),
/// `want_update` is stood in for by `--update` or the child being
/// directly requested, the child's graph node is the installed instance
/// the atom is bound to, and a visible replacement ebuild exists. The
/// `_select_atoms_probe` check (`||` choices) and `_too_deep` are not
/// modelled; the consumer's atoms are flattened like the scan's.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_update_force_scan(
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    entries: &[GraphEntry],
    reachable: &HashSet<(String, String)>,
    already: &BTreeSet<(String, String)>,
    undone: &BTreeSet<(String, String)>,
    with_bdeps: bool,
    update: bool,
    top_level_cps: &HashSet<(String, String)>,
    excluded: &[String],
) -> SlotOpForceScan {
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
        excluded
            .iter()
            .any(|a| matches_config_entry(a, &cpv, cat, pkg))
    };
    let probe_parents = collect_probe_parents(
        root, entries, reachable, &walked, &in_graph, already, with_bdeps, excluded,
    );
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

/// Backlog #24 S5: real `_slot_change_probe` (`depgraph.py:2317-2359`),
/// the first arm of `_slot_operator_trigger_reinstalls` (`3103-3107`).
/// For an *unbuilt* slot-operator dep -- `:=` or `:S=`, real's
/// `not (atom.soname or atom.slot_operator_built)`, i.e. anything but
/// the built `:S/SS=` form -- whose parent is an ebuild being merged
/// and whose child the graph resolved to an installed instance, the
/// tree ebuild at the child's own cpv may carry a different
/// `(slot, sub_slot)` than the vdb record: a slot move without a
/// revbump (bug 456208; `test_slot_change_without_revbump.py`,
/// `test_regular_slot_change_without_revbump.py`). Real finds the dep
/// through `_slot_operator_deps` (registered when the parent's dep was
/// walked); portuale re-reads the parent entry's own tree metadata and
/// use-reduces it against the node's enabled USE ([`flat_dep_atoms`])
/// so only deps this run would walk are seen -- `||` arms are flattened
/// (the same over-approximation the vdb side of the scan makes; the S4
/// undo and the probe's own rule-6 keep are the correction for a false
/// hit). [`slot_changed`] is the probe's tree-ebuild-at-cpv lookup. A
/// hit joins `scheduled`, exactly real's `_slot_change_backtrack`
/// (`2361-2399`) writing `slot_operator_replace_installed` +
/// `_need_restart`.
///
/// Real's `runtime_pkg_mask` / excluded-package checks on the tree ebuild
/// (2341/2343) have no portuale counterpart on the installed arm:
/// [`slot_changed`] is the same tolerant metadata re-lookup the standalone
/// `--changed-slot` trigger uses. The binary arm below carries real's
/// visibility gate (`_pkg_visibility_check` 2347, #243).
/// The binary arm (`dep.child` a binary package scheduled for merge) is
/// v2 `#24c`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_slot_change_probe(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    undone: &BTreeSet<(String, String)>,
    scheduled: &mut BTreeSet<(String, String)>,
    excluded: &[String],
    // #212 (v2 `#24c`): every merge-bound binary the arm below finds
    // stale -- `(category, package, version)` triples for
    // `collect_feedback`'s `slot_operator_mask_built` records (real
    // `_slot_change_backtrack`, `depgraph.py:2381-2398`).
    masked: &mut BTreeSet<(String, String, String)>,
    config: &portage_profile::Config,
) {
    const DEP_KEYS: [&str; 5] = ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"];
    for e in entries {
        // Real's `not dep.parent.built`: only an ebuild scheduled for
        // merge is a probing parent.
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
                // Real's `dep.child.built`: the graph must have resolved
                // this dep to that installed instance. A merge-bound
                // entry means the walk chose a different candidate
                // (typically a real upgrade) -- the probe does not fire.
                // (A merge-bound *binary* is the #212 arm below, which
                // runs independently -- real files the two backtracks
                // from independent deps.)
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
                // #212 binary arm (real `_slot_change_backtrack`,
                // `depgraph.py:2381-2398`, v2 `#24c`): the graph resolved
                // this dep to a merge-bound *binary* (`dep.child.built`
                // and not installed), and the tree ebuild at the binary's
                // own cpv (`=cpv`, real `_slot_change_probe`'s
                // `self._pkg(dep.child.cpv, "ebuild", ...)` then
                // `_iter_match_pkgs(..., "ebuild", ...)` fallback) carries
                // a different `(slot, sub_slot)` -- the same slot move
                // without a revbump, but the stale side is a binary, not
                // the vdb. Real masks it (`slot_operator_mask_built`) so
                // the backtrack-config restart resolves the dep to the
                // tree ebuild instead. Portuale's one-instance-per-cp
                // model makes entry existence equivalent to dep
                // resolution: whatever atom pulled this cp sees the
                // binary entry's version.
                //
                // Gates, mirroring real (`_slot_change_probe`
                // `2317-2359`): the tree ebuild at the binary's cpv must
                // exist (a `replacement_parent` -- else masking dead-ends
                // the re-pass, the same gate the #211 arms carry), must
                // not be `--exclude`d (real `:2343`, the same
                // `matches_config_entry` idiom as the C4 grouping and
                // `collect_probe_parents`), and must be visible
                // (`_pkg_visibility_check`, `:2347`, #243: an explicit
                // gate here -- a binary entry does not imply a visible
                // same-version ebuild, and masking a binary whose ebuild
                // is hidden dead-ends the re-pass).
                // Real's `runtime_pkg_mask` check on the tree ebuild
                // (`:2341`) has no counterpart (standing cut, same as
                // the installed arm above).
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
                    // A merge-bound entry always carries its resolved
                    // slot; without it there is nothing to compare
                    // against the tree (real compares package objects
                    // that always have one).
                    let (Some(bin_slot), Some(bin_sub)) =
                        (child_entry.slot.as_deref(), child_entry.sub_slot.as_deref())
                    else {
                        continue;
                    };
                    // The replacement gate and the slot compare read one
                    // tree lookup: the highest-priority repo's candidate
                    // at the binary's own version (the same tolerant
                    // metadata `slot_changed` trusts).
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
                    // Backlog #243: real masks the binary only when the
                    // same-version ebuild passes `_pkg_visibility_check`
                    // (`depgraph.py:2347`); an ebuild a keyword or package
                    // mask hides leaves the binary alone (the live bed
                    // run keeps `[binary]` on `--usepkg` there).
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
