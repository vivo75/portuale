//! Updates withheld by constraints, and missed updates to backtrack on.

use crate::*;

/// An installed binary's version as Portage prints it: `<ver>-<BUILD_ID>`
/// when the vdb records a positive build id, else the bare version.
// Portage: Package.py, __str__ (`cpv + build_id_str`)
pub(crate) fn installed_version_with_build_id(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> String {
    match vdb_aux_get(root, category, package, version, "BUILD_ID")
        .trim()
        .parse::<u64>()
    {
        Ok(id) if id > 0 => format!("{version}-{id}"),
        _ => version.to_string(),
    }
}

/// An enforced holdable pin settling the installed version while a higher
/// visible candidate exists is a withhold, reported with the removal rows
/// (no slot conflict forms: the re-resolve picks installed up front).
// Portage: depgraph.py, _conflict_missed_update
pub(crate) fn constraint_withheld_updates(
    gate: CategoryGate<'_>,
    root: &Path,
    config: &portage_profile::Config,
    entries: &[GraphEntry],
    enforced: &[RevDepPin],
) -> Vec<SkippedUpdate> {
    let mut out: Vec<SkippedUpdate> = Vec::new();
    // Two consumers recording the same atom are two parents (Portage's
    // `_conflict_missed_update` keeps one `(parent, atom)` pair per parent
    // atom), so the consumer is part of the key.
    let mut seen: HashSet<MaskParentKey> = HashSet::new();
    for pin in enforced {
        if !seen.insert((pin.cp.clone(), pin.atom.clone(), pin.consumer.clone())) {
            continue;
        }
        // An upgrade went through: nothing was withheld.
        if entries.iter().any(|e| {
            (e.category.clone(), e.package.clone()) == pin.cp
                && matches!(
                    e.outcome,
                    PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. }
                )
        }) {
            continue;
        }
        let settled = entries.iter().find_map(|e| {
            if (e.category.clone(), e.package.clone()) == pin.cp {
                match &e.outcome {
                    PretendOutcome::AlreadyInstalled { version } => Some(version.clone()),
                    _ => None,
                }
            } else {
                None
            }
        });
        let (settled_ver, settled_slot, settled_sub) = match settled {
            Some(v) => {
                let (s, sub) = read_vdb_slot(root, &pin.cp.0, &pin.cp.1, &v);
                (v, s, sub)
            }
            None => continue,
        };
        let mut withheld: Vec<(String, String, String)> = Vec::new();
        if let Ok(cands) = gate.repo_candidates(&pin.cp.0, &pin.cp.1) {
            for cand in cands.iter() {
                if cand.slot != settled_slot {
                    continue;
                }
                match vercmp_ordering(&cand.version, &settled_ver) {
                    std::cmp::Ordering::Greater => {}
                    // A same-version slot/sub-slot change without a revbump is
                    // a missed update for the slot too.
                    std::cmp::Ordering::Equal if cand.sub_slot != settled_sub => {}
                    _ => continue,
                }
                let probe = cand.match_str(&pin.cp.0, &pin.cp.1);
                let structurally_excluded =
                    portage_dep::match_from_list(&pin.atom, &[probe.as_str()])
                        .is_none_or(|m| m.is_empty());
                // A structural match is not enough when the recorded pin carries
                // `[use]` deps: re-check them against the candidate's own
                // effective flags (parsed from the verbatim recorded text --
                // `pin.atom` never carries them). A structural miss withholds
                // regardless of USE.
                let mut use_ok = true;
                if !structurally_excluded {
                    if let Some(atom) = parse_dep_atom(&pin.raw_atom, "withhold") {
                        let use_deps = atom.use_deps.unwrap_or_default();
                        if !use_deps.is_empty() {
                            let cpv = cand.match_str(&pin.cp.0, &pin.cp.1);
                            let enabled = effective_use_flags(EffectiveUseFlagsArgs {
                                config,
                                iuse: &cand.iuse,
                                keywords: &cand.keywords,
                                candidate_str: &cpv,
                                build_id: cand.build_id.as_deref(),
                                category: &pin.cp.0,
                                package: &pin.cp.1,
                            });
                            let declared: HashSet<String> = cand
                                .iuse
                                .split_whitespace()
                                .map(|t| t.trim_start_matches(['+', '-']).to_string())
                                .collect();
                            use_ok = portage_dep::use_deps_satisfied_union(
                                &use_deps,
                                &declared,
                                &[&config.iuse_effective],
                                &enabled,
                            );
                        }
                    }
                    if use_ok {
                        continue;
                    }
                }
                withheld.push((
                    cand.version.clone(),
                    cand.sub_slot.clone(),
                    cand.repo_name.clone(),
                ));
            }
        }
        if let Some((wver, wsub, wrepo)) = withheld
            .into_iter()
            .max_by(|x, y| vercmp_ordering(&x.0, &y.0))
        {
            let (cc, cp2, cv) = &pin.consumer;
            let (cslot, csub, crepo) = match installed_refs(root, cc, cp2)
                .into_iter()
                .find(|r| &r.version == cv)
            {
                Some(r) => (r.slot, r.sub_slot, r.repo),
                None => ("0".to_string(), "0".to_string(), "__unknown__".to_string()),
            };
            out.push(SkippedUpdate {
                category: pin.cp.0.clone(),
                package: pin.cp.1.clone(),
                slot: settled_slot,
                skipped_version: wver.clone(),
                skipped_sub_slot: wsub,
                skipped_repo: wrepo.clone(),
                skipped_build_id: None,
                skipped_use: skipped_update_use_display_for(
                    gate, config, &pin.cp.0, &pin.cp.1, &wver,
                ),
                atom: pin.raw_atom.clone(),
                consumer_cpv: format!(
                    // match-str-ok: slot_conflict_meta tuple, not a Candidate
                    "{cc}/{cp2}-{}:{cslot}/{csub}::{crepo}",
                    installed_version_with_build_id(root, cc, cp2, cv)
                ),
                consumer_build_id: None,
                consumer_installed: true,
                consumer_use: skipped_update_installed_use_display_for(root, config, cc, cp2, cv),
                // Target-walk only (same walk's vdb and versions).
                missed_targets_running_root: false,
                consumer_targets_running_root: false,
            });
        }
    }
    out.sort_by(|a, b| {
        (
            &a.category,
            &a.package,
            &a.skipped_version,
            &a.atom,
            &a.consumer_cpv,
        )
            .cmp(&(
                &b.category,
                &b.package,
                &b.skipped_version,
                &b.atom,
                &b.consumer_cpv,
            ))
    });
    out.dedup();
    out
}

/// Highest missed version per `(cat, pkg, slot)` while deriving
/// [`backtrack_missed_updates`]: version, first-seen mask order, mask reason,
/// tree sub-slot and repo.
pub(crate) type KeptMissedValue = (String, usize, MaskReason, String, String);

pub(crate) type KeptMissed = HashMap<(String, String, String), KeptMissedValue>;

/// Row-identity key for [`collapse_skipped_updates`]' exact-duplicate pass:
/// every [`SkippedUpdate`] field, root flags included.
pub(crate) type SkippedUpdateKey = (
    (String, String, String, String, String, String),
    Vec<(String, String)>,
    String,
    String,
    bool,
    Vec<(String, String)>,
    bool,
    bool,
);

/// Arguments of [`backtrack_missed_updates`].
pub(crate) struct BacktrackMissedUpdatesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) params: &'a BacktrackParams,
    /// The settled pass's dropped set-member atoms (consulted by the vdb
    /// fallback in [`missing_dep_full_row`]). Empty at the prune-rebuilds
    /// probe site, where the row must not materialize.
    pub(crate) missing_args: &'a [String],
    pub(crate) gate: CategoryGate<'a>,
}

/// Missed updates over the settled backtrack trial state: the accumulated
/// `runtime_pkg_mask` (the slot-conflict removals already ride
/// `PassResult::skipped_updates`). The highest masked version per slot above
/// the chosen one becomes a row. Narrowings: one root only; installed or
/// metadata-less versions skipped; cross-source collapse at the call site.
// Portage: depgraph.py, _get_missed_updates
pub(crate) fn backtrack_missed_updates(
    args: BacktrackMissedUpdatesArgs<'_>,
) -> (
    Vec<SkippedUpdate>,
    Vec<SkippedMissingDep>,
    Vec<SkippedMissingDepFull>,
) {
    let BacktrackMissedUpdatesArgs {
        root,
        config,
        entries,
        params,
        missing_args,
        gate,
    } = args;
    // First-seen position per negative (dict insertion order).
    let order_of = |cp: &(String, String), neg: &str| -> usize {
        params
            .mask_order
            .iter()
            .position(|(k, n)| k == cp && n == neg)
            .unwrap_or(usize::MAX)
    };
    // Ties go to the first-seen mask.
    let mut kept: KeptMissed = HashMap::new();
    for ((cat, pkg), bucket) in &params.runtime_pkg_mask {
        let cp = (cat.clone(), pkg.clone());
        for entry in bucket {
            // Built-binary steers render no missed-update rows: the mask steers
            // *within* a version, so the masked version is never higher.
            if matches!(entry.reason, MaskReason::SlotOperatorBuilt) {
                continue;
            }
            // `!=cat/pkg-ver` back to the version.
            let (ver, order) = match entry.neg.strip_prefix("!=").and_then(split_cpv) {
                Some((c, p, v)) if c == *cat && p == *pkg => (v, order_of(&cp, &entry.neg)),
                _ => continue,
            };
            // Installed exclusion for `SlotConflict` masks only: a mask whose
            // version is installed reports nothing. `MissingDependency` masks
            // are keyed by package object, so they still report (the
            // `[installed]`-marked neg already skips at `split_cpv` above).
            if matches!(entry.reason, MaskReason::SlotConflict { .. })
                && installed_candidates(root, cat, pkg)
                    .iter()
                    .any(|(v, _, _)| v == &ver)
            {
                continue;
            }
            let (sub, repo, slot) = slot_conflict_meta(gate, cat, pkg, &ver);
            if slot.is_empty() {
                continue;
            }
            // The tracker's `(root, slot_atom)` match: merge-bound entries plus
            // installed ones still listed.
            let mut any_selected = false;
            let mut missed = true;
            for e in entries {
                if e.category != *cat || e.package != *pkg {
                    continue;
                }
                let (chosen_ver, chosen_installed) =
                    match &e.outcome {
                        PretendOutcome::New { version }
                        | PretendOutcome::Reinstall { version, .. } => (version, false),
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } => (to, false),
                        PretendOutcome::AlreadyInstalled { version } => (version, true),
                        PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => {
                            continue;
                        }
                    };
                let chosen_slot = e.slot.clone().or_else(|| {
                    let s = if chosen_installed {
                        read_vdb_slot(root, cat, pkg, chosen_ver).0
                    } else {
                        slot_conflict_meta(gate, cat, pkg, chosen_ver).2
                    };
                    if s.is_empty() { None } else { Some(s) }
                });
                if chosen_slot.as_deref() != Some(slot.as_str()) {
                    continue;
                }
                any_selected = true;
                match vercmp_ordering(chosen_ver, &ver) {
                    std::cmp::Ordering::Greater => {
                        missed = false;
                        break;
                    }
                    std::cmp::Ordering::Equal if !chosen_installed => {
                        missed = false;
                        break;
                    }
                    _ => {}
                }
            }
            if !any_selected
                && matches!(entry.reason, MaskReason::MissingDependency { .. })
                && missing_args.iter().any(|a| {
                    parse_dep_atom(a, "withhold").is_some_and(|p| {
                        p.category == *cat
                            && p.package == *pkg
                            && p.blocker == portage_dep::Blocker::None
                    })
                })
                && installed_candidates(root, cat, pkg)
                    .iter()
                    .any(|(iv, _, _)| iv == &ver && read_vdb_slot(root, cat, pkg, iv).0 == slot)
            {
                // The dropped set member's installed instance still counts as
                // selected (scoped to version equality); it never clears the
                // miss, like an equal-version installed choice.
                any_selected = true;
            }
            if !any_selected || !missed {
                continue;
            }
            // Mask types with no parents are skipped.
            if let MaskReason::SlotConflict { parents } = &entry.reason
                && parents.is_empty()
            {
                continue;
            }
            let key = (cat.clone(), pkg.clone(), slot.clone());
            let replace = match kept.get(&key) {
                None => true,
                Some((kept_ver, kept_order, _, _, _)) => match vercmp_ordering(&ver, kept_ver) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Equal => order < *kept_order,
                    _ => false,
                },
            };
            if replace {
                kept.insert(key, (ver, order, entry.reason.clone(), sub, repo));
            }
        }
    }
    // Emit in first-seen order.
    let mut kept: Vec<((String, String, String), KeptMissedValue)> = kept.into_iter().collect();
    kept.sort_by(|a, b| {
        a.1.1
            .cmp(&b.1.1)
            .then_with(|| (a.0.clone(), a.1.0.clone()).cmp(&(b.0.clone(), b.1.0.clone())))
    });
    let mut skipped: Vec<SkippedUpdate> = Vec::new();
    let mut missing: Vec<SkippedMissingDep> = Vec::new();
    let mut full: Vec<SkippedMissingDepFull> = Vec::new();
    for ((cat, pkg, slot), (ver, _, reason, sub, repo)) in kept {
        match reason {
            MaskReason::SlotConflict { parents } => {
                for ((pc, pp, pv), atom) in parents {
                    if pc.is_empty() {
                        continue;
                    }
                    // An installed enforcing consumer renders vdb-based
                    // (`(<cpv>, installed ...)`); a graph puller keeps the
                    // tree form.
                    let installed_ref = installed_refs(root, &pc, &pp)
                        .into_iter()
                        .find(|r| r.version == pv);
                    let (consumer_cpv, consumer_installed, consumer_use) = match &installed_ref {
                        Some(r) => (
                            format!(
                                // match-str-ok: installed ref row (r.repo), not a Candidate
                                "{pc}/{pp}-{}:{}/{}::{}",
                                installed_version_with_build_id(root, &pc, &pp, &pv),
                                r.slot,
                                r.sub_slot,
                                r.repo
                            ),
                            true,
                            skipped_update_installed_use_display_for(root, config, &pc, &pp, &pv),
                        ),
                        None => (
                            slot_conflict_puller_cpv(gate, &pc, &pp, &pv),
                            false,
                            skipped_update_use_display_for(gate, config, &pc, &pp, &pv),
                        ),
                    };
                    // Per-node roots: the missed instance rides the rejecting
                    // parent's edge, a merge parent its entry, an installed
                    // parent the walk root's vdb.
                    let (missed_running, consumer_running) = if consumer_installed {
                        (false, false)
                    } else {
                        skipped_update_roots(entries, (&cat, &pkg), (&pc, &pp, &pv), &atom)
                    };
                    skipped.push(SkippedUpdate {
                        category: cat.clone(),
                        package: pkg.clone(),
                        slot: slot.clone(),
                        skipped_version: ver.clone(),
                        skipped_sub_slot: sub.clone(),
                        skipped_repo: repo.clone(),
                        skipped_build_id: None,
                        skipped_use: skipped_update_use_display_for(gate, config, &cat, &pkg, &ver),
                        atom,
                        consumer_cpv,
                        consumer_build_id: None,
                        consumer_installed,
                        consumer_use,
                        missed_targets_running_root: missed_running,
                        consumer_targets_running_root: consumer_running,
                    });
                }
            }
            MaskReason::MissingDependency { atom, .. } => {
                if backtrack_masked_match(gate, params, &atom) {
                    missing.push(SkippedMissingDep {
                        category: cat.clone(),
                        package: pkg.clone(),
                        slot: slot.clone(),
                    });
                } else if let Some(row) = missing_dep_full_row(MissingDepFullRowArgs {
                    root,
                    config,
                    entries,
                    cat: &cat,
                    pkg: &pkg,
                    slot: &slot,
                    ver: &ver,
                    sub: &sub,
                    repo: &repo,
                    atom: &atom,
                    gate,
                }) {
                    full.push(row);
                }
            }
            // The collection loop above skips these, so none ever reach `kept`.
            MaskReason::SlotOperatorBuilt => {}
        }
    }
    (skipped, missing, full)
}
