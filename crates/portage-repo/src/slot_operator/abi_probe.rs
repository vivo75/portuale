//! The slot-conflict ABI probe.

use crate::*;

/// One installed parent's atom on a provider, for the update-probe refusal:
/// the parent's identity plus the effective atom text to match the fresh
/// candidate against.
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

/// Per provider cp, every installed package whose recorded deps name that cp.
/// `probe_refused` then refuses a fresh provider candidate that fails any
/// surviving parent atom (skipping the consumer itself, whose rebuild replaces
/// it). The `:2622` replacement-parent gate is vacuous here: the scheduled
/// rebuild reinstalls the same version.
// Portage: _emerge/depgraph.py, _slot_operator_check_reverse_dependencies
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
    for pkg in all_installed_packages(root).iter() {
        let qcp = (pkg.category.clone(), pkg.package.clone());
        if !reachable.contains(&qcp) && !walked.contains(&qcp) {
            continue;
        }
        // Merge-bound this pass, already scheduled, being removed, or
        // excluded: the pin is stale.
        if in_graph.contains(&qcp) || already.contains(&qcp) {
            continue;
        }
        if being_removed.contains(&qcp) {
            continue;
        }
        let q_str = format!("{}/{}-{}", pkg.category, pkg.package, pkg.version);
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &q_str))
        {
            continue;
        }
        // A built package's DEPEND/BDEPEND only count when bdeps is on and
        // the parent was walked.
        let dep_keys: &[&str] = match with_bdeps && walked.contains(&qcp) {
            true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
            false => &["RDEPEND", "PDEPEND", "IDEPEND"],
        };
        for key in dep_keys {
            let tokens = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
            for token in tokens.split_whitespace() {
                let Some(atom) = parse_dep_atom(token, "abi_probe_vdb_deps") else {
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

/// Argument parents of the update probe: each top-level atom is filed under
/// its own cp with an empty-cp pseudo parent, so it vetoes a provider
/// candidate it does not match. Only the rebuild scan files arguments.
pub(crate) fn add_argument_probe_parents(
    parents: &mut HashMap<(String, String), Vec<ProbeParent>>,
    top_level_atoms: &HashSet<String>,
) {
    for token in top_level_atoms {
        let Some(atom) = parse_dep_atom(token, "abi_probe_argument_atoms") else {
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
/// (other than the consumer itself) reject `fresh_cand`? A parent scheduled
/// earlier in this same scan is already being rebuilt, so its pin is stale too.
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

/// `cat/pkg-ver` for a slot-conflict parent edge, stripped back from the
/// display form (empty for a top-level `(Argument)` parent).
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

/// Portage's slot-conflict-fired half of the update probe: for every recorded
/// conflict, for every parent edge whose atom is a built `:=` atom that rejects
/// another instance, schedule the parent reinstall when the same-version tree
/// ebuild's live atoms accept that instance and no other parent vetoes it.
/// Runs over the direct solve's surviving records, before the mask arms, and
/// returns the newly-scheduled parent cps; no masks are filed for a resolved
/// conflict.
// Portage: _emerge/depgraph.py, _slot_conflict_backtrack_abi
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
    // cp -> () for every merge-bound entry: only installed parents probe.
    let merge_bound: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| merge_bound_version(&e.outcome).is_some())
        .map(|e| (e.category.clone(), e.package.clone()))
        .collect();
    // Portage restarts the whole search per resolution; the growing set
    // below is the local equivalent.
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
                    .any(|ex| matches_cli_option_atom(ex, &pq_str))
                {
                    continue;
                }
                let Some(atom) = parse_dep_atom(&parent.atom, "abi_probe_parent_atom") else {
                    continue;
                };
                if !is_built_slot_op(&atom) {
                    continue;
                }
                // Every other instance is a candidate child.
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
                        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
                    // The same-version reinstall Portage forces: its live
                    // atoms must accept C.
                    let Some(live) = live_metadata_for_installed(repos, root, &pcat, &ppkg, &pver)
                    else {
                        continue;
                    };
                    // The replacement's build-time keys follow the same rule
                    // as the scan.
                    let dep_keys: &[&str] = match with_bdeps && walked.contains(&pcp) {
                        true => &["RDEPEND", "PDEPEND", "DEPEND", "BDEPEND", "IDEPEND"],
                        false => &["RDEPEND", "PDEPEND", "IDEPEND"],
                    };
                    // The replacement's USE conditionals are evaluated against
                    // its effective USE: an atom under a disabled `flag?`
                    // group is never selected, so it cannot accept C.
                    let Some(rep_use) = list_candidates(repos, &pcat, &ppkg).ok().and_then(|cs| {
                        let c = cs
                            .iter()
                            .filter(|c| c.version == pver && c.source == CandidateSource::Ebuild)
                            .max_by_key(|c| c.repo_priority)?;
                        let cand_str = c.match_str(&pcat, &ppkg);
                        Some(effective_use_flags_uncached(
                            EffectiveUseFlagsUncachedArgs {
                                config,
                                iuse: live.get("IUSE").map(String::as_str).unwrap_or_default(),
                                keywords: &c.keywords,
                                candidate_str: &cand_str,
                                build_id: c.build_id.as_deref(),
                                category: &pcat,
                                package: &ppkg,
                            },
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
                            let Some(la) = parse_dep_atom(&tok, "abi_probe_live_deps") else {
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

/// Display continuity for conflict-fired rebuilds: recomputed here from the
/// settled state, since the firing pass restarts the search. For every
/// `slot_operator_rebuild` reinstall entry, each recorded built `:=` atom on
/// a provider cp merged in the same slot at a different sub-slot contributes
/// its `(provider-cpv, consumer-cpv)` pair.
// Portage: _emerge/depgraph.py, _compute_abi_rebuild_info
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
                let Some(atom) = parse_dep_atom(tok, "abi_probe_recorded_deps") else {
                    continue;
                };
                if !is_built_slot_op(&atom) {
                    continue;
                }
                let Some(bound_slot) = atom.slot.clone() else {
                    continue;
                };
                let bound_sub = atom.sub_slot.clone().unwrap_or_default();
                // Only a provider that is a graph child of the replacement
                // consumer counts: a rebuild whose new build no longer pulls
                // the provider was not caused by it.
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

/// The same-slot vdb scan (Portage's `_slot_operator_trigger_reinstalls`):
/// an installed consumer whose built `:S/SS=` atom no longer matches how this
/// run leaves that slot is rebuilt. Returns the consumer set plus display
/// pairs and builds no `GraphEntry`: the consumer becomes a walked node on the
/// next pass. Only a provider entry that replaces an installed version in a
/// slot counts, and only for a consumer bound to that slot at a different
/// sub-slot. The parameter gates (`reachable`, `already`, `undone`,
/// `with_bdeps`) are recorded in the notes.
// Portage: _emerge/depgraph.py, _slot_operator_trigger_reinstalls
/// Highest visible `(slot, version)` for a cp: the slot a `--update` request
/// keeps unpinned while every lower installed slot gains a `cat/pkg:S` pin.
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

/// The requested cps Portage's `--update` greedy expansion actually pins:
/// each directly-requested atom expands into per-slot `cat/pkg:S` args, but
/// only when the atom spans slots (an explicit slot or exact-version request
/// pins nothing). Visibility/masking is unfiltered here.
// Portage: _emerge/depgraph.py, _greedy_slots
pub(crate) fn greedy_pinned_cps(
    atoms: &HashSet<String>,
    repos: &[RepoConfig],
    installed_by_cp: &HashMap<(String, String), Vec<(String, String)>>,
) -> HashSet<(String, String)> {
    let mut out = HashSet::new();
    for atom_str in atoms {
        let Some(atom) = parse_dep_atom(atom_str, "abi_probe_pinned_atoms") else {
            continue;
        };
        let cp = (atom.category.clone(), atom.package.clone());
        if out.contains(&cp) {
            continue;
        }
        let mut slots: HashSet<String> = HashSet::new();
        if let Some(insts) = installed_by_cp.get(&cp) {
            for (version, slot) in insts {
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
                        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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

/// Whether the bound-slot pin survives Portage's greedy revision with blocker
/// lookahead: a `!`/`!!` token on either side matching the other package
/// discards the pin, so the old slot is uninstalled instead of updated and the
/// probe fires. Unreadable tree metadata keeps the pin; an unreducible dep
/// string drops it.
// Portage: _emerge/depgraph.py, _greedy_slots
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
        let cand_str = c.match_str(&provider_cp.0, &provider_cp.1);
        let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
            config,
            iuse: meta.get("IUSE").map(String::as_str).unwrap_or_default(),
            keywords: &c.keywords,
            candidate_str: &cand_str,
            build_id: c.build_id.as_deref(),
            category: &provider_cp.0,
            package: &provider_cp.1,
        });
        let mut out = HashSet::new();
        for key in ["BDEPEND", "DEPEND", "IDEPEND", "PDEPEND", "RDEPEND"] {
            let atoms =
                flat_dep_atoms(meta.get(key).map(String::as_str).unwrap_or(""), &use_flags)?;
            for tok in atoms {
                let Some(la) = parse_dep_atom(&tok, "abi_probe_meta_deps") else {
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
        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
        "{}/{}-{}:{}/{}",
        provider_cp.0, provider_cp.1, g.version, g.slot, g.sub_slot
    );
    let h_ref = format!(
        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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

/// Portage's update probe only reports a new-slot update when some replacement
/// parent (another available package in the consumer's slot) carries an atom
/// on the provider that accepts the candidate. The candidate is refused only
/// when some replacement ebuild names the provider and none of those atoms
/// accepts it; a replacement that names the provider nowhere stays accepted.
// Portage: _emerge/depgraph.py, _slot_operator_update_probe
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
        let cand_str = c.match_str(cat, pkg);
        let use_flags = effective_use_flags_uncached(EffectiveUseFlagsUncachedArgs {
            config,
            iuse: meta.get("IUSE").map(String::as_str).unwrap_or_default(),
            keywords: &c.keywords,
            candidate_str: &cand_str,
            build_id: c.build_id.as_deref(),
            category: cat,
            package: pkg,
        });
        for tok in dep_keys
            .iter()
            .filter_map(|k| {
                flat_dep_atoms(meta.get(*k).map(String::as_str).unwrap_or(""), &use_flags)
            })
            .flatten()
        {
            let Some(la) = parse_dep_atom(&tok, "abi_probe_meta_tokens") else {
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
