//! The unsatisfied slot-operator dependency probe.

use crate::*;

/// Arguments of [`slot_operator_unsatisfied_probe`].
#[cfg(test)]
pub(crate) struct SlotOperatorUnsatisfiedProbeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) slot_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
}

/// Post-pass trigger for an unsatisfied built slot-operator dep: each
/// `NoVisibleCandidate` entry whose targeting atoms include one seeds its
/// installed owner into the replace set, and the driver restarts.
/// Backtrack-gated at the call site.
// Portage: _emerge/depgraph.py, _slot_operator_unsatisfied_probe
#[cfg(test)]
pub(crate) fn slot_operator_unsatisfied_probe(
    args: SlotOperatorUnsatisfiedProbeArgs<'_>,
) -> BTreeSet<(String, String)> {
    let SlotOperatorUnsatisfiedProbeArgs {
        root,
        repos,
        entries,
        slot_want,
        pullers,
        masked,
        with_bdeps,
        excluded,
        config,
    } = args;
    slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
        root,
        repos,
        entries,
        slot_want,
        pullers,
        masked,
        with_bdeps,
        excluded,
        config,
    })
    .0
}

/// [`slot_operator_unsatisfied_probe_full`]'s result: the installed
/// parents to reinstall and the `(category, package, version)` binary
/// parents to mask.
pub(crate) type SlotOpUnsatProbe = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// Arguments of [`slot_operator_unsatisfied_probe_full`].
pub(crate) struct SlotOperatorUnsatisfiedProbeFullArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) slot_want: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) pullers: &'a SlotPullers,
    pub(crate) masked: &'a HashMap<(String, String), Vec<MaskEntry>>,
    pub(crate) with_bdeps: bool,
    pub(crate) excluded: &'a [String],
    pub(crate) config: &'a portage_profile::Config,
}

/// [`slot_operator_unsatisfied_probe`] plus the binary arm: a merge-bound
/// binary parent is masked (`slot_operator_mask_built`) instead of seeded.
// Portage: _emerge/depgraph.py, _slot_operator_unsatisfied_backtrack
pub(crate) fn slot_operator_unsatisfied_probe_full(
    args: SlotOperatorUnsatisfiedProbeFullArgs<'_>,
) -> SlotOpUnsatProbe {
    let SlotOperatorUnsatisfiedProbeFullArgs {
        root,
        repos,
        entries,
        slot_want,
        pullers,
        masked,
        with_bdeps,
        excluded,
        config,
    } = args;
    const RUNTIME_KEYS: [&str; 3] = ["RDEPEND", "IDEPEND", "PDEPEND"];
    const BUILD_KEYS: [&str; 2] = ["DEPEND", "BDEPEND"];
    // A `!=cp-ver` negative screens only that version, and a
    // `[binary]`-marked negative only ever screens the binary instance.
    let masked_version = |cp: &(String, String), version: &str| -> bool {
        masked.get(cp).is_some_and(|bucket| {
            bucket.iter().any(|entry| {
                if is_binary_only_mask(&entry.neg) {
                    return false;
                }
                // An `[installed]`-marked negative screens the installed
                // instance itself, or the restart would loop.
                if slot_operator_installed_masked_version(&entry.neg, &cp.0, &cp.1)
                    .is_some_and(|v| v == version)
                {
                    return true;
                }
                entry
                    .neg
                    .strip_prefix("!=")
                    .and_then(split_cpv)
                    .is_some_and(|(c, p, v)| c == cp.0 && p == cp.1 && v == version)
            })
        })
    };
    let mut seeds: BTreeSet<(String, String)> = BTreeSet::new();
    let mut binary_masks: BTreeSet<(String, String, String)> = BTreeSet::new();
    for e in entries {
        if !matches!(e.outcome, PretendOutcome::NoVisibleCandidate) {
            continue;
        }
        let dep_cp = (e.category.clone(), e.package.clone());
        let Some(wants) = slot_want.get(&dep_cp) else {
            continue;
        };
        // With no built-form want on this cp there is no edge to probe.
        if !wants.iter().any(|atom_str| {
            portage_dep::parse_atom(atom_str).is_some_and(|atom| {
                atom.blocker == portage_dep::Blocker::None && is_built_slot_op(&atom)
            })
        }) {
            continue;
        }
        for owner in &e.required_by {
            // Seed only an owner whose own recorded want is the built atom,
            // never the cp-level cross product. The puller triples are
            // recorded after USE evaluation, so a conditional built want
            // that evaluates false is absent here.
            let paired = pullers.get(&dep_cp).is_some_and(|triples| {
                triples.iter().any(|(pc, pp, _, atom_text)| {
                    pc == &owner.0
                        && pp == &owner.1
                        && portage_dep::parse_atom(atom_text).is_some_and(|atom| {
                            atom.blocker == portage_dep::Blocker::None
                                && is_built_slot_op(&atom)
                                && atom.category == dep_cp.0
                                && atom.package == dep_cp.1
                        })
                })
            });
            if !paired {
                continue;
            }
            // The installed parent, else a merge-bound binary parent.
            let mut installed: Vec<(String, String, bool)> =
                installed_candidates(root, &owner.0, &owner.1)
                    .into_iter()
                    .map(|(v, slot, _)| (v, slot, false))
                    .collect();
            if installed.is_empty() {
                let binary = entries.iter().find_map(|x| {
                    if x.category != owner.0
                        || x.package != owner.1
                        || x.source != CandidateSource::Binary
                    {
                        return None;
                    }
                    let v = merge_bound_version(&x.outcome)?;
                    Some((
                        v.clone(),
                        x.slot.clone().unwrap_or_else(|| "0".to_string()),
                        true,
                    ))
                });
                match binary {
                    Some(b) => installed.push(b),
                    None => continue,
                }
            }
            // An installed parent matching `--exclude` refuses the probe.
            let excluded_parent = installed.iter().any(|(v, _, _)| {
                let q_str = format!("{}/{}-{v}", owner.0, owner.1);
                excluded
                    .iter()
                    .any(|ex| matches_cli_option_atom(ex, &q_str))
            });
            if excluded_parent {
                continue;
            }
            for (installed_version, parent_slot, is_binary) in &installed {
                // An instance already in `runtime_pkg_mask` would
                // restart-loop; for a binary parent, its own `[binary]`
                // mask is that instance.
                let binary_neg = format!("!={}/{}-{installed_version}[binary]", owner.0, owner.1);
                if (!is_binary && masked_version(owner, installed_version))
                    || (*is_binary
                        && masked
                            .get(owner)
                            .is_some_and(|b| b.iter().any(|m| m.neg == binary_neg)))
                {
                    continue;
                }
                let Ok(tree) = list_candidates(repos, &owner.0, &owner.1) else {
                    continue;
                };
                let mut hit = false;
                for cand in tree.iter().filter(|c| {
                    c.source == CandidateSource::Ebuild
                        && c.slot == *parent_slot
                        && is_visible(c, &owner.0, &owner.1, config)
                        && !masked_version(owner, &c.version)
                }) {
                    let cand_str = format!(
                        "{}/{}-{}:{}/{}::{}",
                        owner.0, owner.1, cand.version, cand.slot, cand.sub_slot, cand.repo_name
                    );
                    if excluded
                        .iter()
                        .any(|ex| matches_cli_option_atom(ex, &cand_str))
                    {
                        continue;
                    }
                    let pf = format!("{}-{}", owner.1, cand.version);
                    let Ok(meta) = repo_aux_metadata(&cand.repo_location, &owner.0, &pf) else {
                        continue;
                    };
                    let mut keys: Vec<&str> = RUNTIME_KEYS.to_vec();
                    if with_bdeps {
                        keys.extend(BUILD_KEYS);
                    }
                    for key in keys {
                        // The replacement's live atoms: every conditional
                        // branch kept, regardless of the replacement's USE.
                        let toks: Vec<String> = meta
                            .get(key)
                            .map(String::as_str)
                            .unwrap_or_default()
                            .split_whitespace()
                            .map(String::from)
                            .collect();
                        let Ok(tokens) = portage_use_reduce::use_reduce_flat(
                            &toks,
                            &HashSet::new(),
                            portage_use_reduce::MatchMode::All,
                        ) else {
                            continue;
                        };
                        for token in tokens {
                            let Some(live) = portage_dep::parse_atom(&token) else {
                                continue;
                            };
                            // A `=`-operator, non-blocker atom on the dep's
                            // own cp.
                            if live.slot_operator != Some(portage_dep::SlotOperator::Equals)
                                || live.blocker != portage_dep::Blocker::None
                                || live.category != dep_cp.0
                                || live.package != dep_cp.1
                            {
                                continue;
                            }
                            // USE discarded, then visible tree or installed.
                            let stripped = portage_dep::without_use(&token).to_string();
                            if !visible_tree_matches(repos, &stripped, config, &[]).is_empty()
                                || best_installed_for_atom(root, &stripped, &dep_cp.0, &dep_cp.1)
                                    .is_some()
                            {
                                hit = true;
                                break;
                            }
                        }
                        if hit {
                            break;
                        }
                    }
                    if hit {
                        break;
                    }
                }
                if hit {
                    if *is_binary {
                        binary_masks.insert((
                            owner.0.clone(),
                            owner.1.clone(),
                            installed_version.clone(),
                        ));
                    } else {
                        seeds.insert(owner.clone());
                    }
                    break;
                }
            }
        }
    }
    (seeds, binary_masks)
}
