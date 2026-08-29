//! Which alternative of an `||` group to prefer.

use crate::*;

/// Arguments of [`alternative_downgrade_demoted`].
pub(crate) struct AlternativeDowngradeDemotedArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) self_cp: &'a RootCp,
    pub(crate) constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) atoms: &'a [String],
    pub(crate) queued: &'a [QueueItem],
    pub(crate) update: bool,
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
        repos,
        root,
        config,
        entries,
        self_cp,
        constraints,
        atoms,
        queued,
        update,
    } = args;
    atoms.iter().any(|a| {
        let Some(at) = portage_dep::parse_atom(a) else {
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
        let Some((avail_version, avail_slot)) = visible_tree_matches(repos, a, config, extra)
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
                let Some(qat) = portage_dep::parse_atom(&q.atom) else {
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
                        visible_tree_matches(repos, &q.atom, config, qextra)
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
        !downgrade_probe(repos, config, &slot_atom, &avail_version, extra)
    })
}

/// Arguments of [`disjunction_preference`].
pub(crate) struct DisjunctionPreferenceArgs<'a> {
    pub(crate) repos: &'a [RepoConfig],
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
    /// The whole-graph views for the circular-self-dep gate below. `entries`/`queued`
    /// above are the group's own root scope; the gate asks whether the owner's cp is in
    /// play *at all*, so it reads these unscoped views instead. Same slices on every
    /// same-root call -- a strict no-op there.
    pub(crate) gate_entries: &'a [GraphEntry],
    pub(crate) gate_queued: &'a [QueueItem],
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
        repos,
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
        gate_entries,
        gate_queued,
    } = args;
    let constraints_for = |a: &str| -> &[String] {
        portage_dep::parse_atom(a)
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
        let self_naming = portage_dep::parse_atom(a).is_some_and(|at| {
            at.blocker == portage_dep::Blocker::None
                && at.category == self_cp.1
                && at.package == self_cp.2
        });
        if self_naming
            && !atom_cp_installed(root, a)
            && !atom_cp_present_in_graph(
                a,
                gate_entries,
                config,
                queued_cps.get_or_insert_with(|| {
                    gate_queued
                        .iter()
                        .filter_map(|q| {
                            portage_dep::parse_atom(&q.atom)
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
        let avail = atom_currently_satisfiable(repos, stripped, config, extra_constraints)
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
            atom_currently_satisfiable(repos, a, config, extra_constraints)
                // A *built* package's `[use]` deps match its recorded vdb USE, not the
                // tree candidate's profile USE; without this, installed-satisfied
                // alternatives are wrongly demoted.
                || atom_matches_installed(root, a, config)
                || root_deps_running_root
                    .is_some_and(|running_root| running_root_satisfies_atom(a, running_root, config))
        };
        if !use_ok {
            all_use_satisfied = false;
            // Only evaluated for an atom whose own USE-match actually failed.
            if !atom_all_use_unmasked(repos, a, config, extra_constraints) {
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
                portage_dep::parse_atom(a)
                    .is_some_and(|at| at.blocker == portage_dep::Blocker::None)
            })
            .collect();
        let all_installed = non_blocker
            .iter()
            .all(|a| atom_matches_installed(root, a, config));
        if all_installed {
            return portage_use_reduce::AltPreference::OtherInstalled;
        }
        let some_installed = non_blocker
            .iter()
            .any(|a| atom_matches_installed(root, a, config));
        if some_installed {
            return portage_use_reduce::AltPreference::OtherInstalledSome;
        }
        // Portage's own fuzzy cp-level fallback -- plain vdb match, deliberately NOT
        // `atom_cp_installed` (that helper's `virtual/` exemption doesn't apply here).
        let any_cp_installed = non_blocker.iter().any(|a| {
            portage_dep::parse_atom(a)
                .is_some_and(|at| !installed_candidates(root, &at.category, &at.package).is_empty())
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
        repos,
        root,
        config,
        entries,
        self_cp,
        constraints,
        atoms,
        queued,
        update,
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
            let Some(atom) = portage_dep::parse_atom(a) else {
                return false;
            };
            if atom.blocker != portage_dep::Blocker::None {
                return false;
            }
            if atom_matches_installed(root, a, config) {
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
        return if atoms.iter().all(|a| atom_cp_installed(root, a))
            || atoms_all_in_graph(atoms, entries, config, queued)
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
    if atoms_all_in_graph(atoms, entries, config, queued) {
        return portage_use_reduce::AltPreference::UnsatUseInGraph;
    }
    // Cp-level installed is not enough: the *slot* the best USE-ignoring candidate
    // pulls in must also be installed, else `unsat_use_non_installed`.
    if atoms.iter().all(|a| atom_cp_installed(root, a))
        && atoms
            .iter()
            .all(|a| atom_installed_in_slot_of(root, repos, config, a, constraints_for(a)))
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
