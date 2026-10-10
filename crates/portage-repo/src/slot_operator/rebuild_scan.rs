//! Rebuilds triggered by slot-operator dependencies.

use crate::*;

/// [`slot_operator_rebuild_scan`]'s result: the consumer `(cat, pkg)`
/// replace set, the `(provider_cpv, consumer_cpv)` display pairs, and the
/// stale-binary `(category, package, version)` triples.
pub(crate) type SlotOpRebuildScan = (
    BTreeSet<(String, String)>,
    Vec<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// Arguments of [`slot_operator_rebuild_scan`].
pub(crate) struct SlotOperatorRebuildScanArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) undone: &'a BTreeSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) top_level_atoms: &'a HashSet<String>,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
}

/// Post-walk scan for consumers needing a slot-operator rebuild: the same-slot
/// arm (bound sub-slot moved under a replacing provider) and the new-slot arm
/// (bound slot abandoned for a fresh one). Returns the replace set, the display
/// pairs, and the stale-binary triples.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls
pub(crate) fn slot_operator_rebuild_scan(
    args: SlotOperatorRebuildScanArgs<'_>,
) -> SlotOpRebuildScan {
    let SlotOperatorRebuildScanArgs {
        root,
        repos,
        entries,
        reachable,
        already,
        undone,
        with_bdeps,
        update,
        top_level_cps,
        top_level_atoms,
        excluded,
        config,
    } = args;
    // cp -> (new version, new slot, new sub-slot) for every entry that
    // replaces an installed version in that slot.
    let mut new_slot: HashMap<(String, String), (String, String, String)> = HashMap::new();
    // cp -> (new version, new slot, new sub-slot) for every entry merged
    // into a fresh slot (no installed instance there).
    let mut new_slot_fresh: HashMap<(String, String), (String, String, String)> = HashMap::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        // A removal installs nothing, so it is not "in graph".
        if !matches!(
            e.outcome,
            PretendOutcome::AlreadyInstalled { .. }
                | PretendOutcome::NoVisibleCandidate
                | PretendOutcome::Uninstall { .. }
        ) {
            in_graph.insert((e.category.clone(), e.package.clone()));
        }
        let version = match &e.outcome {
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => Some(to),
            PretendOutcome::Reinstall { version, .. } => Some(version),
            _ => None,
        };
        if let (Some(version), Some(slot), Some(sub_slot)) =
            (version, e.slot.clone(), e.sub_slot.clone())
        {
            let cp = (e.category.clone(), e.package.clone());
            // Under `--update`, a downgraded non-requested provider was held
            // back by a consumer's built `:=` pin; model the missed update
            // from the replaced version's slot so the arm below fires.
            let missed = match &e.outcome {
                PretendOutcome::Downgrade { from, .. }
                    if update && !top_level_cps.contains(&cp) =>
                {
                    let (from_slot, from_sub) = read_vdb_slot(root, &cp.0, &cp.1, from);
                    (!from_slot.is_empty() && from_slot == slot && from_sub != sub_slot)
                        .then_some((from.clone(), from_slot, from_sub))
                }
                _ => None,
            };
            new_slot.insert(cp, missed.unwrap_or((version.clone(), slot, sub_slot)));
        }
        if let PretendOutcome::New { version } = &e.outcome
            && let (Some(slot), Some(sub_slot)) = (e.slot.clone(), e.sub_slot.clone())
        {
            new_slot_fresh.insert(
                (e.category.clone(), e.package.clone()),
                (version.clone(), slot, sub_slot),
            );
        }
    }
    let installed = all_installed_packages(root);
    // cp -> every installed (version, main slot).
    let mut installed_by_cp: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
    for p in &installed {
        installed_by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push((p.version.clone(), p.slot.clone()));
    }
    let mut scheduled: BTreeSet<(String, String)> = already.clone();
    let mut abi_rebuilds: Vec<(String, String)> = Vec::new();
    // Every cp this pass actually walked (any outcome). Only consumers
    // the walk reached can fire through build-time keys; `RDEPEND` /
    // `PDEPEND` / `IDEPEND` keep the wider `reachable` gate.
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    // The post-walk vdb scan stays behind the `reachable` gate for the
    // same-slot arm; the new-slot arm additionally accepts walked
    // consumers, and the `_slot_change_probe` half below is never gated.
    let mut probe_parents = collect_probe_parents(CollectProbeParentsArgs {
        root,
        entries,
        reachable,
        walked: &walked,
        in_graph: &in_graph,
        already,
        with_bdeps,
        excluded,
    });
    // Arguments also veto a provider candidate they do not match.
    add_argument_probe_parents(&mut probe_parents, top_level_atoms);
    // The requested cps the `--update` greedy expansion actually pins.
    let greedy_cps: HashSet<(String, String)> = if update {
        greedy_pinned_cps(top_level_atoms, repos, &installed_by_cp)
    } else {
        HashSet::new()
    };
    // Per-(provider, bound-slot) memo for the revision check below.
    let mut revision_memo: HashMap<((String, String), String), bool> = HashMap::new();
    // The new-slot arm reads available candidates too, so it can fire with
    // both maps empty; pre-check `want_update` here so plain resolves skip
    // the vdb loop as before.
    let want_probe = update || !top_level_cps.is_empty();
    if !new_slot.is_empty() || !new_slot_fresh.is_empty() || want_probe {
        for pkg in &installed {
            let cp = (pkg.category.clone(), pkg.package.clone());
            if undone.contains(&cp) || (in_graph.contains(&cp) && !already.contains(&cp)) {
                continue;
            }
            // Same-slot arm scope (frozen): complete-mode reachability only.
            let same_slot_scope = !new_slot.is_empty() && reachable.contains(&cp);
            // New-slot arm scope: reachability or walked this pass.
            // Merge-bound consumers stay skipped by the `in_graph` rule:
            // their live `:=` re-binds through their own walk.
            let new_slot_scope = reachable.contains(&cp) || walked.contains(&cp);
            if !same_slot_scope && !new_slot_scope {
                continue;
            }
            let consumer_cpv = pkg.cpv();
            // A built package's DEPEND/BDEPEND only count when bdeps is on.
            let dep_keys: &[&str] = match with_bdeps && walked.contains(&cp) {
                true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
                false => &["RDEPEND", "PDEPEND", "IDEPEND"],
            };
            // (display `provider_cpv`, provider cp, fresh candidate
            // `"cat/pkg-ver:slot/sub"`, and whether the edge came from the
            // same-slot arm).
            let mut edges: Vec<(String, (String, String), String, bool)> = dep_keys
                .iter()
                .flat_map(|key| {
                    read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key)
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .filter_map(|tok| portage_dep::parse_atom(&tok))
                .filter_map(|atom| {
                    if atom.slot_operator != Some(portage_dep::SlotOperator::Equals) {
                        return None;
                    }
                    let (a_slot, a_sub) = (atom.slot.as_deref()?, atom.sub_slot.as_deref()?);
                    let provider_cp = (atom.category.clone(), atom.package.clone());
                    if same_slot_scope {
                        let (n_ver, n_slot, n_sub) = new_slot.get(&provider_cp)?;
                        if a_slot == n_slot && a_sub != n_sub {
                            let display = format!("{}/{}-{n_ver}", atom.category, atom.package);
                            let cand = format!(
                                "{}/{}-{n_ver}:{n_slot}/{n_sub}",
                                atom.category, atom.package
                            );
                            return Some((display, provider_cp, cand, true));
                        }
                    }
                    // New-slot arm: an installed consumer whose built `:S/SS=`
                    // dep is bound to a slot the provider no longer occupies
                    // rebuilds against the fresh slot. First acceptable
                    // candidate wins, gates and refusal per candidate.
                    if new_slot_scope {
                        // A greedy request-argument pin on the bound slot
                        // refuses the whole edge; same-slot candidates match
                        // `P:S` trivially. The pin needs a multi-slot request
                        // atom and must survive the greedy revision with
                        // blocker lookahead.
                        if update
                            && greedy_cps.contains(&provider_cp)
                            && let Some((high_slot, high_ver)) =
                                highest_visible_slot(repos, &provider_cp)
                            && high_slot.as_str() != a_slot
                            && installed_by_cp.get(&provider_cp).is_some_and(|insts| {
                                insts
                                    .iter()
                                    .filter(|(_, s)| *s == a_slot)
                                    .map(|(v, _)| v)
                                    .max_by(|a, b| vercmp_ordering(a, b))
                                    .is_some_and(|best| {
                                        vercmp_ordering(&high_ver, best) == Ordering::Greater
                                    })
                            })
                            && *revision_memo
                                .entry((provider_cp.clone(), a_slot.to_string()))
                                .or_insert_with(|| {
                                    greedy_pin_survives_revision(
                                        repos,
                                        config,
                                        &provider_cp,
                                        a_slot,
                                    )
                                })
                        {
                            return None;
                        }
                        let mut cands: Vec<(&String, &String, &String)> = Vec::new();
                        if let Some(fresh) = new_slot_fresh.get(&provider_cp) {
                            cands.push((&fresh.0, &fresh.1, &fresh.2));
                        }
                        if let Some(other) = new_slot.get(&provider_cp) {
                            cands.push((&other.0, &other.1, &other.2));
                        }
                        // An installed provider instance in another slot that an
                        // argument names is a probe candidate the argument
                        // itself accepts.
                        let arg_installed: Vec<(String, String, String)> =
                            installed_refs(root, &provider_cp.0, &provider_cp.1)
                                .into_iter()
                                .filter(|r| r.slot != a_slot)
                                .filter(|r| {
                                    let s = format!(
                                        "{}/{}-{}:{}",
                                        provider_cp.0, provider_cp.1, r.version, r.slot
                                    );
                                    top_level_atoms.iter().any(|t| {
                                        portage_dep::match_from_list(t, &[s.as_str()])
                                            .is_some_and(|m| !m.is_empty())
                                    })
                                })
                                .map(|r| (r.version, r.slot, r.sub_slot))
                                .collect();
                        for (v, sl, sub) in &arg_installed {
                            cands.push((v, sl, sub));
                        }
                        // The probe also ranges over every available package,
                        // not just scheduled entries. Ebuilds only, visible
                        // ones only, highest first; already-installed
                        // versions are not updates.
                        let tree = list_candidates(repos, &provider_cp.0, &provider_cp.1).ok();
                        if let Some(tree) = tree.as_deref() {
                            let installed_versions: HashSet<&String> = installed_by_cp
                                .get(&provider_cp)
                                .map(|insts| insts.iter().map(|(v, _)| v).collect())
                                .unwrap_or_default();
                            let mut avail: Vec<(&String, &String, &String)> = tree
                                .iter()
                                .filter(|c| c.source == CandidateSource::Ebuild)
                                .filter(|c| !installed_versions.contains(&c.version))
                                .filter(|c| {
                                    ebuild_visible_at(
                                        repos,
                                        &provider_cp.0,
                                        &provider_cp.1,
                                        &c.version,
                                        config,
                                    )
                                })
                                .map(|c| (&c.version, &c.slot, &c.sub_slot))
                                .collect();
                            avail.sort_by(|a, b| vercmp_ordering(a.0, b.0).reverse());
                            for a in avail {
                                if !cands.contains(&a) {
                                    cands.push(a);
                                }
                            }
                        }
                        for (c_ver, c_slot, c_sub) in cands {
                            if *c_slot == a_slot {
                                continue;
                            }
                            // An installed provider instance must sit in the
                            // bound slot, else there is nothing to update
                            // away from.
                            let Some(old_ver) = installed_by_cp.get(&provider_cp)?.iter().find_map(
                                |(version, slot)| (*slot == a_slot).then(|| version.clone()),
                            ) else {
                                continue;
                            };
                            // Higher version only; equal versions (a slot move
                            // without a revbump) probe.
                            if vercmp_ordering(c_ver, &old_ver) == Ordering::Less {
                                continue;
                            }
                            // `--update`, or the provider itself is directly
                            // requested.
                            if !update && !top_level_cps.contains(&provider_cp) {
                                continue;
                            }
                            // With no visible tree candidate the forced reinstall
                            // would dead-end.
                            if list_candidates(repos, &pkg.category, &pkg.package)
                                .ok()
                                .is_none_or(|cands| cands.is_empty())
                            {
                                continue;
                            }
                            let display = format!("{}/{}-{c_ver}", atom.category, atom.package);
                            let cand = format!(
                                "{}/{}-{c_ver}:{c_slot}/{c_sub}",
                                atom.category, atom.package
                            );
                            // A replacement parent whose own atom accepts the
                            // new slot must exist.
                            if !replacement_parent_accepts(ReplacementParentAcceptsArgs {
                                repos,
                                config,
                                consumer: (&pkg.category, &pkg.package, &pkg.slot),
                                dep_keys,
                                provider: (&atom.category, &atom.package),
                                candidate: &cand,
                            }) {
                                continue;
                            }
                            if probe_refused(&probe_parents, &scheduled, &provider_cp, &cand, &cp) {
                                continue;
                            }
                            return Some((display, provider_cp, cand, false));
                        }
                        return None;
                    }
                    None
                })
                .collect();
            edges.sort();
            edges.dedup();
            // A fresh candidate that violates another parent's atom refuses
            // the whole replacement. Only the same-slot arm contributes
            // `(provider, consumer)` display pairs; the consumer itself is
            // scheduled on both arms alike.
            let mut survived = false;
            for (display, provider_cp, cand, same_slot) in edges {
                if probe_refused(&probe_parents, &scheduled, &provider_cp, &cand, &cp) {
                    continue;
                }
                survived = true;
                if same_slot {
                    abi_rebuilds.push((display, consumer_cpv.clone()));
                }
            }
            if !survived {
                continue;
            }
            scheduled.insert(cp);
        }
    }

    // The slot-move-without-revbump arm below runs even when the vdb scan
    // above is gated off; see the helper's doc comment.
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root,
        repos,
        entries,
        undone,
        scheduled: &mut scheduled,
        excluded,
        masked: &mut masked,
        config,
    });

    abi_rebuilds.sort();
    abi_rebuilds.dedup();
    (scheduled, abi_rebuilds, masked)
}
