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

/// The higher-in-slot node whose presence demotes an `||` arm under the
/// bug-531656 downgrade guard: the arm's best candidate is older than what
/// the graph already holds in its slot.
#[derive(Debug)]
pub(crate) struct DowngradeDemoter {
    pub(crate) category: String,
    pub(crate) package: String,
    // Trace identity only (read by the `Debug` seed-decision log, which dead-code
    // analysis ignores); the contest check is slot-scoped by design.
    #[allow(dead_code)]
    pub(crate) version: String,
    pub(crate) slot: String,
}

/// Every higher-in-slot node demoting one of `atoms` (one per demoted atom).
/// [`alternative_downgrade_demoted`] is the boolean projection.
pub(crate) fn downgrade_demoters(
    args: AlternativeDowngradeDemotedArgs<'_>,
) -> Vec<DowngradeDemoter> {
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
    atoms
        .iter()
        .filter_map(|a| {
            let at = parse_dep_atom(a, "or_preference")?;
            if at.blocker != portage_dep::Blocker::None
                || (at.category == self_cp.1 && at.package == self_cp.2)
            {
                return None;
            }
            let extra = constraints
                .get(&(at.category.clone(), at.package.clone()))
                .map_or(&[][..], Vec::as_slice);
            if let OrView::PostMerge(set) = view {
                // The set holds the graph: the best member is `avail_pkg`, the highest
                // member of its slot is `highest_in_slot`.
                let (avail_version, avail_slot) = set.best_matching(a)?;
                let highest = set.highest_in_slot(&at.category, &at.package, &avail_slot)?;
                if vercmp_ordering(&avail_version, &highest) != Ordering::Less {
                    return None;
                }
                let slot_atom = format!("{}/{}:{avail_slot}", at.category, at.package);
                if downgrade_probe(gate, config, &slot_atom, &avail_version, extra) {
                    return None;
                }
                return Some(DowngradeDemoter {
                    category: at.category.clone(),
                    package: at.package.clone(),
                    version: highest,
                    slot: avail_slot,
                });
            }
            let (avail_version, avail_slot) = visible_tree_matches(gate, a, config, extra)
                .into_iter()
                .max_by(|x, y| vercmp_ordering(&x.0, &y.0))?;
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
            let highest_in_graph = in_slot.iter().max_by(|x, y| vercmp_ordering(x, y))?;
            if vercmp_ordering(&avail_version, highest_in_graph) != Ordering::Less {
                return None;
            }
            let slot_atom = format!("{}/{}:{avail_slot}", at.category, at.package);
            // Both downgrade guards share one test once `highest_in_slot` is a graph node:
            // lower than what the graph holds, and not a desirable downgrade.
            if downgrade_probe(gate, config, &slot_atom, &avail_version, extra) {
                return None;
            }
            Some(DowngradeDemoter {
                category: at.category.clone(),
                package: at.package.clone(),
                version: highest_in_graph.clone(),
                slot: avail_slot,
            })
        })
        .collect()
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
    !downgrade_demoters(args).is_empty()
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

/// Arguments of [`alternative_want_update`].
pub(crate) struct AlternativeWantUpdateArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) parent: Option<(String, String)>,
    pub(crate) depth: u32,
    pub(crate) deep: Deep,
    pub(crate) update: bool,
    pub(crate) arg_atoms: &'a [String],
    pub(crate) set_args: &'a HashSet<String>,
    pub(crate) excluded: &'a [String],
    pub(crate) complete: bool,
    pub(crate) atoms: &'a [String],
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// Portage's per-choice `want_update`: whether any available package of the
/// choice wants an update for `parent` (dep_check.py `dep_zapdeps`, gated at
/// :771-775). Per package (depgraph.py `_want_update_pkg`): not complete
/// mode, not `--exclude`d, an argument match (depth reset to 0) or
/// `--update`, and not `_too_deep` at the effective depth.
///
/// The argument leg is candidate-based with the higher-slot guard
/// (depgraph.py `_iter_atoms_for_pkg`): an argument matching the candidate is
/// ignored while a higher visible version sits in another slot. `PackageArg`
/// vs `SetArg` (`reset_depth=False` internal sets, the identity skip, the
/// virtual-cp skip) has no portuale counterpart, so every argument counts and
/// resets the depth; with `--update` the leg is dead anyway (the `-uDN`
/// `@world` path only needs the update leg).
// Portage: dep_check.py:771-775; depgraph.py:7332-7362, 7387-7403, 4988-5012
pub(crate) fn alternative_want_update(args: AlternativeWantUpdateArgs<'_>) -> bool {
    let AlternativeWantUpdateArgs {
        config,
        gate,
        parent,
        depth,
        deep,
        update,
        arg_atoms,
        set_args,
        excluded,
        complete,
        atoms,
        constraints,
    } = args;
    // No parent (installed walk, blocker scan, PostMerge view): never wanted.
    if parent.is_none() {
        return false;
    }
    if complete {
        return false;
    }
    let extra_for = |a: &str| -> &[String] {
        parse_dep_atom(a, "or_preference")
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    for a in atoms {
        let Some(atom) = parse_dep_atom(a, "or_preference") else {
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        // The choice's available package for this atom (USE ignored, like
        // Portage's `avail_pkg`).
        let Some(candidate) = best_available_candidate(BestAvailableCandidateArgs {
            gate,
            config,
            atom_str: a,
            category: &atom.category,
            package: &atom.package,
            extra_constraints: extra_for(a),
        }) else {
            continue;
        };
        let candidate_str = candidate.match_str(&atom.category, &atom.package);
        // `--exclude` opts this package out.
        if excluded
            .iter()
            .any(|ex| matches_cli_option_atom(ex, &candidate_str))
        {
            continue;
        }
        if update {
            if !update_too_deep(deep, depth) {
                return true;
            }
        } else if candidate_matches_argument(CandidateMatchesArgumentArgs {
            gate,
            config,
            arg_atoms,
            set_args,
            category: &atom.category,
            package: &atom.package,
            candidate: &candidate,
            candidate_str: &candidate_str,
        }) {
            // An argument match resets the depth to 0, which is never too deep.
            return true;
        }
    }
    false
}

/// Arguments of [`best_available_candidate`].
struct BestAvailableCandidateArgs<'a> {
    gate: CategoryGate<'a>,
    config: &'a portage_profile::Config,
    atom_str: &'a str,
    category: &'a str,
    package: &'a str,
    extra_constraints: &'a [String],
}

/// The highest visible candidate matching `atom_str` (USE ignored), cloned
/// for the slot-aware checks. Mirrors
/// [`highest_available_candidate_ignoring_use`]'s pool and mask.
fn best_available_candidate(args: BestAvailableCandidateArgs<'_>) -> Option<Candidate> {
    let BestAvailableCandidateArgs {
        gate,
        config,
        atom_str,
        category,
        package,
        extra_constraints,
    } = args;
    let stripped = portage_dep::without_use(atom_str);
    let pool = gate.repo_candidates(category, package).ok()?;
    let visible: Vec<&Candidate> = pool
        .iter()
        .filter(|c| is_visible(c, category, package, config))
        .collect();
    if visible.is_empty() {
        return None;
    }
    let candidate_strs: Vec<String> = visible
        .iter()
        .map(|c| c.match_str(category, package))
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(stripped, &candidate_str_refs)?;
    let matched: Vec<&str> = if extra_constraints.is_empty() {
        matched
    } else {
        matched
            .into_iter()
            .filter(|m| {
                extra_constraints.iter().all(|c| {
                    if is_instance_only_mask(c) {
                        return true;
                    }
                    if let Some(neg) = c.strip_prefix('!') {
                        !portage_dep::match_from_list(neg, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    } else {
                        portage_dep::match_from_list(c, std::slice::from_ref(m))
                            .is_some_and(|r| !r.is_empty())
                    }
                })
            })
            .collect()
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in candidate_str_refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
        .cloned()
}

/// Arguments of [`candidate_matches_argument`].
struct CandidateMatchesArgumentArgs<'a, 'b> {
    gate: CategoryGate<'a>,
    config: &'a portage_profile::Config,
    arg_atoms: &'a [String],
    set_args: &'a HashSet<String>,
    category: &'a str,
    package: &'a str,
    candidate: &'b Candidate,
    candidate_str: &'a str,
}

/// Whether the available `candidate` matches a top-level argument, with
/// Portage's higher-slot guard: an argument is ignored while a higher visible
/// version sits in another slot. Every argument counts and resets the depth
/// (see [`alternative_want_update`]).
// Portage: depgraph.py, _iter_atoms_for_pkg
fn candidate_matches_argument(args: CandidateMatchesArgumentArgs<'_, '_>) -> bool {
    let CandidateMatchesArgumentArgs {
        gate,
        config,
        arg_atoms,
        set_args,
        category,
        package,
        candidate,
        candidate_str,
    } = args;
    for arg_text in arg_atoms.iter().chain(set_args.iter()) {
        let Some(arg) = parse_dep_atom(arg_text, "or_preference") else {
            continue;
        };
        if arg.category != category || arg.package != package {
            continue;
        }
        // Higher-slot guard: a higher visible version in another slot
        // suppresses this argument.
        if higher_visible_in_other_slot(gate, config, arg_text, candidate) {
            continue;
        }
        if portage_dep::match_from_list(arg_text, std::slice::from_ref(&candidate_str))
            .is_some_and(|m| !m.is_empty())
        {
            return true;
        }
    }
    false
}

/// Whether a higher visible version of the candidate's package sits in another
/// slot (Portage `_iter_atoms_for_pkg`'s `higher_slot` guard, which matches
/// the argument atom's own visible list in descending order).
fn higher_visible_in_other_slot(
    gate: CategoryGate<'_>,
    config: &portage_profile::Config,
    arg_text: &str,
    candidate: &Candidate,
) -> bool {
    let Some(arg) = parse_dep_atom(arg_text, "or_preference") else {
        return false;
    };
    let Ok(pool) = gate.repo_candidates(&arg.category, &arg.package) else {
        return false;
    };
    let visible: Vec<&Candidate> = pool
        .iter()
        .filter(|c| is_visible(c, &arg.category, &arg.package, config))
        .collect();
    if visible.is_empty() {
        return false;
    }
    let strs: Vec<String> = visible
        .iter()
        .map(|c| c.match_str(&arg.category, &arg.package))
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(arg_text, &refs) else {
        return false;
    };
    let mut by_str: HashMap<&str, &Candidate> = HashMap::new();
    for (s, c) in refs.iter().zip(visible.iter()) {
        by_str.insert(*s, *c);
    }
    let mut higher: Vec<&Candidate> = matched
        .iter()
        .filter_map(|m| by_str.get(m).copied())
        .filter(|c| vercmp_ordering(&c.version, &candidate.version).is_gt())
        .collect();
    higher.sort_by(|a, b| vercmp_ordering(&b.version, &a.version));
    higher.into_iter().any(|c| {
        (c.slot.as_str(), c.sub_slot.as_str())
            != (candidate.slot.as_str(), candidate.sub_slot.as_str())
    })
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
    /// The `||` owner's `(category, package)`: `None` pins every choice's
    /// `want_update` to `false` (installed/PostMerge/blocker callers never
    /// want an update). `Some` only at merge-bound `Normal`-view sites.
    pub(crate) parent: Option<(String, String)>,
    /// The effective child depth (`parent.depth + 1`), already resolved by
    /// the caller. Read only when `parent` is `Some`.
    pub(crate) depth: u32,
    /// The run's `--deep` setting, for `_too_deep`.
    pub(crate) deep: Deep,
    /// The run's `--update` flag.
    pub(crate) update: bool,
    /// The run's top-level atom texts plus set members, for the
    /// `_iter_atoms_for_pkg` argument match (a match resets depth to 0).
    pub(crate) arg_atoms: &'a [String],
    pub(crate) set_args: &'a HashSet<String>,
    /// The run's `--exclude` atoms: an excluded package never wants update.
    pub(crate) excluded: &'a [String],
    /// Portage's `_complete_mode`: nothing is ever wanted for update in it.
    pub(crate) complete: bool,
}

/// Portage's in-bin upgrade-preference pass as `use_reduce_flat_disjunctive`'s
/// `tie_break` (2+ tied alternatives only). A literal remove+insert over an `id`-tagged
/// `Vec`, not a stable sort -- the promotion isn't transitive. Returns the WINNING
/// alternative's original index. Portage: dep_check.py, dep_zapdeps.
///
/// `minimize_slots` stays pinned to Portage's default (`False`), which only ever
/// WIDENS the installed-slots promotion; a same-`cp` repeat inside one
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
        parent,
        depth,
        deep,
        update,
        arg_atoms,
        set_args,
        excluded,
        complete,
    } = args;
    let probe = OrProbe { gate, root, config };
    struct Choice {
        id: usize,
        all_installed_slots: bool,
        all_in_graph: bool,
        cp_map: HashMap<String, String>,
        // Portage's per-choice `want_update` (dep_check.py:771-775), computed
        // lazily below: only the installed-slots arm reads it, and single
        // alternatives never promote.
        want_update: Option<bool>,
    }
    let constraints_for = |a: &str| -> &[String] {
        parse_dep_atom(a, "or_preference")
            .and_then(|at| constraints.get(&(at.category, at.package)))
            .map_or(&[][..], Vec::as_slice)
    };
    // Single-alternative groups never promote: return before the per-choice
    // scans (`best_available_candidate` repo walks).
    if alts.len() < 2 {
        return 0;
    }
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
                want_update: None,
            }
        })
        .collect();
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
        for (pos2, choice2) in choices.iter_mut().enumerate() {
            if choice2.id == id1 {
                // Own position reached with no promotion: stop.
                break;
            }
            // `None` parents (installed/PostMerge/blocker sites) never want
            // an update, exactly like before.
            if choice1_all_installed_slots && !choice2.all_installed_slots {
                if choice2.want_update.is_none() {
                    let id2 = choice2.id;
                    choice2.want_update =
                        Some(alternative_want_update(AlternativeWantUpdateArgs {
                            config,
                            gate,
                            parent: parent.clone(),
                            depth,
                            deep,
                            update,
                            arg_atoms,
                            set_args,
                            excluded,
                            complete,
                            atoms: &alts[id2],
                            constraints,
                        }));
                }
                // Portage: dep_check.py (the installed-slots promotion is
                // skipped when the staying choice wants an update).
                if !choice2.want_update.is_some_and(|w| w) {
                    promote_before = Some(pos2);
                    break;
                }
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

    /// Each `||` group's tokens with the atoms of all its arms.
    pub(crate) fn groups(&self) -> &[(Vec<String>, HashSet<String>)] {
        &self.groups
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
    /// The `||` owner's `(category, package)` for the upgrade-preference
    /// promotion: `None` pins it off (installed/PostMerge callers). `Some`
    /// only on the merge-bound `Normal`-view path, with the caller's
    /// already-resolved effective child depth and run flags.
    pub(crate) want_parent: Option<(String, String)>,
    pub(crate) want_depth: u32,
    pub(crate) want_deep: Deep,
    pub(crate) want_arg_atoms: &'a [String],
    pub(crate) want_set_args: Option<&'a HashSet<String>>,
    pub(crate) want_excluded: &'a [String],
    pub(crate) want_complete: bool,
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
        want_parent,
        want_depth,
        want_deep,
        want_arg_atoms,
        want_set_args,
        want_excluded,
        want_complete,
    } = args;
    let no_constraints: HashMap<(String, String), Vec<String>> = HashMap::new();
    let no_set_args: HashSet<String> = HashSet::new();
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
                        // Only the merge-bound leak path fills these in (see
                        // rev_dep_or.rs); every other caller pins `None`.
                        parent: want_parent.clone(),
                        depth: want_depth,
                        deep: want_deep,
                        update,
                        arg_atoms: want_arg_atoms,
                        set_args: want_set_args.unwrap_or(&no_set_args),
                        excluded: want_excluded,
                        complete: want_complete,
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
