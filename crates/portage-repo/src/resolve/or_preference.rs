//! Which alternative of an `||` group to prefer.

use crate::*;

/// Arguments of [`alternative_downgrade_demoted`].
pub(crate) struct AlternativeDowngradeDemotedArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) self_cp: &'a RootCp,
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) atoms: &'a [String],
    pub(crate) queued: &'a [QueueItem],
    pub(crate) update: bool,
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) view: OrView<'a>,
}

/// An otherwise-available `||` alternative is demoted when one of its atoms would pull
/// in a *lower* version of a package whose slot the live graph already holds at a
/// higher version, unless that downgrade is desirable (`downgrade_probe`).
/// Portage: dep_check.py, dep_zapdeps (conflict_downgrade / installed_downgrade, bug 531656).
///
/// Portage's `graph_db` is the live package set; portuale's is the in-progress `entries`,
/// the same set at the same moment. `avail_pkg` ignores `[use]`; `highest_in_slot` needs
/// a graph node holding the slot; an atom of the entry's own `cat/pkg` never demotes.
pub(crate) fn alternative_downgrade_demoted(args: AlternativeDowngradeDemotedArgs<'_>) -> bool {
    let AlternativeDowngradeDemotedArgs {
        root,
        config,
        entries,
        self_cp,
        constraints,
        atoms,
        queued,
        update,
        gate,
        view,
    } = args;
    atoms.iter().any(|a| {
        let Some(at) = parse_dep_atom(a, "or_preference") else {
            return false;
        };
        if at.blocker != portage_dep::Blocker::None
            || (at.category == self_cp.1 && at.package == self_cp.2)
        {
            return false;
        }
        let extra = constraints
            .get(&(at.category.clone(), at.package.clone()))
            .map_or(&[][..], Vec::as_slice);
        if let OrView::PostMerge(set) = view {
            // The set holds the graph: the best member is `avail_pkg`, the highest
            // member of its slot is `highest_in_slot`.
            let Some((avail_version, avail_slot)) = set.best_matching(a) else {
                return false;
            };
            let Some(highest) = set.highest_in_slot(&at.category, &at.package, &avail_slot) else {
                return false;
            };
            if vercmp_ordering(&avail_version, &highest) != Ordering::Less {
                return false;
            }
            let slot_atom = format!("{}/{}:{avail_slot}", at.category, at.package);
            return !downgrade_probe(gate, config, &slot_atom, &avail_version, extra);
        }
        let Some((avail_version, avail_slot)) = visible_tree_matches(gate, a, config, extra)
            .into_iter()
            .max_by(|x, y| vercmp_ordering(&x.0, &y.0))
        else {
            return false;
        };
        // Merge-bound versions, plus the installed version when an `AlreadyInstalled`
        // node holds the slot.
        let mut in_slot: Vec<String> = entries
            .iter()
            .filter(|e| {
                e.category == at.category
                    && e.package == at.package
                    && e.slot.as_deref() == Some(avail_slot.as_str())
            })
            .filter_map(|e| match &e.outcome {
                PretendOutcome::AlreadyInstalled { version }
                | PretendOutcome::New { version }
                | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some(to.clone())
                }
                // A removal installs nothing into the slot.
                PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
            })
            .collect();
        // Portage selects world/argv atoms before walking, so the pending update is already
        // a graph node at judgement time. Portuale judges eagerly: a queued same-cp atom
        // stands in (tree-best under `--update`, installed-best otherwise).
        if in_slot.is_empty() {
            let mut hypothetical: Vec<String> = Vec::new();
            for q in queued {
                let Some(qat) = parse_dep_atom(&q.atom, "or_preference") else {
                    continue;
                };
                if qat.category != at.category || qat.package != at.package {
                    continue;
                }
                let qextra = constraints
                    .get(&(qat.category.clone(), qat.package.clone()))
                    .map_or(&[][..], Vec::as_slice);
                if update {
                    hypothetical.extend(
                        visible_tree_matches(gate, &q.atom, config, qextra)
                            .into_iter()
                            .map(|(v, _)| v),
                    );
                } else {
                    hypothetical.extend(
                        installed_candidates(root, &qat.category, &qat.package)
                            .into_iter()
                            .filter_map(|(v, _, _)| {
                                let probe = format!("{}/{}-{v}", qat.category, qat.package);
                                if portage_dep::match_from_list(&q.atom, &[probe.as_str()])
                                    .is_some_and(|m| !m.is_empty())
                                {
                                    Some(v)
                                } else {
                                    None
                                }
                            }),
                    );
                }
            }
            in_slot = hypothetical;
        }
        let Some(highest_in_graph) = in_slot.iter().max_by(|x, y| vercmp_ordering(x, y)) else {
            return false;
        };
        if vercmp_ordering(&avail_version, highest_in_graph) != Ordering::Less {
            return false;
        }
        let slot_atom = format!("{}/{}:{avail_slot}", at.category, at.package);
        // Both downgrade guards share one test once `highest_in_slot` is a graph node:
        // lower than what the graph holds, and not a desirable downgrade.
        !downgrade_probe(gate, config, &slot_atom, &avail_version, extra)
    })
}

/// Arguments of [`disjunction_preference`].
pub(crate) struct DisjunctionPreferenceArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) self_cp: &'a RootCp,
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) circular_dependency: &'a HashMap<RootCp, Vec<CircularDepChild>>,
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) atoms: &'a [String],
    pub(crate) queued: &'a [QueueItem],
    pub(crate) update: bool,
    /// The category allow-list gate for the downgrade guard's tree lookups.
    pub(crate) gate: CategoryGate<'a>,
    /// The whole-graph views for the circular-self-dep gate below. `entries`/`queued`
    /// above are the group's own root scope; the gate asks whether the owner's cp is in
    /// play *at all*, so it reads these unscoped views instead. Same slices on every
    /// same-root call -- a strict no-op there.
    pub(crate) gate_entries: &'a [GraphEntry],
    pub(crate) gate_queued: &'a [QueueItem],
    /// The package set the choice is judged against.
    pub(crate) view: OrView<'a>,
}

/// The single probe closure `use_reduce_flat_disjunctive` calls for every `||`
/// alternative -- Portage's whole choice-bin classification collapsed into one rank.
/// Shared by both of portuale's `||`-resolution call sites so a fix to one can never
/// silently miss the other again. Portage: dep_check.py, dep_zapdeps.
///
/// `constraints` is the backtrack loop's per-`cat/pkg` mask (kept as the raw map: the
/// closure's borrowed return can't satisfy the loop's later mutations).
/// `circular_dependency` is the loop's map for this pass (empty on the first attempt).
/// `root_deps_running_root` is the `--root-deps` feed-in (`None` = no-op).
///
/// `circular_dependency` demotes an alternative matching one of `self_cp`'s recorded
/// children to `other`, so the re-walk breaks the cycle. Installed-walk call sites pass
/// an empty map (an installed parent is never a merge-bound cycle node).
///
/// Each atom's availability is probed once and reused for both facts when there is no
/// `[use]` block to strip (for such an atom the two probes would do byte-identical
/// candidate-listing work twice).
pub(crate) fn disjunction_preference(
    args: DisjunctionPreferenceArgs<'_>,
) -> portage_use_reduce::AltPreference {
    let DisjunctionPreferenceArgs {
        config,
        root,
        entries,
        self_cp,
        constraints,
        circular_dependency,
        root_deps_running_root,
        atoms,
        queued,
        update,
        gate,
        gate_entries,
        gate_queued,
        view,
    } = args;
    let probe = OrProbe { gate, root, config };
    let constraints_for = |a: &str| -> &[String] {
        parse_dep_atom(a, "or_preference")
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    let mut all_available = true;
    let mut all_use_satisfied = true;
    let mut all_use_unmasked = true;
    // Once true, the alternative is hard `Unsatisfiable` in BOTH passes below, never
    // promoted into an `other_*` bin the way a genuine `!all_available` atom is.
    //
    // Narrowed to Portage's actual pass-1 rule (no self-exclusion): the self-naming atom
    // matches an in-graph node and ranks bin 0; the cycle forms, serialization dead-ends,
    // and the map re-resolves it. Only a self atom matching nothing stays `Unsatisfiable`.
    let mut circular = false;
    // The drain-state approximation (see `atoms_all_in_graph`), built lazily for
    // self-naming atoms only.
    let mut queued_cps: Option<HashSet<(String, String)>> = None;
    for a in atoms {
        let self_naming = parse_dep_atom(a, "or_preference").is_some_and(|at| {
            at.blocker == portage_dep::Blocker::None
                && at.category == self_cp.1
                && at.package == self_cp.2
        });
        if self_naming
            && !view.cp_installed(root, a)
            && !atom_cp_present_in_graph(
                a,
                gate_entries,
                config,
                queued_cps.get_or_insert_with(|| {
                    gate_queued
                        .iter()
                        .filter_map(|q| {
                            parse_dep_atom(&q.atom, "or_preference")
                                .map(|at| (at.category.clone(), at.package.clone()))
                        })
                        .collect()
                }),
            )
        {
            all_available = false;
            circular = true;
            break;
        }
        let stripped = portage_dep::without_use(a);
        let extra_constraints = constraints_for(a);
        // USE settings never affect `||` preference evaluation at this stage.
        let avail = view.satisfiable(gate, stripped, config, extra_constraints)
            || root_deps_running_root
                .is_some_and(|running_root| running_root_satisfies_atom(a, running_root, config));
        if !avail {
            all_available = false;
            all_use_satisfied = false;
            break;
        }
        // No `[use]` block: reuse `avail` instead of re-probing the identical list.
        let use_ok = if stripped.len() == a.len() {
            true
        } else {
            view.use_satisfiable(probe, a, extra_constraints)
                // A *built* package's `[use]` deps match its recorded vdb USE, not the
                // tree candidate's profile USE; without this, installed-satisfied
                // alternatives are wrongly demoted.
                || root_deps_running_root
                    .is_some_and(|running_root| running_root_satisfies_atom(a, running_root, config))
        };
        if !use_ok {
            all_use_satisfied = false;
            // Only evaluated for an atom whose own USE-match actually failed.
            if !atom_all_use_unmasked(a, config, extra_constraints, gate) {
                all_use_unmasked = false;
            }
        }
    }
    if !all_available {
        if circular {
            return portage_use_reduce::AltPreference::Unsatisfiable;
        }
        // Reached when some atom has no tree candidate at all, even ignoring USE.
        // Blockers are excluded from the fold entirely, not merely satisfied.
        let non_blocker: Vec<&String> = atoms
            .iter()
            .filter(|a| {
                parse_dep_atom(a, "or_preference")
                    .is_some_and(|at| at.blocker == portage_dep::Blocker::None)
            })
            .collect();
        let all_installed = non_blocker
            .iter()
            .all(|a| view.matches_installed(root, a, config));
        if all_installed {
            return portage_use_reduce::AltPreference::OtherInstalled;
        }
        let some_installed = non_blocker
            .iter()
            .any(|a| view.matches_installed(root, a, config));
        if some_installed {
            return portage_use_reduce::AltPreference::OtherInstalledSome;
        }
        // Portage's own fuzzy cp-level fallback -- plain vdb match, deliberately NOT
        // `atom_cp_installed` (that helper's `virtual/` exemption doesn't apply here).
        let any_cp_installed = non_blocker.iter().any(|a| {
            parse_dep_atom(a, "or_preference")
                .is_some_and(|at| view.any_cp_installed(root, &at.category, &at.package))
        });
        return if any_cp_installed {
            portage_use_reduce::AltPreference::OtherInstalledAnySlot
        } else {
            portage_use_reduce::AltPreference::Other
        };
    }
    // An otherwise-`all_available` choice demoted straight to `other` on a downgrade
    // guard (see `alternative_downgrade_demoted`).
    if alternative_downgrade_demoted(AlternativeDowngradeDemotedArgs {
        root,
        config,
        entries,
        self_cp,
        constraints,
        atoms,
        queued,
        update,
        gate,
        view,
    }) {
        return portage_use_reduce::AltPreference::Other;
    }
    // Backtrack restart after a stranded walk: an alternative matching one of the
    // parent's recorded in-cycle children ranks `other`, breaking the cycle on re-walk.
    // Before USE binning (Portage demotes regardless of USE-satisfaction). Exemptions:
    // blockers never match; installed-satisfied atoms aren't circular. The
    // `parent.onlydeps` half stays cut: `--onlydeps` isn't threaded to this depth.
    if let Some(children) = circular_dependency.get(self_cp) {
        let circular_hit = atoms.iter().any(|a| {
            let Some(atom) = parse_dep_atom(a, "or_preference") else {
                return false;
            };
            if atom.blocker != portage_dep::Blocker::None {
                return false;
            }
            if view.matches_installed(root, a, config) {
                return false;
            }
            children.iter().any(|child| {
                let candidate = circular_child_candidate(child);
                portage_dep::match_from_list(a, &[candidate.as_str()])
                    .is_some_and(|m| !m.is_empty())
            })
        });
        if circular_hit {
            return portage_use_reduce::AltPreference::Other;
        }
    }
    if all_use_satisfied {
        // Choice bin 0: an alternative every non-blocker atom of which is either
        // already installed (cp-level) OR already a merge-bound node this run
        // (`[use]`-checked) ranks above one that would need a fresh merge.
        return if atoms.iter().all(|a| view.cp_installed(root, a))
            || view.all_in_graph(atoms, entries, config, queued)
        {
            portage_use_reduce::AltPreference::Installed
        } else {
            portage_use_reduce::AltPreference::Available
        };
    }
    // `!all_use_satisfied`: the alternative needs USE changes. If the flags to change
    // sit in `use.mask` / `use.force`, demote to plain `other`; otherwise keep the
    // `unsat_use_in_graph` > `unsat_use_installed` > `unsat_use_non_installed` order.
    if !all_use_unmasked {
        return portage_use_reduce::AltPreference::Other;
    }
    if view.all_in_graph(atoms, entries, config, queued) {
        return portage_use_reduce::AltPreference::UnsatUseInGraph;
    }
    // Cp-level installed is not enough: the *slot* the best USE-ignoring candidate
    // pulls in must also be installed, else `unsat_use_non_installed`.
    if atoms.iter().all(|a| view.cp_installed(root, a))
        && atoms
            .iter()
            .all(|a| view.installed_in_slot_of(probe, a, constraints_for(a)))
    {
        portage_use_reduce::AltPreference::UnsatUseInstalled
    } else {
        portage_use_reduce::AltPreference::UnsatUseNonInstalled
    }
}

/// The `version` field of one `highest_available_candidate_ignoring_use` candidate
/// string: the per-atom seed of an alternative's `cp_map`.
pub(crate) fn candidate_version(
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = candidate_str.strip_prefix(&format!("{category}/{package}-"))?;
    rest.split(':').next().map(str::to_string)
}

/// Arguments of [`promote_tied_alternative`].
pub(crate) struct PromoteTiedAlternativeArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) alts: &'a [Vec<String>],
    pub(crate) queued: &'a [QueueItem],
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) view: OrView<'a>,
}

/// Portage's in-bin upgrade-preference pass as `use_reduce_flat_disjunctive`'s
/// `tie_break` (2+ tied alternatives only). A literal remove+insert over an `id`-tagged
/// `Vec`, not a stable sort -- the promotion isn't transitive. Returns the WINNING
/// alternative's original index. Portage: dep_check.py, dep_zapdeps.
///
/// `minimize_slots` and `want_update` are pinned to Portage's default (`False`), which
/// only ever WIDENS the installed-slots promotion; and a same-`cp` repeat inside one
/// alternative always just keeps the higher version (see notes).
pub(crate) fn promote_tied_alternative(args: PromoteTiedAlternativeArgs<'_>) -> usize {
    let PromoteTiedAlternativeArgs {
        config,
        root,
        entries,
        constraints,
        alts,
        queued,
        gate,
        view,
    } = args;
    let probe = OrProbe { gate, root, config };
    struct Choice {
        id: usize,
        all_installed_slots: bool,
        all_in_graph: bool,
        cp_map: HashMap<String, String>,
    }
    let constraints_for = |a: &str| -> &[String] {
        parse_dep_atom(a, "or_preference")
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    let mut choices: Vec<Choice> = alts
        .iter()
        .enumerate()
        .map(|(id, atoms)| {
            let mut cp_map: HashMap<String, String> = HashMap::new();
            for a in atoms {
                let Some(atom) = parse_dep_atom(a, "or_preference") else {
                    continue;
                };
                if atom.blocker != portage_dep::Blocker::None {
                    continue;
                }
                let cp = format!("{}/{}", atom.category, atom.package);
                if let Some((version, _slot)) =
                    view.highest_available(probe, &atom, a, constraints_for(a))
                {
                    let better = cp_map
                        .get(&cp)
                        .is_none_or(|existing| vercmp_ordering(&version, existing).is_gt());
                    if better {
                        cp_map.insert(cp, version);
                    }
                }
            }
            Choice {
                id,
                all_installed_slots: atoms.iter().all(|a| view.cp_installed(root, a))
                    && atoms
                        .iter()
                        .all(|a| view.installed_in_slot_of(probe, a, constraints_for(a))),
                all_in_graph: view.all_in_graph(atoms, entries, config, queued),
                cp_map,
            }
        })
        .collect();

    if choices.len() < 2 {
        return 0;
    }
    // Snapshot of every choice BUT the first, by identity, before mutation.
    let snapshot_ids: Vec<usize> = choices[1..].iter().map(|c| c.id).collect();
    for id1 in snapshot_ids {
        let Some(pos1) = choices.iter().position(|c| c.id == id1) else {
            continue;
        };
        let choice1_all_installed_slots = choices[pos1].all_installed_slots;
        let choice1_all_in_graph = choices[pos1].all_in_graph;
        let choice1_cp_map = choices[pos1].cp_map.clone();
        let mut promote_before: Option<usize> = None;
        for (pos2, choice2) in choices.iter().enumerate() {
            if choice2.id == id1 {
                // Own position reached with no promotion: stop.
                break;
            }
            if choice1_all_installed_slots && !choice2.all_installed_slots {
                promote_before = Some(pos2);
                break;
            }
            let mut has_upgrade = false;
            let mut has_downgrade = false;
            for (cp, v1) in &choice1_cp_map {
                if let Some(v2) = choice2.cp_map.get(cp) {
                    if vercmp_ordering(v1, v2).is_gt() {
                        has_upgrade = true;
                    } else if vercmp_ordering(v1, v2).is_lt() {
                        has_downgrade = true;
                    }
                }
            }
            if (has_upgrade && !has_downgrade)
                || (choice1_all_in_graph
                    && !choice2.all_in_graph
                    && !(has_downgrade && !has_upgrade))
            {
                promote_before = Some(pos2);
                break;
            }
        }
        if let Some(pos2) = promote_before {
            // `pos2 < pos1` always, so removing `pos1` first never shifts `pos2`.
            let choice1 = choices.remove(pos1);
            choices.insert(pos2, choice1);
        }
    }
    choices[0].id
}

/// A dependency string's plain atoms and its `||` groups, USE already applied.
pub(crate) struct OrSplit {
    plain: HashSet<String>,
    /// Each `||` group's tokens, with the atoms of all its arms.
    groups: Vec<(Vec<String>, HashSet<String>)>,
}

/// Splits `depstr` into plain atoms and `||` groups. `None` when it does not parse.
pub(crate) fn split_or_groups(depstr: &str, use_flags: &HashSet<String>) -> Option<OrSplit> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let (plains, groups) = split_tokens(&tokens, use_flags)?;
    let plain = flat_dep_atoms(&plains.join(" "), use_flags)?;
    let groups = groups
        .into_iter()
        .map(|g| {
            let atoms = flat_dep_atoms(&g.join(" "), use_flags)?;
            Some((g, atoms))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(OrSplit { plain, groups })
}

impl OrSplit {
    /// The atoms of `||` arms that are not in `chosen` and not plain atoms elsewhere.
    pub(crate) fn arms_outside(&self, chosen: &HashSet<String>) -> HashSet<String> {
        self.groups
            .iter()
            .flat_map(|(_, atoms)| atoms.iter())
            .filter(|a| !chosen.contains(*a) && !self.plain.contains(*a))
            .cloned()
            .collect()
    }

    /// Whether the string holds no `||` group.
    pub(crate) fn is_flat(&self) -> bool {
        self.groups.is_empty()
    }

    /// Every plain atom of the string.
    pub(crate) fn plain_atoms(&self) -> &HashSet<String> {
        &self.plain
    }
}

/// Arguments of [`installed_consumer_or_selection`].
pub(crate) struct InstalledConsumerOrSelectionArgs<'a> {
    pub(crate) split: &'a OrSplit,
    pub(crate) use_flags: &'a HashSet<String>,
    /// The consumer's `(category, package)`.
    pub(crate) consumer: &'a (String, String),
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) root: &'a Path,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) update: bool,
    pub(crate) root_deps_running_root: Option<&'a Path>,
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) view: OrView<'a>,
}

/// The atoms `dep_zapdeps` selects from each `||` group of an installed consumer's
/// dependency string, judged against `view`. A group that fails to flatten selects
/// every arm.
// Portage: dep_check.py, dep_zapdeps
pub(crate) fn installed_consumer_or_selection(
    args: InstalledConsumerOrSelectionArgs<'_>,
) -> HashSet<String> {
    let InstalledConsumerOrSelectionArgs {
        split,
        use_flags,
        consumer,
        config,
        root,
        entries,
        update,
        root_deps_running_root,
        gate,
        view,
    } = args;
    let no_constraints: HashMap<(String, String), Vec<String>> = HashMap::new();
    let no_circular: HashMap<RootCp, Vec<CircularDepChild>> = HashMap::new();
    let self_node: RootCp = (false, consumer.0.clone(), consumer.1.clone());
    let mut selected: HashSet<String> = HashSet::new();
    for (group, arms) in &split.groups {
        let flat = portage_use_reduce::use_reduce_flat_disjunctive(
            portage_use_reduce::UseReduceFlatDisjunctiveArgs {
                tokens: group,
                uselist: use_flags,
                mode: portage_use_reduce::MatchMode::Normal,
                empty_groups_always_true: false,
                tie_break: &mut |alts: &[Vec<String>]| {
                    promote_tied_alternative(PromoteTiedAlternativeArgs {
                        config,
                        root,
                        entries,
                        constraints: &no_constraints,
                        alts,
                        queued: &[],
                        gate,
                        view,
                    })
                },
            },
            &mut |atoms: &[String]| {
                disjunction_preference(DisjunctionPreferenceArgs {
                    config,
                    root,
                    entries,
                    self_cp: &self_node,
                    constraints: &no_constraints,
                    circular_dependency: &no_circular,
                    root_deps_running_root,
                    atoms,
                    queued: &[],
                    update,
                    gate,
                    gate_entries: entries,
                    gate_queued: &[],
                    view,
                })
            },
        );
        match flat {
            Ok(chosen) => selected.extend(chosen),
            Err(_) => selected.extend(arms.iter().cloned()),
        }
    }
    selected
}
