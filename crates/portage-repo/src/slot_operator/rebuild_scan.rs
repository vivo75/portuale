//! Rebuilds triggered by slot-operator dependencies.

use crate::*;

/// [`slot_operator_rebuild_scan`]'s result: the consumer `(cat, pkg)`
/// replace set (real `_slot_operator_replace_installed`), the
/// `(provider_cpv, consumer_cpv)` display pairs (real `_forced_rebuilds`,
/// what `_show_abi_rebuild_info` renders), and the stale-binary
/// `(category, package, version)` triples for `collect_feedback`'s
/// `slot_operator_mask_built` records (real `_slot_change_backtrack`,
/// backlog #212 v2 `#24c`).
pub(crate) type SlotOpRebuildScan = (
    BTreeSet<(String, String)>,
    Vec<(String, String)>,
    BTreeSet<(String, String, String)>,
);

// Ten parameters like its siblings below; the arity is the port, not
// accident (`#[allow]` matches `slot_operator_rebuild_entries` etc.).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_rebuild_scan(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    reachable: &HashSet<(String, String)>,
    already: &BTreeSet<(String, String)>,
    undone: &BTreeSet<(String, String)>,
    with_bdeps: bool,
    update: bool,
    top_level_cps: &HashSet<(String, String)>,
    top_level_atoms: &HashSet<String>,
    excluded: &[String],
    config: &portage_profile::Config,
) -> SlotOpRebuildScan {
    // cp -> (new version, new slot, new sub-slot) for every entry that
    // replaces an installed version in that slot.
    let mut new_slot: HashMap<(String, String), (String, String, String)> = HashMap::new();
    // cp -> (new version, new slot, new sub-slot) for every entry merged
    // into a fresh slot (no installed instance in that slot -- real's
    // `_get_installed_best` empty-`myinslotlist` case, rendered `[ebuild N]`).
    // A provider that moves slot only ever arrives this way; the update
    // probe's new-child-slot arm (`:3121-3126`) prefers these, falling
    // back to `Upgrade`/`Downgrade`/`Reinstall` entries in a slot the
    // consumer is not bound to (same real candidate loop).
    let mut new_slot_fresh: HashMap<(String, String), (String, String, String)> = HashMap::new();
    let mut in_graph: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        // #72 B3: a removal installs nothing, so it is not "in graph" for
        // the slot-operator rebuild scan (real only rebuilds consumers of
        // a package this run actually leaves at a new slot).
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
            // Backlog #303: under `--update`, a non-requested provider the
            // pass *downgraded* was held back by an installed consumer's
            // built `:=` pin (the pass picked the older sub-slot the
            // consumer is bound to). Real's first pass does the same, then
            // `_slot_operator_update_probe` (`depgraph.py:3131`) finds the
            // installed instance as the missed update and backtracks to
            // replace the consumer. Model the missed update by taking the
            // slot/sub-slot of the version the downgrade replaces, so the
            // same-slot arm below flags the consumer and the pin lifts.
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
    // cp -> every installed (version, main slot): the update probe's
    // superseded child (`dep.child`) is the installed instance in the
    // consumer's bound slot, looked up per built atom below.
    let mut installed_by_cp: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
    for p in &installed {
        installed_by_cp
            .entry((p.category.clone(), p.package.clone()))
            .or_default()
            .push((p.version.clone(), p.slot.clone()));
    }
    let mut scheduled: BTreeSet<(String, String)> = already.clone();
    let mut abi_rebuilds: Vec<(String, String)> = Vec::new();
    // #65 S2: every cp this pass actually walked (any outcome, installed
    // nodes included). Real only registers a slot-operator dep for a
    // *built* parent's `DEPEND`/`BDEPEND` when the walk reaches it
    // non-optionally (`_add_pkg_deps`' `optional=pkg.built`,
    // `depgraph.py:4277/4287` combined with `dep.want_update`,
    // `:3802-3806`), so a consumer that complete mode merely *reaches*
    // (`slot_op_reachable`) never rebuilds through a build-time key. The
    // world shape still qualifies: there the consumers are walked nodes
    // (complete-mode seeds), not reachability-only. `RDEPEND`/`PDEPEND`/
    // `IDEPEND` keep the wider `reachable` gate.
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    // #24 S5: the post-walk vdb scan stays behind the `reachable` gate
    // (complete mode) for the same-slot arm. The #211 new-slot arm
    // additionally accepts walked consumers: real registers slot-operator
    // deps for every walked parent (`_add_pkg_deps`), and the update
    // probe's own gate is `want_update`, not reachability -- an empty
    // world (the conflict-mass shape) leaves `reachable` empty while the
    // installed consumers sit in the graph as walked nodes.
    // The `_slot_change_probe` half below is *not* complete-mode gated in
    // real (its first trigger arm runs for any merge-bound parent,
    // 3103-3107), so it is called unconditionally.
    // #211 R2: the update-probe refusal parents (real
    // `_slot_operator_check_reverse_dependencies`, `depgraph.py:2472`).
    // Collected once per scan; the per-edge verdict reads the replace
    // set as it grows (see `probe_refused`).
    let mut probe_parents = collect_probe_parents(
        root, entries, reachable, &walked, &in_graph, already, with_bdeps, excluded,
    );
    // #288: real records the arguments as parents too (`_add_parent_atom`),
    // so an argument pin vetoes a provider candidate it does not match.
    add_argument_probe_parents(&mut probe_parents, top_level_atoms);
    // #268 cut (a): the requested cps real's `--update` greedy expansion
    // actually pins. Collected once per scan; the `#252` gate below reads
    // it per edge.
    let greedy_cps: HashSet<(String, String)> = if update {
        greedy_pinned_cps(top_level_atoms, repos, &installed_by_cp)
    } else {
        HashSet::new()
    };
    // #268 cut (b): per-(provider, bound-slot) memo for the revision
    // check below (tree metadata + effective-USE walks, so not
    // recomputed per consumer edge).
    let mut revision_memo: HashMap<((String, String), String), bool> = HashMap::new();
    // #269: real's update probe ranges over every *available* package
    // (`_iter_similar_available`, `depgraph.py:3015`), not just the
    // scheduled entries, so the new-slot arm below can fire with both
    // maps empty (the `mmcons` shape: no pass schedules the provider,
    // yet the probe finds it among the available packages). The arm
    // itself still needs `want_update` (the per-candidate gate below),
    // which is exactly `update || provider requested` -- pre-check it
    // here so plain resolves skip the vdb loop as before.
    let want_probe = update || !top_level_cps.is_empty();
    if !new_slot.is_empty() || !new_slot_fresh.is_empty() || want_probe {
        for pkg in &installed {
            let cp = (pkg.category.clone(), pkg.package.clone());
            if undone.contains(&cp) || (in_graph.contains(&cp) && !already.contains(&cp)) {
                continue;
            }
            // Same-slot arm scope (frozen): complete-mode reachability only.
            let same_slot_scope = !new_slot.is_empty() && reachable.contains(&cp);
            // New-slot arm scope (#211): reachability or walked this pass.
            // Merge-bound consumers stay skipped by the `in_graph` rule
            // above -- their live `:=` re-binds through their own walk and
            // their vdb is rewritten at merge, so no reinstall is needed
            // (real's not-installed-parent disjunct would probe them; the
            // refusal half of that probe is v2 `#24b` remainder either way).
            // #269: the arm reads available candidates too (see below), so
            // unlike the same-slot arm it needs no scheduled entry -- the
            // outer `want_probe` gate already covers that.
            let new_slot_scope = reachable.contains(&cp) || walked.contains(&cp);
            if !same_slot_scope && !new_slot_scope {
                continue;
            }
            let consumer_cpv = pkg.cpv();
            // #65 S1/S2: real empties a built package's DEPEND/BDEPEND
            // when bdeps is not y/auto, and only the consumers the walk
            // reached non-optionally can fire through them.
            let dep_keys: &[&str] = match with_bdeps && walked.contains(&cp) {
                true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
                false => &["RDEPEND", "PDEPEND", "IDEPEND"],
            };
            // (display `provider_cpv`, provider cp, fresh candidate
            // `"cat/pkg-ver:slot/sub"` for the refusal check, and whether
            // the edge came from the same-slot arm -- #269: only that arm
            // contributes display pairs).
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
                    // #211 (v2 `#24b`): real `_slot_operator_update_probe`
                    // with `new_child_slot=True` (`depgraph.py:3121-3126`,
                    // bug 486580): an installed consumer whose built
                    // `:S/SS=` dep is bound to a slot the provider no
                    // longer occupies rebuilds against the fresh slot.
                    // Candidates are the fresh (`New`) entries first, then
                    // `Upgrade`/`Downgrade`/`Reinstall` entries in a slot
                    // the consumer is not bound to -- but only while no
                    // greedy request-argument pin covers the bound slot
                    // (the `#252` gate below): real's candidate loop
                    // (`_iter_similar_available`, `:2660-2695`) ranges
                    // over every available package, yet the reverse-deps
                    // gate refuses each off-slot candidate against the
                    // `P:S` arg, so a bound-slot mismatch on a requested
                    // provider's upgrade probes nothing (entries at the
                    // bound slot stay with the same-slot arm above).
                    // First acceptable candidate wins, gates and R2 refusal
                    // per candidate.
                    if new_slot_scope {
                        // #252: real's greedy request-argument pin. Under
                        // `--update` real expands a directly-requested cp
                        // into per-slot arg atoms (`_select_files`,
                        // `depgraph.py:5380`, via `_greedy_slots`, `:5896`):
                        // every installed slot below the highest visible
                        // slot gains a `cat/pkg:S` arg recorded on the
                        // installed instance in S (`_add_parent_atom`,
                        // `:4136`). The update probe's reverse-deps gate
                        // (`_slot_operator_check_reverse_dependencies`,
                        // `:2472-2538`) checks the fresh candidate against
                        // those too, and a non-`Package` parent goes
                        // straight to the atom match (`:2487-2541`) -- so
                        // every candidate in another slot is refused, the
                        // probe returns None, and the replace set stays
                        // empty. Probed on real 3.0.82.2 (host
                        // staged-fixture probe 2026-09-29, plus a
                        // vendored-`ResolverPlayground` spy run): on the
                        // `mmprov` shape (`mmprov` requested, consumer
                        // bound `0/1=`, upgrade `1/1 -> 1/2`) both
                        // `new_child_slot` forms return None -- the
                        // `mmprov:0` arg kills the slot-1 candidates --
                        // and only `mmprov-3` merges. Without the pin
                        // (provider not requested, bound to the highest
                        // slot, or no `--update`) the arm fires as before.
                        //
                        // The pin is recorded per installed instance, but
                        // this edge already carries its bound slot
                        // (`a_slot`), and every candidate this arm keeps
                        // sits in another slot -- so a pin on `a_slot`
                        // refuses the whole edge. Same-slot candidates
                        // match `P:S` trivially, so the same-slot arm
                        // above needs no gate.
                        //
                        // #268: both cuts are now modelled. (a) The pin
                        // only exists when the request atom spans slots
                        // (`greedy_cps` -- real `_greedy_slots`' own
                        // single-slot emptiness): an explicit slot or
                        // exact-version request pins nothing, so the arm
                        // fires. (b) The pin must survive the greedy
                        // revision with blocker lookahead
                        // (`greedy_pin_survives_revision`): a discarded
                        // pin refuses nothing, so the arm fires and the
                        // old slot is uninstalled instead of updated.
                        // Still cut: the pairwise lower-version discard
                        // among two or more greedy packages (needs three
                        // installed slots).
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
                        // #288: an installed provider instance in another
                        // slot that an argument names is a graph package
                        // (real selects it for the argument), hence a probe
                        // candidate the argument itself accepts: with
                        // `=mmprov-2` the rebuild proceeds against it, while
                        // `=mmprov-1` (the bound slot's own instance) leaves
                        // no other-slot candidate the argument accepts.
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
                        // #269: real's probe ranges over every *available*
                        // package (`_iter_similar_available`,
                        // `depgraph.py:3015-3057`), not just the scheduled
                        // entries above: a provider upgrade nobody pulled
                        // in (the `mmcons` shape) still rebuilds the
                        // consumer. Ebuilds only: a binary candidate has
                        // no ebuild-visibility verdict at this layer (cut,
                        // same direction as the old miss). Visible ones
                        // only (real's `_pkg_visibility_check`), highest
                        // first like the walk's own selection; versions
                        // already installed are not updates (real's
                        // `pkg.installed` skip).
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
                            // The superseded child: an installed provider
                            // instance must sit in the bound slot (else
                            // there is no `dep.child` to update away from).
                            let Some(old_ver) = installed_by_cp.get(&provider_cp)?.iter().find_map(
                                |(version, slot)| (*slot == a_slot).then(|| version.clone()),
                            ) else {
                                continue;
                            };
                            // Higher version only (real `pkg < dep.child`
                            // skips downgrades; equal versions -- a slot
                            // move without a revbump -- probe).
                            if vercmp_ordering(c_ver, &old_ver) == Ordering::Less {
                                continue;
                            }
                            // `want_update` stand-in (real `:3805-3809`):
                            // `--update`, or the provider itself is
                            // directly requested (real's arg-chain
                            // disjunct). The registration-time
                            // complete-mode/depth nuance is not modelled.
                            if !update && !top_level_cps.contains(&provider_cp) {
                                continue;
                            }
                            // A `replacement_parent` must exist: with no
                            // visible tree candidate the forced reinstall
                            // would dead-end (real finds no replacement
                            // and does nothing).
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
                            // #304: a replacement parent whose own atom
                            // accepts the new slot must exist.
                            if !replacement_parent_accepts(
                                repos,
                                config,
                                (&pkg.category, &pkg.package, &pkg.slot),
                                dep_keys,
                                (&atom.category, &atom.package),
                                &cand,
                            ) {
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
            // #211 R2: real `_slot_operator_check_reverse_dependencies`
            // (`depgraph.py:2472-2573`, gates `:2622`/`:2738`) refuses
            // the whole replacement when the fresh candidate violates
            // another parent's atom -- on both arms alike.
            // #269: real's `_slot_operator_update_backtrack`
            // (`depgraph.py:2442`) files the child (provider) reinstall
            // only when `new_child_slot is None` -- the new-slot arm
            // records the consumer alone, so only the same-slot arm
            // contributes `(provider, consumer)` display pairs (real's
            // `_forced_rebuilds`, what renders the provider `r` and the
            // `causing rebuilds` block). The consumer itself is scheduled
            // on both arms alike.
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

    // #24 S5: real `_slot_change_probe` (depgraph.py:2317-2359), the
    // slot-move-without-revbump arm (bug 456208). Runs even when the vdb
    // scan above is gated off (real's first trigger arm is not
    // complete-mode gated) -- see the helper's own doc comment. The #212
    // binary arm rides the same call and reports stale binaries into
    // `masked` for `collect_feedback`'s `slot_operator_mask_built`
    // records.
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(
        root,
        repos,
        entries,
        undone,
        &mut scheduled,
        excluded,
        &mut masked,
        config,
    );

    abi_rebuilds.sort();
    abi_rebuilds.dedup();
    (scheduled, abi_rebuilds, masked)
}
