//! The unsatisfied slot-operator dependency probe.

use crate::*;

/// Backlog #215 (v2 `#24f`): real `_slot_operator_unsatisfied_probe`
/// (`_emerge/depgraph.py:2817`) as a post-pass trigger. Real fires it
/// in-walk (`:3447-3458`) when a built slot-operator dep (`dep.atom`
/// with `slot_operator_built`, e.g. the vdb-recorded
/// `>=app-misc/sousatprov-1:0/1=`) goes unsatisfied: for each
/// `replacement_parent` -- a visible, non-installed, non-excluded
/// package in the parent's own slot (`_iter_similar_available`, `:3015`)
/// -- every live `:=` atom (`atom.slot_operator == "="`, `:2835-2839`,
/// not a blocker, same cp) is tried with USE deps discarded
/// (`atom.without_use`, `:2843`) through `_select_package` (`:2845`);
/// the first hit returns true and `_slot_operator_unsatisfied_backtrack`
/// (`:2881`) writes `slot_operator_replace_installed` (for an installed
/// parent, via `_replace_installed_atom`, `:2908-2911`) plus
/// `_need_restart`. The non-installed-parent `slot_operator_mask_built`
/// arm (`:2901-2903`) is a cut of its own -- deliberately not ported
/// here, and not covered by #212 (whose slot-change probe fires on a
/// different trigger: a merge-bound binary's shifted built dep, real
/// `:2381-2398`): a non-installed built parent (a binary package whose
/// recorded `:=` dep is unsatisfied) has no unsatisfied-probe path at
/// all (residue #257). The installed arm below is this probe.
///
/// Portuale has no in-walk backtrack for this shape, so the probe runs
/// here, on a dead-end pass, next to the #211/#212 post-pass trigger:
/// each `NoVisibleCandidate` entry whose targeting atoms include an
/// unsatisfied built slot-operator atom from an installed owner seeds
/// that owner into the replace set, and the driver restarts with the
/// same `BacktrackFeedback::Config` the scan uses (real's
/// `backtrack_infos["config"]` + `_need_restart`). The seed grows the
/// set monotonically, so the search terminates; a pass that heals
/// settles, one that finds no replacement stays a dead end.
///
/// Gates and cuts, all real-grounded:
/// - backtrack-gated at the call site (`ctx.backtrack_max > 0`), like
///   real's `_allow_backtracking` gate (`:3447`) and the scan's own bt0
///   gate: under `--backtrack=0` real fails the run (`emerge: there are
///   no ebuilds to satisfy ...`), and so does this port.
/// - `dep.atom.package` (real `:3449`) is vacuous: parsed atoms always
///   carry a package.
/// - the parent must be installed (real `dep.parent.installed`) and
///   the probed installed instance must not sit in `runtime_pkg_mask`
///   already (real `:3447-3448`, per package instance, else the restart
///   would loop); an installed parent matching `--exclude` refuses
///   (real `:2818-2824`).
/// - the replacement search is same-slot (real passes
///   `dep.parent.slot_atom`), visible (`is_visible`, standing in for
///   `_pkg_visibility_check`), non-excluded and unmasked per instance
///   (real `:3036-3040` skips `pkg in _runtime_pkg_mask` then the
///   excluded check), ebuild-only (installed instances never seed a
///   reinstall).
/// - `onlydeps` (real `:2844-2845`, threading `dep.onlydeps` into
///   `_select_package`) is ignored: the re-resolve passes `&[]`.
/// - the replacement's live dep keys are runtime keys always,
///   `DEPEND`/`BDEPEND` only `with_bdeps` (the scan's own key
///   discipline; real's `validated_atoms` spans every key).
/// - the replacement's atoms are reduced with every conditional branch
///   active, real's `validated_atoms` (`use_reduce(..., matchall=True)`,
///   `_emerge/Package.py:329-359`).
/// - no `check_reverse_dependencies` refusal: real's unsatisfied probe
///   has none (unlike the #211 update probe) -- a vetoing sibling does
///   not withhold the reinstall (S0 veto cell).
/// - same-version-first seeding (`_replace_installed_atom`'s `=cpv`
///   half) is a cp-level seed here like the scan's: with a single
///   available parent version the two coincide; a newer-version
///   replacement alongside the same-version reinstall is a filed cut.
#[cfg(test)]
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_unsatisfied_probe(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    slot_want: &HashMap<(String, String), Vec<String>>,
    pullers: &SlotPullers,
    masked: &HashMap<(String, String), Vec<MaskEntry>>,
    with_bdeps: bool,
    excluded: &[String],
    config: &portage_profile::Config,
) -> BTreeSet<(String, String)> {
    slot_operator_unsatisfied_probe_full(
        root, repos, entries, slot_want, pullers, masked, with_bdeps, excluded, config,
    )
    .0
}

/// [`slot_operator_unsatisfied_probe_full`]'s result: the installed
/// parents to reinstall and the `(category, package, version)` binary
/// parents to mask.
pub(crate) type SlotOpUnsatProbe = (
    BTreeSet<(String, String)>,
    BTreeSet<(String, String, String)>,
);

/// [`slot_operator_unsatisfied_probe`] plus #257's binary arm: real
/// `_slot_operator_unsatisfied_backtrack` (`_emerge/depgraph.py:2881-2915`)
/// masks a *non-installed* built parent (`slot_operator_mask_built`,
/// `:2896-2903`) instead of seeding a reinstall, and restarts, so the
/// next pass selects the replacement ebuild. Here that parent is a
/// merge-bound binary graph entry; the second result carries its
/// `(category, package, version)`, filed by the caller as the same
/// `[binary]`-marked `SlotOperatorBuilt` negative #212 uses. The firing
/// gate is real's `dep.parent not in _runtime_pkg_mask` (`:3447-3448`)
/// for that binary instance.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn slot_operator_unsatisfied_probe_full(
    root: &Path,
    repos: &[RepoConfig],
    entries: &[GraphEntry],
    slot_want: &HashMap<(String, String), Vec<String>>,
    pullers: &SlotPullers,
    masked: &HashMap<(String, String), Vec<MaskEntry>>,
    with_bdeps: bool,
    excluded: &[String],
    config: &portage_profile::Config,
) -> SlotOpUnsatProbe {
    const RUNTIME_KEYS: [&str; 3] = ["RDEPEND", "IDEPEND", "PDEPEND"];
    const BUILD_KEYS: [&str; 2] = ["DEPEND", "BDEPEND"];
    // Real `_iter_similar_available` (`:3036-3040`) and the firing gate
    // (`:3447-3448`) both skip `pkg in _runtime_pkg_mask` per package
    // instance (real keys by package object): a `!=cp-ver` negative
    // screens only that version, and a `[binary]`-marked negative only
    // ever screens the binary instance, never the same-version ebuild
    // (real still yields the ebuild there; see `is_binary_only_mask`).
    let masked_version = |cp: &(String, String), version: &str| -> bool {
        masked.get(cp).is_some_and(|bucket| {
            bucket.iter().any(|entry| {
                if is_binary_only_mask(&entry.neg) {
                    return false;
                }
                // #276: an `[installed]`-marked negative screens the
                // installed instance itself (real `:3447-3448` skips a
                // masked parent, or the restart would loop).
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
        // Fast pre-filter: real only ever probes a built-atom edge, so
        // with no built-form want on this cp there is no edge to probe.
        if !wants.iter().any(|atom_str| {
            portage_dep::parse_atom(atom_str).is_some_and(|atom| {
                atom.blocker == portage_dep::Blocker::None && is_built_slot_op(&atom)
            })
        }) {
            continue;
        }
        for owner in &e.required_by {
            // Real probes one dep edge (parent -> the built atom,
            // `:3447-3458` into the `:2817` probe): seed only an owner
            // whose own recorded want is the built atom -- this pass's
            // puller triples pair owner and atom text -- never the
            // cp-level `required_by` x `slot_want` cross product. The
            // puller triples are recorded after USE evaluation, so a
            // conditional built want that evaluates false is absent here,
            // as in real's USE-evaluated view. Real restarts on the first
            // hitting edge (`:3453-3455`); this seeds every hitting owner
            // of the cp in one pass (residue #256).
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
            // Real `dep.parent.installed`; #257: otherwise a merge-bound
            // binary parent (real's `not parent.installed` mask arm).
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
            // Real `:2818-2824`: an installed parent matching
            // `--exclude` refuses the probe.
            let excluded_parent = installed.iter().any(|(v, _, _)| {
                let q_str = format!("{}/{}-{v}", owner.0, owner.1);
                excluded
                    .iter()
                    .any(|ex| matches_config_entry(ex, &q_str, &owner.0, &owner.1))
            });
            if excluded_parent {
                continue;
            }
            for (installed_version, parent_slot, is_binary) in &installed {
                // Real `:3447-3448`: an installed instance already in
                // `runtime_pkg_mask` would restart-loop; for a binary
                // parent, its own `[binary]` mask is that instance.
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
                        .any(|ex| matches_config_entry(ex, &cand_str, &owner.0, &owner.1))
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
                        // Real `replacement_parent.validated_atoms`
                        // (`_emerge/Package.py:329-359`): `use_reduce(...,
                        // matchall=True)`, every conditional branch kept
                        // regardless of the replacement's USE (#254's
                        // conditional shape heals only through this).
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
                            // Real `:2835-2839`: a `=`-operator,
                            // non-blocker atom on the dep's own cp.
                            if live.slot_operator != Some(portage_dep::SlotOperator::Equals)
                                || live.blocker != portage_dep::Blocker::None
                                || live.category != dep_cp.0
                                || live.package != dep_cp.1
                            {
                                continue;
                            }
                            // Real `:2843-2845`: USE discarded,
                            // then `_select_package` -- visible
                            // tree or installed.
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
