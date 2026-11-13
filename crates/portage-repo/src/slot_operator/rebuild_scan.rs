//! Rebuilds triggered by slot-operator dependencies.

use crate::*;

/// [`slot_operator_rebuild_scan`]'s result: the consumer `(cat, pkg)`
/// replace set, the `(provider_cpv, consumer_cpv)` display pairs, the
/// stale-binary `(category, package, version)` triples, and the stale-pin
/// bindings the scan refused.
pub(crate) struct SlotOpRebuildScan {
    pub(crate) scheduled: BTreeSet<(String, String)>,
    pub(crate) abi_rebuilds: Vec<(String, String)>,
    pub(crate) masked: BTreeSet<BinaryMask>,
    /// Read by tests; the resolver files nothing for a refusal (a refused
    /// consumer is simply not scheduled).
    #[allow(dead_code)]
    pub(crate) refused: Vec<SlotOpRefused>,
}

/// A stale-pin binding the scan does not repair: the kept consumer, the
/// recorded atom that forbids the repair, and the final provider version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotOpRefused {
    pub(crate) consumer: (String, String),
    pub(crate) atom: String,
    pub(crate) provider: Option<String>,
}

/// Arguments of [`slot_operator_rebuild_scan`].
pub(crate) struct SlotOperatorRebuildScanArgs<'a> {
    /// Dynamic-deps view of the installed parents that veto a candidate.
    pub(crate) effective: Option<EffectiveDeps<'a>>,
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
    /// Whether binary packages are eligible: with binaries instead of
    /// builds, stale build-time bindings cannot mis-bind anything.
    pub(crate) usepkg: bool,
    pub(crate) update: bool,
    pub(crate) top_level_cps: &'a HashSet<(String, String)>,
    pub(crate) top_level_atoms: &'a HashSet<String>,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
    /// `--emptytree`: Portage files no slot-operator rebuild there
    /// (`depgraph.py:7917-7925`), so the stale-pin arm stays out.
    pub(crate) empty: bool,
}

/// Post-walk scan for consumers needing a slot-operator rebuild: the same-slot
/// arm (bound sub-slot moved under a replacing provider), the new-slot arm
/// (bound slot abandoned for a fresh one), and the stale-pin arm (a kept
/// consumer whose recorded `:slot/sub-slot=` no longer matches the final
/// provider version, whether or not this run moves the provider). Returns the
/// replace set, the display pairs, the stale-binary triples, and the refused
/// stale-pin bindings.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls
pub(crate) fn slot_operator_rebuild_scan(
    args: SlotOperatorRebuildScanArgs<'_>,
) -> SlotOpRebuildScan {
    let SlotOperatorRebuildScanArgs {
        effective,
        root,
        entries,
        reachable,
        scope,
        walked_installed,
        already,
        undone,
        with_bdeps,
        usepkg,
        update,
        top_level_cps,
        top_level_atoms,
        excluded,
        config,
        gate,
        empty,
    } = args;
    // cp -> (new version, new slot, new sub-slot) for every entry that
    // replaces an installed version in that slot.
    let mut new_slot: HashMap<(String, String), (String, String, String)> = HashMap::new();
    // cp -> (new version, new slot, new sub-slot) for every entry merged
    // into a fresh slot (no installed instance there).
    let mut new_slot_fresh: HashMap<(String, String), (String, String, String)> = HashMap::new();
    // Every provider this run moves to another version: an upgrade or a
    // downgrade entry, or a fresh slot. A same-version reinstall keeps the
    // sub-slot, so it moves nothing (the unsatisfied-oneshot oracle: a
    // re-merged provider alone rebuilds no consumer).
    let mut moved: HashSet<(String, String)> = HashSet::new();
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
            let cp = (e.category.clone(), e.package.clone());
            moved.insert(cp.clone());
            new_slot_fresh.insert(cp, (version.clone(), slot, sub_slot));
        }
        if matches!(
            e.outcome,
            PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
        ) {
            moved.insert((e.category.clone(), e.package.clone()));
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
    let mut refused: Vec<SlotOpRefused> = Vec::new();
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
        effective,
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
            // scheduled (its own walk re-binds the live atom). Its pair is
            // still listed, under the replacement; Portage lists it only if
            // the consumer is also in its force-reinstall set.
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
            // Stale-pin arm scope: under --update every installed consumer
            // the run does not merge, except one the pass already walked
            // outside any scope: its own entry governs, so a walked
            // consumer the pass keeps as-is stays out. Otherwise only one
            // outside the walk (the #744 shape -- the provider move itself
            // is checked per atom, since an unchanged provider still
            // leaves a stale final).
            let in_scope =
                scope.unwrap_or(reachable).contains(&cp) || walked_installed.contains(&cp);
            let stale_scope = if update {
                in_scope || !walked.contains(&cp)
            } else {
                !in_scope
            };
            if !same_slot_scope && !new_slot_scope && !stale_scope {
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
            // the whole replacement. The same-slot arm and the stale-pin arm
            // contribute `(provider, consumer)` display pairs; the consumer
            // itself is scheduled on all arms alike.
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
                    entries,
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
            // display pair only (no schedule, no child filing). The stale-pin
            // arm still files an unmerged consumer no arm above reached: its
            // own walk re-binds nothing. The display pair is re-emitted on
            // every pass (like the same-slot arm's): the fold path files the
            // schedule but drops the pair, so only the settling pass's pair
            // names the parent (`assemble_result` reads it into required_by).
            // Re-emission also covers an already-scheduled consumer its scope
            // no longer reaches (a walked consumer the pass keeps filed but
            // unscoped): scheduling stays scoped, naming does not.
            if !survived || merge_bound_consumer {
                if (stale_scope || scheduled.contains(&cp)) && !merge_bound_consumer {
                    stale_pin_for_consumer(StalePinConsumerArgs {
                        root,
                        entries,
                        reachable,
                        scope,
                        walked_installed,
                        update,
                        config,
                        gate,
                        new_slot: &new_slot,
                        new_slot_fresh: &new_slot_fresh,
                        moved: &moved,
                        installed_by_cp: &installed_by_cp,
                        probe_parents: &probe_parents,
                        scheduled: &mut scheduled,
                        abi_rebuilds: &mut abi_rebuilds,
                        refused: &mut refused,
                        pkg,
                        walked: &walked,
                        with_bdeps,
                        usepkg,
                        cp: &cp,
                        empty,
                    });
                }
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
    let mut masked: BTreeSet<BinaryMask> = BTreeSet::new();
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
    SlotOpRebuildScan {
        scheduled,
        abi_rebuilds,
        masked,
        refused,
    }
}

/// Arguments of [`stale_pin_for_consumer`].
struct StalePinConsumerArgs<'a> {
    root: &'a Path,
    entries: &'a [GraphEntry],
    reachable: &'a HashSet<(String, String)>,
    scope: Option<&'a HashSet<(String, String)>>,
    walked_installed: &'a HashSet<(String, String)>,
    update: bool,
    config: &'a portage_profile::Config,
    gate: CategoryGate<'a>,
    new_slot: &'a HashMap<(String, String), (String, String, String)>,
    new_slot_fresh: &'a HashMap<(String, String), (String, String, String)>,
    moved: &'a HashSet<(String, String)>,
    installed_by_cp: &'a HashMap<(String, String), Vec<(String, String)>>,
    probe_parents: &'a HashMap<(String, String), Vec<ProbeParent>>,
    scheduled: &'a mut BTreeSet<(String, String)>,
    abi_rebuilds: &'a mut Vec<(String, String)>,
    refused: &'a mut Vec<SlotOpRefused>,
    pkg: &'a InstalledPackage,
    walked: &'a HashSet<(String, String)>,
    with_bdeps: bool,
    usepkg: bool,
    cp: &'a (String, String),
    /// `--emptytree`: Portage files no slot-operator rebuild there
    /// (`depgraph.py:7917-7925`), so the stale-pin arm stays out.
    empty: bool,
}

/// The stale-pin arm for one installed consumer: a kept consumer whose
/// recorded `:slot/sub-slot=` no longer matches the provider version the run
/// leaves behind is scheduled for a rebuild. The final version is the move
/// this run makes, else the installed one, so an already-stale binding fires
/// without any provider entry. Without `--update` only a consumer outside the
/// walk whose provider moves fires (the #744 shape).
///
/// Gates: a visible replacement for the consumer; the §2.3 hold-back
/// check on the recorded atoms (refused by name); then per stale provider --
/// the consumer's own atoms accept that provider's final version, some
/// replacement parent names and accepts it, the tree still names it, and no
/// veto parent refuses it. The consumer is scheduled when any pin survives;
/// each surviving pin earns its own display pair. A consumer with no visible
/// tree candidate, or one every gate skips, stays out silently; a recorded
/// versioned pin that forbids the final version is refused by name.
fn stale_pin_for_consumer(args: StalePinConsumerArgs<'_>) {
    let StalePinConsumerArgs {
        root,
        entries,
        reachable,
        scope,
        walked_installed,
        update,
        config,
        gate,
        new_slot,
        new_slot_fresh,
        moved,
        installed_by_cp,
        probe_parents,
        scheduled,
        abi_rebuilds,
        refused,
        pkg,
        walked,
        with_bdeps,
        usepkg,
        cp,
        empty,
    } = args;
    // `--emptytree` files no slot-operator rebuild: every reached slot
    // merges anyway, and Portage has no installed parent or child to file
    // (`assemble_result` drops the pairs there by the same rule).
    if empty {
        return;
    }
    // Without --update the consumer must sit outside the walk to be
    // scheduled; re-emitting the pair for an already-scheduled consumer is
    // scope-free (scheduling stays scoped, naming does not).
    if !update
        && (walked_installed.contains(cp) || scope.unwrap_or(reachable).contains(cp))
        && !scheduled.contains(cp)
    {
        return;
    }
    let use_flags = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
    // The selected recorded atoms per key: the arm the merge would take, as
    // the replacement gates read them. Runtime keys always; build-time keys
    // for a walked consumer when build-time deps are on or a source build
    // can happen anyway. Portage registers a built parent's slot-operator
    // dep only when the walk reaches it non-optionally (an unreached
    // consumer's build-time pin never fires -- the oneshot-argument half of
    // the with-bdeps oracle shape); `--usepkg` alone, with binaries instead
    // of source builds, keeps the arm to runtime keys everywhere (the
    // `--usepkg` half). On this host `-uDN --getbinpkg=n` builds from
    // source, so walked BDEPEND-only consumers (the 23 `go` holders) do
    // fire; under `--usepkg` they stay out.
    let mut stale_keys: Vec<&str> = vec!["RDEPEND", "PDEPEND", "IDEPEND"];
    if walked.contains(cp) && (with_bdeps || !usepkg) {
        stale_keys.extend(["DEPEND", "BDEPEND"]);
    }
    let mut keyed: Vec<Vec<String>> = Vec::new();
    for key in &stale_keys {
        let text = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
        keyed.push(selected_parent_atoms(&text, root, entries, &use_flags));
    }
    // Every stale pin: a recorded `=`-operated atom whose slot the final
    // version keeps with another sub-slot. A moved slot stays with the
    // new-slot arm. Every stale provider is collected: a two-provider
    // consumer is scheduled when any of its pins survives the gates below.
    let mut edges: Vec<((String, String), String, String, String)> = Vec::new();
    for toks in &keyed {
        for tok in toks {
            let Some(atom) = parse_dep_atom(tok, "rebuild_scan_stale") else {
                continue;
            };
            if atom.blocker != portage_dep::Blocker::None
                || atom.slot_operator != Some(portage_dep::SlotOperator::Equals)
            {
                continue;
            }
            let (Some(slot), Some(sub)) = (atom.slot.as_deref(), atom.sub_slot.as_deref()) else {
                continue;
            };
            if sub == "*" {
                continue;
            }
            let provider_cp = (atom.category.clone(), atom.package.clone());
            let Some((final_version, final_slot, final_sub)) = new_slot
                .get(&provider_cp)
                .or_else(|| new_slot_fresh.get(&provider_cp))
                .cloned()
                .or_else(|| stale_installed_final(installed_by_cp, root, &provider_cp, slot))
            else {
                continue;
            };
            if final_slot != slot || final_sub == sub {
                continue;
            }
            // Without --update the provider must move this run: a
            // same-version reinstall keeps the sub-slot, so it moves
            // nothing (the unsatisfied-oneshot oracle).
            if !update && !moved.contains(&provider_cp) {
                continue;
            }
            let cand = format!(
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                "{}/{}-{final_version}:{final_slot}/{final_sub}",
                atom.category, atom.package
            );
            if !edges.iter().any(|(cp, _, _, _)| cp == &provider_cp) {
                edges.push((provider_cp, cand, final_version, final_slot));
            }
        }
    }
    if edges.is_empty() {
        return;
    }
    // A visible replacement for the consumer.
    if gate
        .repo_candidates(&pkg.category, &pkg.package)
        .ok()
        .is_none_or(|cands| cands.is_empty())
    {
        return;
    }
    // No rebuild may hold a package back: every recorded non-`:=` atom
    // naming a provider this run moves must match the final version there.
    // A `:=` trigger is judged by the sub-slot compare only: the matcher
    // enforces recorded-sub equality, so it would refuse every stale binding.
    for toks in &keyed {
        for tok in toks {
            let Some(atom) = parse_dep_atom(tok, "rebuild_scan_holdback") else {
                continue;
            };
            if atom.blocker != portage_dep::Blocker::None
                || atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
            {
                continue;
            }
            let atom_cp = (atom.category.clone(), atom.package.clone());
            if !(new_slot.contains_key(&atom_cp) || new_slot_fresh.contains_key(&atom_cp)) {
                continue;
            }
            let (final_version, final_slot, final_sub) = new_slot
                .get(&atom_cp)
                .or_else(|| new_slot_fresh.get(&atom_cp))
                .expect("provider moves this run");
            let final_cand = format!(
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
                "{}/{}-{final_version}:{final_slot}/{final_sub}",
                atom.category, atom.package
            );
            let stripped = portage_dep::without_use(tok).to_string();
            if portage_dep::match_from_list(&stripped, &[final_cand.as_str()])
                .is_none_or(|m| m.is_empty())
            {
                refused.push(SlotOpRefused {
                    consumer: cp.clone(),
                    atom: (*tok).clone(),
                    provider: Some(format!(
                        "{}/{}-{final_version}",
                        atom.category, atom.package
                    )),
                });
                return;
            }
        }
    }
    // Per stale provider: the replacement's own atoms accept the final
    // version, some replacement parent names the provider and accepts it,
    // and the consumer's tree still names the provider (a rebuild
    // re-records the live dependencies, so a tree that dropped the dep has
    // nothing stale to repair). The consumer is scheduled when any pin
    // survives; each surviving pin earns its display pair below.
    let mut surviving: Vec<(&(String, String), &str, &str, &str)> = Vec::new();
    for (provider_cp, cand, final_version, final_slot) in &edges {
        if !replacement_accepts(ReplacementAcceptsArgs {
            root,
            config,
            pkg,
            dep_keys: &stale_keys,
            provider_cp,
            cand,
            gate,
            entries,
        }) {
            continue;
        }
        if !replacement_parent_accepts(ReplacementParentAcceptsArgs {
            config,
            consumer: (&pkg.category, &pkg.package, &pkg.slot),
            dep_keys: &stale_keys,
            provider: (&provider_cp.0, &provider_cp.1),
            candidate: cand,
            gate,
        }) {
            continue;
        }
        // A rebuild re-records the consumer's live dependencies, so a tree
        // that no longer names the provider has nothing stale to repair:
        // the consumer stays out (its recorded pin dies with the dependency
        // itself, and Portage's probe never fires for it either).
        if !replacement_names_provider(ReplacementNamesProviderArgs {
            gate,
            config,
            root,
            entries,
            consumer: (&pkg.category, &pkg.package, &pkg.slot),
            dep_keys: &stale_keys,
            provider_cp,
        }) {
            continue;
        }
        if probe_refused(probe_parents, scheduled, provider_cp, cand, cp) {
            continue;
        }
        surviving.push((
            provider_cp,
            cand.as_str(),
            final_version.as_str(),
            final_slot.as_str(),
        ));
    }
    if surviving.is_empty() {
        return;
    }
    // The display pair names the provider whose move caused the rebuild,
    // like the same-slot arm: without it the filed entry joins the walk as
    // a parentless root (`assemble_result` names it from the pair). Only a
    // genuine version change gets one: for an already-stale binding the
    // provider stays put, and Portage prints no block for that shape (the
    // unsatisfied-oneshot oracle pins the bare row). The consumer side names
    // the version the plan installs for it (the entry's, when the settling
    // pass already filed it -- version selection may upgrade a forced
    // reinstall past the installed version), else the installed one. Pairs
    // are re-emitted on every pass even for an already-scheduled consumer
    // (only the settling pass's pairs survive to the plan, since the fold
    // path files the schedule but drops the pair).
    let consumer_cpv = entries
        .iter()
        .find(|e| {
            e.category == cp.0 && e.package == cp.1 && e.slot.as_deref() == Some(pkg.slot.as_str())
        })
        .and_then(|e| merge_bound_version(&e.outcome))
        .map_or_else(
            || pkg.cpv(),
            |version| format!("{}/{}-{version}", cp.0, cp.1),
        );
    for (provider_cp, _cand, final_version, final_slot) in &surviving {
        let installed_version = installed_by_cp.get(*provider_cp).and_then(|insts| {
            insts
                .iter()
                .find(|(_, s)| s == final_slot)
                .map(|(v, _)| v.clone())
        });
        if installed_version.as_deref() != Some(*final_version) {
            let display = format!("{}/{}-{final_version}", provider_cp.0, provider_cp.1);
            abi_rebuilds.push((display, consumer_cpv.clone()));
        }
    }
    scheduled.insert(cp.clone());
}

/// Whether any replacement ebuild for the consumer names the provider in an
/// atom its *current* effective USE selects. A rebuild re-records the live
/// dependencies, so a tree that no longer names the provider has nothing
/// stale to repair; Portage's probe never fires for such a consumer either
/// (it pairs only a provider that is a child of the replacement, so the
/// #254 USE-conditional-off shape keeps its unsatisfied-probe rebuild but
/// prints no block).
fn replacement_names_provider(args: ReplacementNamesProviderArgs<'_>) -> bool {
    let ReplacementNamesProviderArgs {
        gate,
        config,
        root,
        entries,
        consumer,
        dep_keys,
        provider_cp,
    } = args;
    let (category, package, slot) = consumer;
    let Ok(cands) = gate.repo_candidates(category, package) else {
        return false;
    };
    cands
        .iter()
        .filter(|c| c.source == CandidateSource::Ebuild && c.slot == slot)
        .any(|c| {
            let Ok(meta) = repo_aux_metadata(
                &c.repo_location,
                category,
                &format!("{package}-{}", c.version),
            ) else {
                return false;
            };
            let cand_str = c.match_str(category, package);
            let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
                config,
                iuse: meta.get("IUSE").map(String::as_str).unwrap_or_default(),
                keywords: &c.keywords,
                candidate_str: &cand_str,
                build_id: c.build_id.as_deref(),
                category,
                package,
            });
            dep_keys
                .iter()
                .flat_map(|key| {
                    selected_parent_atoms(
                        meta.get(*key).map(String::as_str).unwrap_or(""),
                        root,
                        entries,
                        &use_flags,
                    )
                })
                .any(|tok| {
                    parse_dep_atom(&tok, "rebuild_scan_names").is_some_and(|a| {
                        a.blocker == portage_dep::Blocker::None
                            && a.category == provider_cp.0
                            && a.package == provider_cp.1
                    })
                })
        })
}

/// Arguments of [`replacement_names_provider`].
struct ReplacementNamesProviderArgs<'a> {
    gate: CategoryGate<'a>,
    config: &'a portage_profile::Config,
    root: &'a Path,
    entries: &'a [GraphEntry],
    consumer: (&'a str, &'a str, &'a str),
    dep_keys: &'a [&'a str],
    provider_cp: &'a (String, String),
}

/// The final provider version a stale pin binds against when the run makes
/// no move for it: the installed instance in `slot`. `None` when the
/// provider has no instance there.
fn stale_installed_final(
    installed_by_cp: &HashMap<(String, String), Vec<(String, String)>>,
    root: &Path,
    provider_cp: &(String, String),
    slot: &str,
) -> Option<(String, String, String)> {
    let old = installed_by_cp
        .get(provider_cp)?
        .iter()
        .find(|(_, s)| s == slot)?;
    let (_, sub) = read_vdb_slot(root, &provider_cp.0, &provider_cp.1, &old.0);
    Some((old.0.clone(), slot.to_string(), sub))
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
