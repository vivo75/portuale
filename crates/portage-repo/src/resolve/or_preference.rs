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
