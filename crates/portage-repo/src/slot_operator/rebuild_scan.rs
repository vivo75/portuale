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
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    /// Narrower consumer scope (`ResolveCtx::scan_reachable`); `None` is `reachable`.
    /// Veto parents keep the full `reachable` closure.
    pub(crate) scope: Option<&'a HashSet<(String, String)>>,
    /// Installed consumers this request's own walk recursed into (`--deep`);
    /// the same-slot arm schedules them outside `scope` too.
    pub(crate) walked_installed: &'a HashSet<(String, String)>,
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) undone: &'a BTreeSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) top_level_atoms: &'a HashSet<String>,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
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
        entries,
        reachable,
        scope,
        walked_installed,
        already,
        undone,
        with_bdeps,
        update,
        top_level_cps,
        top_level_atoms,
        excluded,
        config,
        gate,
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
    for p in installed.iter() {
        installed_by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push((p.version.clone(), p.slot.clone()));
    }
    // cp -> the version a genuine version move installs this run (the first
    // entry wins, like `merge_bound_index`). A same-version `Reinstall` is the
    // rebuild the scan itself schedules, not a replacement: Portage's
    // `_eliminate_rebuilds` drops the `--changed-slot` shape's edge entirely.
    let mut merge_bound: HashMap<(String, String), String> = HashMap::new();
    for e in entries {
        if matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) && let Some(version) = merge_bound_version(&e.outcome)
        {
            merge_bound
                .entry((e.category.clone(), e.package.clone()))
                .or_insert_with(|| version.clone());
        }
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
        greedy_pinned_cps(gate, top_level_atoms, &installed_by_cp)
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
        for pkg in installed.iter() {
            let cp = (pkg.category.clone(), pkg.package.clone());
            if undone.contains(&cp) {
                continue;
            }
            // A consumer merge-bound for an upgrade in this same run is not
            // scheduled (its own walk re-binds the live atom), but Portage
            // still lists it under its replacement in the rebuild block.
            // Portage: depgraph.py, _compute_abi_rebuild_info (`:931-1145`)
            let merge_bound_consumer = merge_bound.contains_key(&cp) && !already.contains(&cp);
            // Any other merge-bound consumer (its own reinstall: `--changed-slot`
            // and friends) keeps the old skip: its rebuild is its own row and
            // `_eliminate_rebuilds` drops the edge.
            if !merge_bound_consumer && in_graph.contains(&cp) && !already.contains(&cp) {
                continue;
            }
            // Same-slot arm scope: reachability, or an installed consumer the walk recursed into.
            // Portage: depgraph.py, _add_pkg (recurse under --deep)
            let same_slot_scope = !new_slot.is_empty()
                && (scope.unwrap_or(reachable).contains(&cp) || walked_installed.contains(&cp));
            // New-slot arm scope: reachability or walked this pass.
            let new_slot_scope = scope.unwrap_or(reachable).contains(&cp) || walked.contains(&cp);
            if !same_slot_scope && !new_slot_scope {
                continue;
            }
            // The block names the merge-bound replacement, not the installed
            // version the recorded atom lives in -- also when that consumer is
            // already in the replace set (a merge-bound cp can be both; the
            // display still names the replacement).
            // Portage: depgraph.py, _compute_abi_rebuild_info (`:931-1145`)
            let consumer_cpv = merge_bound.get(&cp).map_or_else(
                || pkg.cpv(),
                |version| format!("{}/{}-{version}", pkg.category, pkg.package),
            );
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
                .filter_map(|tok| parse_dep_atom(&tok, "rebuild_scan_tokens"))
                .filter_map(|atom| {
                    slot_op_edge_for_atom(
                        atom,
                        SlotOpEdgeCtx {
                            same_slot_scope,
                            new_slot_scope,
                            new_slot: &new_slot,
                            new_slot_fresh: &new_slot_fresh,
                            update,
                            greedy_cps: &greedy_cps,
                            installed_by_cp: &installed_by_cp,
                            revision_memo: &mut revision_memo,
                            config,
                            root,
                            top_level_atoms,
                            top_level_cps,
                            pkg,
                            dep_keys,
                            probe_parents: &probe_parents,
                            scheduled: &scheduled,
                            cp: &cp,
                            gate,
                        },
                    )
                })
                .collect();
            edges.sort();
            edges.dedup();
            // A fresh candidate that violates another parent's atom refuses
            // the whole replacement. Only the same-slot arm contributes
            // `(provider, consumer)` display pairs; the consumer itself is
            // scheduled on both arms alike.
            let mut survived = false;
            let mut provider_joins: Vec<(String, String)> = Vec::new();
            for (display, provider_cp, cand, same_slot) in edges {
                if probe_refused(&probe_parents, &scheduled, &provider_cp, &cand, &cp) {
                    continue;
                }
                // The replacement consumer's own atoms must accept the
                // candidate, or the probe finds no new child for it.
                // Portage: depgraph.py, _slot_operator_update_probe (replacement_parent atoms)
                if !replacement_accepts(ReplacementAcceptsArgs {
                    root,
                    config,
                    pkg,
                    dep_keys,
                    provider_cp: &provider_cp,
                    cand: &cand,
                    gate,
                }) {
                    continue;
                }
                survived = true;
                if same_slot {
                    abi_rebuilds.push((display, consumer_cpv.clone()));
                    provider_joins.push(provider_cp);
                }
            }
            // A merge-bound consumer is already merging, so it contributes the
            // display pair only (no schedule, no child filing).
            if !survived || merge_bound_consumer {
                continue;
            }
            // The installed child joins the replace set with its parent.
            // Portage: depgraph.py, _slot_operator_update_backtrack (`child.installed`)
            scheduled.extend(provider_joins);
            scheduled.insert(cp);
        }
    }

    // The slot-move-without-revbump arm below runs even when the vdb scan
    // above is gated off; see the helper's doc comment.
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root,
        entries,
        undone,
        scheduled: &mut scheduled,
        excluded,
        masked: &mut masked,
        config,
        gate,
    });

    abi_rebuilds.sort();
    abi_rebuilds.dedup();
    (scheduled, abi_rebuilds, masked)
}

/// Borrowed inputs of [`slot_op_edge_for_atom`].
struct SlotOpEdgeCtx<'a> {
    same_slot_scope: bool,
    new_slot_scope: bool,
    new_slot: &'a HashMap<(String, String), (String, String, String)>,
    new_slot_fresh: &'a HashMap<(String, String), (String, String, String)>,
    update: bool,
    greedy_cps: &'a HashSet<(String, String)>,
    installed_by_cp: &'a HashMap<(String, String), Vec<(String, String)>>,
    revision_memo: &'a mut HashMap<((String, String), String), bool>,
    config: &'a portage_profile::Config,
    root: &'a Path,
    top_level_atoms: &'a HashSet<String>,
    top_level_cps: &'a HashSet<(String, String)>,
    pkg: &'a InstalledPackage,
    dep_keys: &'a [&'a str],
    probe_parents: &'a HashMap<(String, String), Vec<ProbeParent>>,
    scheduled: &'a BTreeSet<(String, String)>,
    cp: &'a (String, String),
    gate: CategoryGate<'a>,
}

/// The slot-operator edge one dependency atom of an installed consumer gives:
/// (display `provider_cpv`, provider cp, fresh candidate, same-slot arm).
fn slot_op_edge_for_atom(
    atom: portage_dep::Atom,
    ctx: SlotOpEdgeCtx<'_>,
) -> Option<(String, (String, String), String, bool)> {
    let SlotOpEdgeCtx {
        same_slot_scope,
        new_slot_scope,
        new_slot,
        new_slot_fresh,
        update,
        greedy_cps,
        installed_by_cp,
        revision_memo,
        config,
        root,
        top_level_atoms,
        top_level_cps,
        pkg,
        dep_keys,
        probe_parents,
        scheduled,
        cp,
        gate,
    } = ctx;
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
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
            && let Some((high_slot, high_ver)) = highest_visible_slot(gate, &provider_cp)
            && high_slot.as_str() != a_slot
            && installed_by_cp.get(&provider_cp).is_some_and(|insts| {
                insts
                    .iter()
                    .filter(|(_, s)| *s == a_slot)
                    .map(|(v, _)| v)
                    .max_by(|a, b| vercmp_ordering(a, b))
                    .is_some_and(|best| vercmp_ordering(&high_ver, best) == Ordering::Greater)
            })
            && *revision_memo
                .entry((provider_cp.clone(), a_slot.to_string()))
                .or_insert_with(|| greedy_pin_survives_revision(gate, config, &provider_cp, a_slot))
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
                        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
        let tree = gate.repo_candidates(&provider_cp.0, &provider_cp.1).ok();
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
                    ebuild_visible_at(gate, &provider_cp.0, &provider_cp.1, &c.version, config)
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
            let Some(old_ver) = installed_by_cp
                .get(&provider_cp)?
                .iter()
                .find_map(|(version, slot)| (*slot == a_slot).then(|| version.clone()))
            else {
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
            if gate
                .repo_candidates(&pkg.category, &pkg.package)
                .ok()
                .is_none_or(|cands| cands.is_empty())
            {
                continue;
            }
            let display = format!("{}/{}-{c_ver}", atom.category, atom.package);
            let cand = format!(
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                "{}/{}-{c_ver}:{c_slot}/{c_sub}",
                atom.category, atom.package
            );
            // A replacement parent whose own atom accepts the
            // new slot must exist.
            if !replacement_parent_accepts(ReplacementParentAcceptsArgs {
                gate,
                config,
                consumer: (&pkg.category, &pkg.package, &pkg.slot),
                dep_keys,
                provider: (&atom.category, &atom.package),
                candidate: &cand,
            }) {
                continue;
            }
            if probe_refused(probe_parents, scheduled, &provider_cp, &cand, cp) {
                continue;
            }
            return Some((display, provider_cp, cand, false));
        }
        return None;
    }
    None
}
