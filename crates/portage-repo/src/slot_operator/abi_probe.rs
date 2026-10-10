//! The slot-conflict ABI probe.

use crate::*;

/// One installed parent's atom on a provider, for the update-probe
/// refusal below: the parent's identity plus the atom text to match the
/// fresh candidate against (built `:S/SS=` atoms already relaxed the way
/// real checks them).
#[derive(Debug, Clone)]
pub(crate) struct ProbeParent {
    /// The installed parent `(category, package)`.
    pub(crate) parent: (String, String),
    /// Effective atom: [`relax_built_slot_operator_atom`] for built
    /// slot-operator atoms, verbatim otherwise.
    pub(crate) atom: String,
}

/// Arguments of [`collect_probe_parents`].
pub(crate) struct CollectProbeParentsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) reachable: &'a HashSet<(String, String)>,
    pub(crate) walked: &'a HashSet<(String, String)>,
    pub(crate) in_graph: &'a HashSet<(String, String)>,
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
}

/// Real `_slot_operator_check_reverse_dependencies`
/// (`lib/_emerge/depgraph.py:2472-2573`) as the update probe's refusal
/// gate on both scan arms (the `:2622` replacement-parent gate and the
/// `:2738` candidate-child gate inside `_slot_operator_update_probe`;
/// the `:2310` slot-conflict-fired probe is v2 `#214`'s own
/// [`slot_conflict_abi_probe`], which reuses this map).
///
/// The caller precollects, per provider cp, every installed package Q
/// whose recorded deps name that cp -- real's `_parent_atoms[child]`.
/// Only parents real could see are collected: Q must be complete-mode
/// reachable or walked this pass (an unwalked parent registers no dep);
/// a Q this run replaces (merge-bound entry, or already in the replace
/// set), uninstalls (blocker-conflict removal), or excludes carries a
/// stale pin real skips (`:2513-2528` elimination,
/// `_upgrade_available`, bug 612772, `excluded_pkgs`).
///
/// `probe_refused` then answers per scheduling edge: the fresh provider
/// candidate (`fresh_cand`, `"cat/pkg-ver:slot/sub"` -- repo-less, so a
/// `::repo` atom passes vacuously, a documented over-match corner) must
/// match every surviving parent atom, with the consumer itself skipped
/// (real's `replacement_parent` skip: the rebuild replaces it, so its
/// own pin is not a veto). A single mismatch refuses the whole
/// replacement -- the caller schedules nothing for that edge.
///
/// The `:2622` gate is not separately ported: the scheduled rebuild
/// reinstalls the same version, which satisfies every atom the installed
/// instance satisfied, so the replacement-parent check is vacuous here
/// (real needs it only because its replacement can be an upgrade).
///
/// Cuts: `_too_deep` (no depths at this layer), the direct-circular
/// escape (no digraph here), and soname atoms (the parser rejects them,
/// same direction as real's `continue`). `[use]` deps are not evaluated
/// (the candidate carries no USE state -- allows, never blocks).
pub(crate) fn collect_probe_parents(
    args: CollectProbeParentsArgs<'_>,
) -> HashMap<(String, String), Vec<ProbeParent>> {
    let CollectProbeParentsArgs {
        root,
        entries,
        reachable,
        walked,
        in_graph,
        already,
        with_bdeps,
        excluded,
    } = args;
    let mut being_removed: HashSet<(String, String)> = HashSet::new();
    for e in entries {
        if matches!(e.outcome, PretendOutcome::Uninstall { .. }) {
            being_removed.insert((e.category.clone(), e.package.clone()));
        }
    }
    let mut parents: HashMap<(String, String), Vec<ProbeParent>> = HashMap::new();
    for pkg in all_installed_packages(root) {
        let qcp = (pkg.category.clone(), pkg.package.clone());
        if !reachable.contains(&qcp) && !walked.contains(&qcp) {
            continue;
        }
        // Merge-bound this pass, or already in the replace set: real's
        // elimination / `_upgrade_available` skips (the pin is stale).
        // Consumers scheduled later in this same scan loop are skipped at
        // check time instead (see `probe_refused`).
        if in_graph.contains(&qcp) || already.contains(&qcp) {
            continue;
        }
        if being_removed.contains(&qcp) {
            continue;
        }
        let q_str = format!("{}/{}-{}", pkg.category, pkg.package, pkg.version);
        if excluded
            .iter()
            .any(|ex| matches_config_entry(ex, &q_str, &pkg.category, &pkg.package))
        {
            continue;
        }
        // Same build-time-key rule as the scheduling loop: real drops a
        // built package's `DEPEND`/`BDEPEND` before any dep is registered
        // when `bdeps` is off, and only non-optional (walked) parents
        // register them when it is on.
        let dep_keys: &[&str] = match with_bdeps && walked.contains(&qcp) {
            true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
            false => &["RDEPEND", "PDEPEND", "IDEPEND"],
        };
        for key in dep_keys {
            let tokens = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
            for token in tokens.split_whitespace() {
                let Some(atom) = portage_dep::parse_atom(token) else {
                    continue;
                };
                if atom.blocker != portage_dep::Blocker::None {
                    continue;
                }
                let provider_cp = (atom.category.clone(), atom.package.clone());
                let effective = relax_built_slot_operator_atom(token);
                parents.entry(provider_cp).or_default().push(ProbeParent {
                    parent: qcp.clone(),
                    atom: effective,
                });
            }
        }
    }
    parents
}

/// Backlog #288: argument parents of the update probe. Real's
/// `_add_parent_atom` records an argument (`Arg`) as a parent of the
/// package it selects, so `_slot_operator_check_reverse_dependencies`
/// refuses a provider candidate that fails an argument atom
/// (`=app-misc/mmprov-1` vetoes every slot-1 candidate, `--debug`:
/// `candidate package does not match atom`). The installed-parent
/// collection above never sees arguments; this files each top-level atom
/// under its own cp with an empty-cp pseudo parent, which
/// [`probe_refused`] neither skips as the consumer nor finds scheduled.
/// Cut: only the rebuild scan's call site files arguments (the slot-
/// conflict probe and the force scan keep installed parents only).
pub(crate) fn add_argument_probe_parents(
    parents: &mut HashMap<(String, String), Vec<ProbeParent>>,
    top_level_atoms: &HashSet<String>,
) {
    for token in top_level_atoms {
        let Some(atom) = portage_dep::parse_atom(token) else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        parents
            .entry((atom.category.clone(), atom.package.clone()))
            .or_default()
            .push(ProbeParent {
                parent: (String::new(), String::new()),
                atom: token.clone(),
            });
    }
}

/// Refusal half of the probe: does any surviving parent of `provider_cp`
/// (other than the consumer itself) reject `fresh_cand`? See
/// [`collect_probe_parents`]. `scheduled` is the replace set as it grows
/// during the scan loop -- a parent scheduled earlier in this same scan
/// is already being rebuilt, so its pin is stale too.
pub(crate) fn probe_refused(
    parents: &HashMap<(String, String), Vec<ProbeParent>>,
    scheduled: &BTreeSet<(String, String)>,
    provider_cp: &(String, String),
    fresh_cand: &str,
    consumer_cp: &(String, String),
) -> bool {
    let Some(list) = parents.get(provider_cp) else {
        return false;
    };
    let refs = [fresh_cand];
    list.iter().any(|p| {
        if &p.parent == consumer_cp || scheduled.contains(&p.parent) {
            return false;
        }
        portage_dep::match_from_list(&p.atom, &refs).is_none_or(|m| m.is_empty())
    })
}

/// `cat/pkg-ver` for a slot-conflict parent edge: [`slot_conflict_puller_cpv`]
/// renders `` `cat/pkg-ver:slot/sub::repo` `` (empty for a top-level
/// `(Argument)` parent). Strips the display suffixes back to the triple.
pub(crate) fn parse_conflict_parent_cpv(parent_cpv: &str) -> Option<(String, String, String)> {
    let head = parent_cpv.split_once("::").map_or(parent_cpv, |(h, _)| h);
    let (cpv_part, _) = head.split_once(':')?;
    split_cpv(cpv_part)
}

/// Arguments of [`slot_conflict_abi_probe`].
pub(crate) struct SlotConflictAbiProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) conflicts: &'a [SlotConflict],
    pub(crate) already: &'a BTreeSet<(String, String)>,
    pub(crate) undone: &'a BTreeSet<(String, String)>,
    pub(crate) with_bdeps: bool,
    pub(crate) walked: &'a HashSet<(String, String)>,
    pub(crate) excluded: &'a [String],
    pub(crate) probe_parents: &'a HashMap<(String, String), Vec<ProbeParent>>,
    pub(crate) config: &'a portage_profile::Config,
}

/// Backlog #214 (v2 `#24e`): real `_slot_conflict_backtrack_abi`
/// (`lib/_emerge/depgraph.py:2282-2315`), the slot-conflict-fired half
/// of the update probe (real `:2310`, via
/// `_slot_operator_update_probe_slot_conflict` `:2453`).
///
/// Real's `_process_slot_conflict` tries this BEFORE the masking
/// backtrack: for every recorded conflict, for every parent edge whose
/// atom is a built package's soname/`:=` atom, it re-probes with
/// `_slot_operator_update_probe_slot_conflict` (the `slot_conflict=True`
/// form -- no `want_update` gate, no complete-mode gate) and, on
/// success, schedules the parent reinstall through
/// `_slot_operator_update_backtrack` (the same
/// `slot_operator_replace_installed` set + restart the #211 scan feeds)
/// instead of reporting the conflict.
///
/// Portuale runs this over the direct solve's surviving records, in the
/// feedback chain before the mask arms (the same order as real), and
/// returns the newly-scheduled parent cps; the caller grows the replace
/// set, which restarts the search -- no masks are filed for a resolved
/// conflict, like real's `remaining` skip. The rebuild's display pair is
/// recomputed at settle time (see [`slot_conflict_abi_display_pairs`]),
/// mirroring real's settle-time `_compute_abi_rebuild_info`.
///
/// Per parent edge (parent P with atom A filed under instance M, other
/// instance C), P is scheduled when every check below holds:
/// - P parses (an `(Argument)` edge is not a package -- real's
///   `not isinstance(parent, Package)` skip) and is installed at that
///   version (real's `parent.built`), but is not merge-bound this pass
///   (a merging parent rebinds its live `:=` through its own walk), is
///   not already scheduled, and is not eliminated;
/// - P is not `--exclude`d (real checks the dep's parent end);
/// - A is a built slot-operator atom (`cat/pkg:S/SS=`, real
///   `Atom.slot_operator_built`) that does NOT match C (else this edge
///   is not conflicted at all);
/// - C differs from M in sub-slot (real skips identical slot/sub-slot)
///   and is not older than M (downgrades need `_downgrade_probe`,
///   unimplemented -- the same direction as the scan);
/// - a same-version tree ebuild for P exists (the reinstall real forces;
///   with no visible candidate it would dead-end) whose live atoms
///   accept C;
/// - the R2 refusal passes for C (the same [`probe_refused`] gate as the
///   scan, over the same [`collect_probe_parents`] map).
///
/// Cuts, all disclosed: soname atoms (the parser rejects them -- the
/// scan carries the same cut); the probe's per-autounmask-level retry
/// (`_slot_operator_update_probe_slot_conflict` re-probes at each level
/// when `_autounmask` is on); merge-bound veto parents (the refusal only
/// sees installed parents, like the scan's); the child-side
/// `--exclude` check (only the parent end is gated); `||` evaluation
/// of the replacement's atoms (every branch is flattened -- USE
/// conditionals ARE evaluated against the replacement's effective USE,
/// #254).
/// Not gated on `rebuild_if_new_slot`: real's conflict path never is
/// (the scan's call-site gate is the pre-existing narrowing for its own
/// arms, not this one).
pub(crate) fn slot_conflict_abi_probe(
    args: SlotConflictAbiProbeArgs<'_>,
) -> BTreeSet<(String, String)> {
    let SlotConflictAbiProbeArgs {
        root,
        repos,
        entries,
        conflicts,
        already,
        undone,
        with_bdeps,
        walked,
        excluded,
        probe_parents,
        config,
    } = args;
    // cp -> () for every merge-bound entry: a merging parent rebinds
    // through its own walk, so only installed parents probe.
    let merge_bound: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| merge_bound_version(&e.outcome).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    // Real restarts the whole search per resolution, so two edges never
    // share one probe call; the growing set below is the local
    // equivalent (a parent scheduled by an earlier edge skips, and its
    // veto is forgiven, exactly like the scan's `scheduled`).
    let mut scheduled: BTreeSet<(String, String)> = BTreeSet::new();
    let mut refusals: BTreeSet<(String, String)> = already.clone();
    for sc in conflicts {
        let provider_cp = (sc.category.clone(), sc.package.clone());
        if sc.instances.len() < 2 {
            continue;
        }
        for (mi, m) in sc.instances.iter().enumerate() {
            for parent in &m.parents {
                let Some((pcat, ppkg, pver)) = parse_conflict_parent_cpv(&parent.parent_cpv) else {
                    continue;
                };
                let pcp = (pcat.clone(), ppkg.clone());
                if !installed_candidates(root, &pcat, &ppkg)
                    .iter()
                    .any(|(v, _, _)| v == &pver)
                {
                    continue;
                }
                if merge_bound.contains(&pcp)
                    || already.contains(&pcp)
                    || scheduled.contains(&pcp)
                    || undone.contains(&pcp)
                {
                    continue;
                }
                let pq_str = format!("{pcat}/{ppkg}-{pver}");
                if excluded
                    .iter()
                    .any(|ex| matches_config_entry(ex, &pq_str, &pcat, &ppkg))
                {
                    continue;
                }
                let Some(atom) = portage_dep::parse_atom(&parent.atom) else {
                    continue;
                };
                if !is_built_slot_op(&atom) {
                    continue;
                }
                // Every other instance is a candidate child; real probes
                // each non-conflicting slot node in turn.
                for (ci, c) in sc.instances.iter().enumerate() {
                    if ci == mi {
                        continue;
                    }
                    if m.sub_slot.is_empty() || c.sub_slot.is_empty() || m.sub_slot == c.sub_slot {
                        continue;
                    }
                    if vercmp_ordering(&c.version, &m.version) == std::cmp::Ordering::Less {
                        continue;
                    }
                    let c_str = format!(
                        "{cat}/{pkg}-{cver}:{slot}/{csub}",
                        cat = sc.category,
                        pkg = sc.package,
                        cver = c.version,
                        slot = sc.slot,
                        csub = c.sub_slot,
                    );
                    // The edge is only conflicted when A rejects C.
                    if portage_dep::match_from_list(&parent.atom, &[c_str.as_str()])
                        .is_some_and(|mt| !mt.is_empty())
                    {
                        continue;
                    }
                    // The same-version reinstall real forces: its live
                    // atoms must accept C (the replacement search), and
                    // its mere presence proves the reinstall can land.
                    let Some(live) = live_metadata_for_installed(repos, root, &pcat, &ppkg, &pver)
                    else {
                        continue;
                    };
                    // Same build-time-key rule as the scan: real empties
                    // the replacement's build-time keys when bdeps is off.
                    let dep_keys: &[&str] = match with_bdeps && walked.contains(&pcp) {
                        true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
                        false => &["RDEPEND", "PDEPEND", "IDEPEND"],
                    };
                    // #254: real's probe evaluates the replacement's USE
                    // conditionals (`_select_atoms_probe`,
                    // `_emerge/depgraph.py:2667-2713`, with
                    // `_pkg_use_enabled(replacement_parent)`): an atom
                    // under a `flag? ( ... )` whose flag the replacement
                    // ebuild does not enable is never selected, so it
                    // cannot accept C. The replacement is the same-version
                    // tree ebuild; its effective USE is the one the walk
                    // would give it.
                    let Some(rep_use) = list_candidates(repos, &pcat, &ppkg).ok().and_then(|cs| {
                        let c = cs
                            .iter()
                            .filter(|c| c.version == pver && c.source == CandidateSource::Ebuild)
                            .max_by_key(|c| c.repo_priority)?;
                        let cand_str = format!(
                            "{pcat}/{ppkg}-{}:{}/{}::{}",
                            c.version, c.slot, c.sub_slot, c.repo_name
                        );
                        Some(effective_use_flags_uncached(
                            config,
                            live.get("IUSE").map(String::as_str).unwrap_or_default(),
                            &c.keywords,
                            &cand_str,
                            &pcat,
                            &ppkg,
                        ))
                    }) else {
                        continue;
                    };
                    let accepts = dep_keys
                        .iter()
                        .filter_map(|k| {
                            flat_dep_atoms(live.get(*k).map(String::as_str).unwrap_or(""), &rep_use)
                        })
                        .flatten()
                        .any(|tok| {
                            let Some(la) = portage_dep::parse_atom(&tok) else {
                                return false;
                            };
                            if la.blocker != portage_dep::Blocker::None {
                                return false;
                            }
                            if la.category != sc.category || la.package != sc.package {
                                return false;
                            }
                            portage_dep::match_from_list(
                                portage_dep::without_use(&tok),
                                &[c_str.as_str()],
                            )
                            .is_some_and(|mt| !mt.is_empty())
                        });
                    if !accepts {
                        continue;
                    }
                    if probe_refused(probe_parents, &refusals, &provider_cp, &c_str, &pcp) {
                        continue;
                    }
                    scheduled.insert(pcp.clone());
                    refusals.insert(pcp.clone());
                }
            }
        }
    }
    scheduled
}

/// Backlog #214 (v2 `#24e`): the display continuity for conflict-fired
/// rebuilds. The firing pass restarts the search, so only the settled
/// graph is left to render real's `causing rebuilds` block -- and the
/// scan cannot re-derive the pair there (no `Upgrade` entry keys its
/// same-slot arm when nothing was installed, and same-slot `New`
/// entries never feed its new-slot arm). Recomputed here from the
/// settled state instead, mirroring real's settle-time
/// `_compute_abi_rebuild_info` (`depgraph.py:996-1090`, which derives
/// the block from the force-reinstalled parents rather than carrying
/// it): for every `slot_operator_rebuild` reinstall entry, each
/// recorded built `:=` atom on a provider cp merged in the same slot at
/// a different sub-slot contributes its `(provider-cpv, consumer-cpv)`
/// pair. Unioned with the scan's pairs and deduped by the caller, so
/// shapes the scan already covers render exactly once.
pub(crate) fn slot_conflict_abi_display_pairs(
    root: &Path,
    entries: &[GraphEntry],
    replace_set: &BTreeSet<(String, String)>,
    with_bdeps: bool,
) -> Vec<(String, String)> {
    let walked: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    let mut pairs: Vec<(String, String)> = Vec::new();
    for e in entries {
        let PretendOutcome::Reinstall {
            version,
            slot_operator_rebuild: true,
            ..
        } = &e.outcome
        else {
            continue;
        };
        let ccp = (e.category.clone(), e.package.clone());
        if !replace_set.contains(&ccp) {
            continue;
        }
        let consumer_cpv = format!("{}/{}-{version}", e.category, e.package);
        let dep_keys: &[&str] = match with_bdeps && walked.contains(&ccp) {
            true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
            false => &["RDEPEND", "PDEPEND", "IDEPEND"],
        };
        for key in dep_keys {
            let recorded = read_vdb_string(root, &e.category, &e.package, version, key);
            for tok in recorded.split_whitespace() {
                let Some(atom) = portage_dep::parse_atom(tok) else {
                    continue;
                };
                if !is_built_slot_op(&atom) {
                    continue;
                }
                let Some(bound_slot) = atom.slot.clone() else {
                    continue;
                };
                let bound_sub = atom.sub_slot.clone().unwrap_or_default();
                // #254: real `_compute_abi_rebuild_info`
                // (`_emerge/depgraph.py:996-1090`) only pairs a provider
                // that is a graph child of the *replacement* consumer
                // (`digraph.child_nodes(reinst_pkg)`): a rebuild whose new
                // build no longer pulls the provider (a USE-conditional
                // `:=` now off) was not caused by it, and real prints no
                // pair. The provider entry's `required_by` is that edge.
                for cand in entries.iter().filter(|o| {
                    o.category == atom.category
                        && o.package == atom.package
                        && merge_bound_version(&o.outcome).is_some()
                        && o.slot.as_deref() == Some(bound_slot.as_str())
                        && o.sub_slot.as_deref().is_some_and(|s| *s != bound_sub)
                        && o.required_by.contains(&ccp)
                }) {
                    let Some(pver) = merge_bound_version(&cand.outcome) else {
                        continue;
                    };
                    pairs.push((
                        format!("{}/{}-{pver}", atom.category, atom.package),
                        consumer_cpv.clone(),
                    ));
                }
            }
        }
    }
    pairs
}

/// Real depgraph's `_slot_operator_trigger_reinstalls` (3089-3132) +
/// `_slot_operator_replace_installed` (the
/// `@__auto_slot_operator_replace_installed__` set), run once every
/// merge-bound entry is resolved: an installed package whose vdb
/// `*DEPEND` carries a *built* slot-operator atom (`cat/pkg:S/SS=` --
/// operator `=`, both slot and sub-slot present, real
/// `Atom.slot_operator_built`) whose bound `S/SS` no longer matches how
/// this run leaves `cat/pkg` **in that same slot** must be rebuilt --
/// real portage's "don't leave a broken ABI link" auto-rebuild.
///
/// #24 S3: this is the *scan* only. It returns the consumer `(cat, pkg)`
/// set for `BacktrackParams::slot_operator_replace_installed` (real's
/// `backtrack_infos["config"]["slot_operator_replace_installed"]`) plus
/// the `(provider_cpv, consumer_cpv)` display pairs (real's
/// `_forced_rebuilds`), and builds **no** `GraphEntry`: the consumer
/// becomes a walked node on the next pass (`run_pass`'s seed + flip),
/// which is what gives it re-evaluated deps, a re-bound `:=` and real
/// merge-order edges. The old post-pass entry synthesiser -- and with it
/// the internal `scheduled`/`new_slot` fixpoint -- is gone; the cascade
/// now falls out of the next pass seeing the rebuilt consumer as a
/// provider at its *tree* sub-slot.
///
/// A provider entry counts only when it *replaces* an installed version
/// in a slot (`Upgrade`/`Downgrade`/`Reinstall` -- a new-other-slot
/// `New` leaves the old slot untouched, so it triggers nothing), and
/// only for a consumer whose bound atom names that exact slot with a
/// **different** sub-slot.
///
/// **The scan is gated on `reachable`** -- real only slot-op-rebuilds a
/// consumer that is in the graph in complete mode (a member of, or a
/// forward-installed-dep of a member of, the required sets: real
/// `_complete_graph` 8677 starts from `_initial_arg_list` -- the CLI
/// args -- and appends `@world`/`@selected`/`@system`). `reachable`
/// empty (not complete mode) means no rebuilds at all. #24 S5: the vdb
/// half of this scan keeps that gate; the appended
/// [`slot_operator_slot_change_probe`] half does *not* (real 3103-3107).
///
/// `already` is the replace set as the current search node carries it.
/// A cp in it is *not* skipped by the in-graph test (it is in the graph
/// precisely because this scan put it there last pass), so its
/// `abi_rebuilds` pairs keep being reported for the whole search; every
/// other in-graph cp is skipped, exactly as before.
///
/// `undone` (#24 S4) is the undo latch: a cp real's `_eliminate_rebuilds`
/// demoted must never be scheduled again within a search node (real
/// mutates the settled graph in place and does not restart, so its scan
/// cannot re-fire; portuale's re-runs every pass and needs the latch).
///
/// `with_bdeps` (#65 S1) mirrors real's `bdeps` parameter
/// (`_emerge/create_depgraph_params.py:97-103`): only when it is true are
/// an *installed* consumer's `DEPEND`/`BDEPEND` read at all.
/// `_add_pkg_deps` (`_emerge/depgraph.py:4194-4247`) empties a built
/// package's `DEPEND`/`BDEPEND` before any dep is registered when `bdeps`
/// is not `y`/`auto` (real's default under `--usepkg`/`--getbinpkg`), so
/// no slot-operator dep can ever be registered from them and no rebuild
/// can fire. `RDEPEND`/`PDEPEND`/`IDEPEND` are runtime/install-time keys
/// and stay unconditional.
///
/// #65 S2: with `with_bdeps` true, `DEPEND`/`BDEPEND` are still read only
/// for a consumer this pass actually **walked** (`entries`), not for one
/// only *reachable* in complete mode. Real registers an installed
/// parent's build-time slot-op dep with `optional=pkg.built`
/// (`depgraph.py:4277/4287`) and `_slot_operator_trigger_reinstalls` only
/// probes `want_update` parents (`:3114`), which complete-mode-only
/// reachability never satisfies (`:3802-3806`) -- so a world-only
/// build-time consumer stays untouched while the same consumer, walked
/// (an `@world` seed or an argument), rebuilds. The runtime keys keep the
/// `reachable` gate: the slot-conflict path rebuilds graph-node runtime
/// consumers in both shapes.
///
/// Cuts (v2 remainder): the `_slot_change_probe`'s `runtime_pkg_mask`
/// check on the tree ebuild (`:2341`); the update-probe
/// (`_slot_operator_update_backtrack`, `:2427-2431`) and unsatisfied-probe
/// (`_slot_operator_unsatisfied_backtrack`, `:2901-2903`)
/// `slot_operator_mask_built` arms, which have no portuale-visible
/// diverging shape -- both steer a *parent* the graph is already
/// replacing (update) or failing on (unsatisfied), so same-version
/// binary-vs-ebuild steering is vacuous there. The `:2310`
/// slot-conflict-fired probe is NOT this scan (v2 `#214` ports it as
/// the sibling [`slot_conflict_abi_probe`], fed by the surviving
/// conflict records rather than the merge entries).
/// Highest visible `(slot, version)` for a cp: real `_greedy_slots`'
/// highest package (`lib/_emerge/depgraph.py:5896`), the slot a
/// `--update` request keeps unpinned while every lower installed slot
/// gains a `cat/pkg:S` request-argument pin. See the `#252` gate in
/// [`slot_operator_rebuild_scan`].
pub(crate) fn highest_visible_slot(
    repos: &[RepoConfig],
    cp: &(String, String),
) -> Option<(String, String)> {
    list_candidates(repos, &cp.0, &cp.1)
        .ok()?
        .iter()
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
        .map(|c| (c.slot.clone(), c.version.clone()))
}

/// Backlog #268 cut (a): the requested cps real's `--update` greedy
/// expansion actually pins (`_greedy_slots`, `depgraph.py:5896`).
///
/// Real expands each directly-requested atom into per-slot `cat/pkg:S`
/// args, but only when the atom spans slots: `slots` is every installed
/// slot matching the atom plus the highest visible match's slot, and a
/// single slot means no pins at all. An explicit slot (`mmprov:1/2`) or
/// exact version (`=mmprov-2`) atom therefore pins nothing -- the update
/// probe's reverse-deps gate has no `P:S` arg to refuse against and the
/// probe fires (probed on real 3.0.82.2, 2026-10-01; see
/// `test_oracle_slotop_pin_gate_explicit_request_atoms_probe`).
/// `top_level_cps` (every requested cp, whatever the atom form) stays
/// the `want_update` stand-in -- an exact-version request still selects
/// the provider through the arg chain (`=mmprov-2` rebuilds its consumer
/// with no upgrade row at all).
///
/// Visibility/masking is unfiltered here, the same approximation as
/// [`highest_visible_slot`] (real `_select_package` respects it).
pub(crate) fn greedy_pinned_cps(
    atoms: &HashSet<String>,
    repos: &[RepoConfig],
    installed_by_cp: &HashMap<(String, String), Vec<(String, String)>>,
) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for atom_str in atoms {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            continue;
        };
        let cp = (atom.category.clone(), atom.package.clone());
        if out.contains(&cp) {
            continue;
        }
        let mut slots: HashSet<String> = HashSet::new();
        if let Some(insts) = installed_by_cp.get(&cp) {
            for (version, slot) in insts {
                let r = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if portage_dep::match_from_list(atom_str, &[r.as_str()])
                    .is_some_and(|m| !m.is_empty())
                {
                    slots.insert(slot.split('/').next().unwrap_or(slot).to_string());
                }
            }
        }
        if let Ok(tree) = list_candidates(repos, &atom.category, &atom.package) {
            let best = tree
                .iter()
                .filter(|c| {
                    let r = format!(
                        "{}/{}-{}:{}/{}",
                        atom.category, atom.package, c.version, c.slot, c.sub_slot
                    );
                    portage_dep::match_from_list(atom_str, &[r.as_str()])
                        .is_some_and(|m| !m.is_empty())
                })
                .max_by(|a, b| vercmp_ordering(&a.version, &b.version));
            if let Some(h) = best {
                slots.insert(h.slot.clone());
            }
        }
        if slots.len() >= 2 {
            out.insert(cp);
        }
    }
    out
}

/// Backlog #268 cut (b): does the bound-slot pin survive real's greedy
/// revision with blocker lookahead (`depgraph.py:5421`,
/// `_greedy_slots(..., blocker_lookahead=True)`)?
///
/// Real re-derives the greedy args accounting for blockers between the
/// selected packages and discards a pin whose package conflicts with
/// the highest one, so the old slot is uninstalled instead of updated
/// and the probe fires (probed on real 3.0.82.2, 2026-10-01; see
/// `test_oracle_slotop_pin_gate_blocker_discard_probes`). This models
/// the highest-vs-bound-slot half: `G` is the highest visible tree
/// package in the bound slot (real `_select_package` of the `P:S`
/// atom), `H` the highest visible overall, both with their effective
/// USE (the walk's own, via `effective_use_flags_uncached` over
/// `flat_dep_atoms` -- real `_select_atoms` with `_pkg_use_enabled`);
/// a `!`/`!!` token on either side matching the other package discards
/// the pin. Unreadable tree metadata keeps the pin (no proven
/// discard); an unreducible dep string drops it (real filters packages
/// with invalid deps out of the greedy set). Still cut: the pairwise
/// lower-version discard among two or more greedy packages (needs three
/// installed slots; no fixture spans that yet).
pub(crate) fn greedy_pin_survives_revision(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    provider_cp: &(String, String),
    bound_slot: &str,
) -> bool {
    let Ok(tree) = list_candidates(repos, &provider_cp.0, &provider_cp.1) else {
        return true;
    };
    let ebuilds: Vec<&Candidate> = tree
        .iter()
        .filter(|c| c.source == CandidateSource::Ebuild)
        .collect();
    let highest = ebuilds
        .iter()
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version));
    let bound = ebuilds
        .iter()
        .filter(|c| c.slot == bound_slot)
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version));
    let (Some(h), Some(g)) = (highest, bound) else {
        return true;
    };
    if h.version == g.version {
        return true;
    }
    let blockers_of = |c: &Candidate| -> Option<HashSet<String>> {
        let pf = format!("{}-{}", provider_cp.1, c.version);
        let meta = repo_aux_metadata(&c.repo_location, &provider_cp.0, &pf).ok()?;
        let cand_str = format!(
            "{}/{}-{}:{}/{}::{}",
            provider_cp.0, provider_cp.1, c.version, c.slot, c.sub_slot, c.repo_name
        );
        let use_flags = effective_use_flags_uncached(
            config,
            meta.get("IUSE").map(String::as_str).unwrap_or_default(),
            &c.keywords,
            &cand_str,
            &provider_cp.0,
            &provider_cp.1,
        );
        let mut out = HashSet::new();
        for key in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
            let atoms =
                flat_dep_atoms(meta.get(key).map(String::as_str).unwrap_or(""), &use_flags)?;
            for tok in atoms {
                let Some(la) = portage_dep::parse_atom(&tok) else {
                    continue;
                };
                if la.blocker != portage_dep::Blocker::None {
                    out.insert(tok);
                }
            }
        }
        Some(out)
    };
    let (Some(h_blockers), Some(g_blockers)) = (blockers_of(h), blockers_of(g)) else {
        return false;
    };
    let g_ref = format!(
        "{}/{}-{}:{}/{}",
        provider_cp.0, provider_cp.1, g.version, g.slot, g.sub_slot
    );
    let h_ref = format!(
        "{}/{}-{}:{}/{}",
        provider_cp.0, provider_cp.1, h.version, h.slot, h.sub_slot
    );
    let hits = |tokens: &HashSet<String>, r: &str| {
        tokens.iter().any(|t| {
            portage_dep::match_from_list(portage_dep::without_use(t), &[r])
                .is_some_and(|m| !m.is_empty())
        })
    };
    !(hits(&h_blockers, &g_ref) || hits(&g_blockers, &h_ref))
}

/// Arguments of [`replacement_parent_accepts`].
pub(crate) struct ReplacementParentAcceptsArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) consumer: (&'a str, &'a str, &'a str),
    pub(crate) dep_keys: &'a [&'a str],
    pub(crate) provider: (&'a str, &'a str),
    pub(crate) candidate: &'a str,
}

/// Backlog #304: real `_slot_operator_update_probe`
/// (`depgraph.py:2576`) only reports a new-slot update when some
/// *replacement parent* -- another available package in the consumer's
/// slot (`_iter_similar_available(dep.parent, dep.parent.slot_atom)`) --
/// carries an atom on the provider that **accepts** the candidate
/// (`replacement_candidates`/`all_candidate_pkgs`, `:2667-2760`; with no
/// agreeing candidate the loop `continue`s and the probe returns `None`).
/// A consumer whose current dependency atom is slot-pinned
/// (`gcr:0/1=`) therefore never rebuilds just because the provider also
/// exists at a higher slot (`gcr:4`).
///
/// Approximates the replacement parent with the consumer's available tree
/// ebuilds in its slot, evaluated with their own effective USE (the same
/// reading the slot-conflict probe's `accepts` uses).
///
/// Deliberate narrowing (documented cut): the candidate is refused only
/// when some replacement ebuild **names the provider** and none of those
/// atoms accepts it -- the `gcr:0/1=` vs `gcr:4` shape. A replacement that
/// names the provider nowhere (real would also `continue`: no atom, no
/// `replacement_candidates`) or one that cannot be read stays accepted, the
/// pre-#304 behaviour, because the bare-ebuild unit fixtures and any tree
/// whose metadata is incomplete would otherwise lose the probe entirely.
pub(crate) fn replacement_parent_accepts(args: ReplacementParentAcceptsArgs<'_>) -> bool {
    let ReplacementParentAcceptsArgs {
        repos,
        config,
        consumer,
        dep_keys,
        provider,
        candidate,
    } = args;
    let (cat, pkg, slot) = consumer;
    let Ok(cands) = list_candidates(repos, cat, pkg) else {
        return true;
    };
    let mut names_provider = false;
    for c in cands
        .iter()
        .filter(|c| c.source == CandidateSource::Ebuild && c.slot == slot)
    {
        let Ok(meta) = repo_aux_metadata(&c.repo_location, cat, &format!("{pkg}-{}", c.version))
        else {
            continue;
        };
        let cand_str = format!(
            "{cat}/{pkg}-{}:{}/{}::{}",
            c.version, c.slot, c.sub_slot, c.repo_name
        );
        let use_flags = effective_use_flags_uncached(
            config,
            meta.get("IUSE").map(String::as_str).unwrap_or_default(),
            &c.keywords,
            &cand_str,
            cat,
            pkg,
        );
        for tok in dep_keys
            .iter()
            .filter_map(|k| {
                flat_dep_atoms(meta.get(*k).map(String::as_str).unwrap_or(""), &use_flags)
            })
            .flatten()
        {
            let Some(la) = portage_dep::parse_atom(&tok) else {
                continue;
            };
            if la.blocker != portage_dep::Blocker::None
                || la.category != provider.0
                || la.package != provider.1
            {
                continue;
            }
            names_provider = true;
            if portage_dep::match_from_list(portage_dep::without_use(&tok), &[candidate])
                .is_some_and(|m| !m.is_empty())
            {
                return true;
            }
        }
    }
    !names_provider
}
