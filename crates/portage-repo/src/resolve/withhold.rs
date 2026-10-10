//! Updates withheld by constraints, and missed updates to backtrack on.

use crate::*;

/// Backlog #90 (S2) + #92: an enforced holdable pin that settles the
/// installed version while a higher visible candidate exists is a
/// withhold real reports through the same skip-conflict block (host
/// qemu/edk2, hermetic whpin cell) -- no slot conflict ever forms on
/// this path (the re-resolve picks the installed version up front),
/// so the direct solve above never sees it. For every enforced pin
/// whose cp settles without an upgrade, the highest same-slot visible
/// candidate the pin excludes becomes a `SkippedUpdate` (same shape as
/// the removal rows; the renderer cannot tell the producers apart and
/// real does not distinguish them either).
pub(crate) fn constraint_withheld_updates(
    repos: &[RepoConfig],
    root: &Path,
    config: &portage_profile::Config,
    entries: &[GraphEntry],
    enforced: &[RevDepPin],
) -> Vec<SkippedUpdate> {
    let mut out: Vec<SkippedUpdate> = Vec::new();
    let mut seen: HashSet<((String, String), String)> = HashSet::new();
    for pin in enforced {
        if !seen.insert((pin.cp.clone(), pin.atom.clone())) {
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
        if let Ok(cands) = list_candidates(repos, &pin.cp.0, &pin.cp.1) {
            for cand in cands.iter() {
                if cand.slot != settled_slot {
                    continue;
                }
                match vercmp_ordering(&cand.version, &settled_ver) {
                    std::cmp::Ordering::Greater => {}
                    // Backlog #210: a same-version slot/sub-slot change
                    // without a revbump is real's missed update for the
                    // slot too -- the reinstall candidate sits at a new
                    // sub-slot with nothing selected above it
                    // (`_get_missed_updates`, `depgraph.py:1529-1562`).
                    std::cmp::Ordering::Equal if cand.sub_slot != settled_sub => {}
                    _ => continue,
                }
                let probe = format!(
                    "{}/{}-{}:{}/{}::{}",
                    pin.cp.0, pin.cp.1, cand.version, cand.slot, cand.sub_slot, cand.repo_name
                );
                let structurally_excluded =
                    portage_dep::match_from_list(&pin.atom, &[probe.as_str()])
                        .is_none_or(|m| m.is_empty());
                // A structural match is not enough when the recorded pin
                // carries `[use]` deps: re-check them against the
                // candidate's own effective flags (the same probe the
                // autounmask path runs). Parsed from the verbatim
                // recorded text -- `pin.atom` is normalised for
                // matching and never carries them. A structural miss
                // withholds regardless of USE (the branch below only
                // ever excuses a structural hit).
                let mut use_ok = true;
                if !structurally_excluded {
                    if let Some(atom) = portage_dep::parse_atom(&pin.raw_atom) {
                        let use_deps = atom.use_deps.unwrap_or_default();
                        if !use_deps.is_empty() {
                            let cpv = format!(
                                "{}/{}-{}:{}/{}::{}",
                                pin.cp.0,
                                pin.cp.1,
                                cand.version,
                                cand.slot,
                                cand.sub_slot,
                                cand.repo_name
                            );
                            let enabled = effective_use_flags(
                                config,
                                &cand.iuse,
                                &cand.keywords,
                                &cpv,
                                &pin.cp.0,
                                &pin.cp.1,
                            );
                            let declared: HashSet<String> = cand
                                .iuse
                                .split_whitespace()
                                .map(|t| t.trim_start_matches(['+', '-']).to_string())
                                .collect();
                            use_ok = portage_dep::use_deps_satisfied(
                                &use_deps,
                                &valid_iuse(&declared, config),
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
                skipped_use: skipped_update_use_display_for(
                    repos, config, &pin.cp.0, &pin.cp.1, &wver,
                ),
                atom: pin.raw_atom.clone(),
                consumer_cpv: format!("{cc}/{cp2}-{cv}:{cslot}/{csub}::{crepo}"),
                consumer_installed: true,
                consumer_use: skipped_update_installed_use_display_for(root, config, cc, cp2, cv),
                // Backlog #242 Slice D: the withhold path is target-walk
                // only (installed consumers from the walk root's own vdb,
                // withheld versions from that same walk).
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

/// Backlog #129 (S1): highest missed version per `(cat, pkg, slot)`
/// while deriving [`backtrack_missed_updates`] -- the version, its
/// first-seen mask order, the mask reason, and the tree sub-slot/repo.
pub(crate) type KeptMissedValue = (String, usize, MaskReason, String, String);

pub(crate) type KeptMissed = HashMap<(String, String, String), KeptMissedValue>;

/// Backlog #129 (review): row-identity key for
/// [`collapse_skipped_updates`]'s exact-duplicate pass -- every
/// [`SkippedUpdate`] field (backlog #242 Slice D: the two root flags
/// included, so a target-rooted and a running-rooted row for the same
/// missed pkg never collapse into one).
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
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) params: &'a BacktrackParams,
    /// Backlog #286: the settled pass's dropped set-member atoms. A
    /// dropped member has no entry, so the chosen-instance scan below
    /// would find nothing selected; real's tracker still holds its
    /// installed instance (complete-walk nomerge node), hence the vdb
    /// fallback in [`missing_dep_full_row`] consults these. Empty at the
    /// prune-rebuilds probe site (that check runs mid-search on the live
    /// graph, where the row must not materialize -- real prunes from the
    /// live graph too, but portuale's blunt clear would drop the healing
    /// sibling's forced reinstall for good).
    pub(crate) missing_args: &'a [String],
}

/// Backlog #129 (S1): real `_get_missed_updates`
/// (`lib/_emerge/depgraph.py:1529-1565`) over the settled backtrack
/// trial state -- `self._dynamic_config._runtime_pkg_mask` (masks
/// accumulated across backtracking runs, `:706`) chained with
/// `_conflict_missed_update` (the slot-conflict handler's removals,
/// `:2090-2106`). The second source already rides
/// `PassResult::skipped_updates` (the direct solve's removal rows, #90
/// S2); this reads the first: for every masked version that is higher
/// than the chosen one in its `(root, slot_atom)` -- keeping the
/// highest per slot -- record `(pkg, mask_type, parent_atoms)`.
///
/// Mask-type mapping: real's `"slot conflict"` becomes `SkippedUpdate`
/// rows (same shape as the direct-solve rows -- one row per rejecting
/// parent, like the established provisional shape, where real renders
/// one block with every parent inside); real's `"missing dependency"`
/// becomes [`SkippedMissingDep`] slots when the missing atom still
/// matches a backtrack-masked version at settle time (real's
/// `check_backtrack` probe (`:6471-6484`): raise `_backtrack_mask`,
/// abbreviated tail), and [`SkippedMissingDepFull`] rows otherwise
/// (backlog #198: the full per-update form, `:1592-1636`).
///
/// Deliberate narrowings (all documented at the call site too):
/// - one root only (portuale resolves a single root; real keys by
///   `(root, slot_atom)` and renders `for <root>` -- omitted like every
///   other portuale notice row).
/// - an installed masked version is never reported (real excludes
///   `pkg.installed`; portuale's rows are merge-shaped).
/// - a masked version with no tree metadata (no slot to key on) is
///   skipped: there is no honest slot_atom for it.
/// - a `"missing dependency"` record whose atom matches nothing
///   backtrack-masked at settle time becomes a
///   [`SkippedMissingDepFull`] row (backlog #198) -- except when no
///   settled entry holds the slot (defensive; real's `any_selected`
///   gate already filtered that) or the masked version's tree metadata
///   cannot name the skipped node (same slot-narrowing as above).
/// - cross-source collapse against the direct-solve rows happens at
///   the call site (`collapse_skipped_updates`, real `:1553-1562`):
///   per slot only the highest-version rows survive, whichever source
///   holds them; equal versions all stay (the established
///   per-(version, parent) shape).
/// - parents render merge-scheduled (`consumer_installed: false`), the
///   `build_slot_conflict` convention -- mask parents are graph pullers.
pub(crate) fn backtrack_missed_updates(
    args: BacktrackMissedUpdatesArgs<'_>,
) -> (
    Vec<SkippedUpdate>,
    Vec<SkippedMissingDep>,
    Vec<SkippedMissingDepFull>,
) {
    let BacktrackMissedUpdatesArgs {
        repos,
        root,
        config,
        entries,
        params,
        missing_args,
    } = args;
    // First-seen position per negative (real's dict insertion order;
    // defensive `usize::MAX` for entries the order vector never saw).
    let order_of = |cp: &(String, String), neg: &str| -> usize {
        params
            .mask_order
            .iter()
            .position(|(k, n)| k == cp && n == neg)
            .unwrap_or(usize::MAX)
    };
    // Highest missed version per `(cat, pkg, slot)`, with its order,
    // reason, sub-slot and repo: ties go to the first-seen mask (real
    // keeps the first record per key on ties across its chained
    // sources; within one source the key is written once).
    let mut kept: KeptMissed = HashMap::new();
    for ((cat, pkg), bucket) in &params.runtime_pkg_mask {
        let cp = (cat.clone(), pkg.clone());
        for entry in bucket {
            // #212: built-binary steers render no missed-update rows.
            // Real `_get_missed_updates` reads the same mask dict, but a
            // `slot_operator_mask_built` mask steers *within* a version
            // (binary -> same-version ebuild), so the masked version is
            // never higher than the chosen one -- the row real would
            // print never materialises. (A lower-version choice against
            // a masked-higher binary is unreached: the re-pass picks the
            // same-version ebuild, never a lower version.)
            if matches!(entry.reason, MaskReason::SlotOperatorBuilt) {
                continue;
            }
            // `!=cat/pkg-ver` back to the version (defensive: skip what
            // does not parse against its own bucket).
            let (ver, order) = match entry.neg.strip_prefix("!=").and_then(split_cpv) {
                Some((c, p, v)) if c == *cat && p == *pkg => (v, order_of(&cp, &entry.neg)),
                _ => continue,
            };
            // Real's installed exclusion (`:1539-1542`): a mask whose
            // package object is installed reports nothing. Portuale's
            // string key conflates the installed and merge-bound
            // instances, so this stays for `SlotConflict` (the masked
            // node may genuinely be the installed one there, and the
            // mg2top pins rely on the silence). It does NOT apply to a
            // `MissingDependency` mask: real keys that mask by package
            // object, so a mask on the merge-bound instance reports even
            // when the same version is installed -- the #286
            // selected/skipped pair (the true installed key, the
            // `[installed]`-marked neg, already skips at `split_cpv`
            // above since brackets never parse as a version).
            if matches!(entry.reason, MaskReason::SlotConflict { .. })
                && installed_candidates(root, cat, pkg)
                    .iter()
                    .any(|(v, _, _)| v == &ver)
            {
                continue;
            }
            let (sub, repo, slot) = slot_conflict_meta(repos, cat, pkg, &ver);
            if slot.is_empty() {
                continue;
            }
            // Real's chosen comparison (`:1543-1554`): the tracker's
            // `(root, slot_atom)` matches here -- merge-bound entries
            // plus installed ones still listed.
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
                        slot_conflict_meta(repos, cat, pkg, chosen_ver).2
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
                    portage_dep::parse_atom(a).is_some_and(|p| {
                        p.category == *cat
                            && p.package == *pkg
                            && p.blocker == portage_dep::Blocker::None
                    })
                })
                && installed_candidates(root, cat, pkg)
                    .iter()
                    .any(|(iv, _, _)| iv == &ver && read_vdb_slot(root, cat, pkg, iv).0 == slot)
            {
                // Backlog #286: the dropped set member's installed
                // instance is real's `selected_pkg` (its complete-walk
                // nomerge node survives the argument drop). The vdb
                // version must equal the masked one -- any other
                // installed version has no proven graph presence
                // (real's `any_selected` gate), so it stays silent.
                // `missed` stays true: like real's equal-version
                // installed choice, it never clears the miss.
                any_selected = true;
            }
            if !any_selected || !missed {
                continue;
            }
            // Real skips mask types with no parents (`:1559-1562`).
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
    // Emit in first-seen order (real's dict order).
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
                    // Backlog #205: an installed enforcing consumer
                    // (the world-reachable bound behind an enforced
                    // pin) renders vdb-based -- `(<cpv>, installed
                    // ...)` with the vdb repo (`__unknown__`
                    // fallback), like real `str(Package)` for an
                    // installed node and like the withhold rows'
                    // established shape -- instead of the
                    // merge-scheduled tree form. A graph puller keeps
                    // the tree form.
                    let installed_ref = installed_refs(root, &pc, &pp)
                        .into_iter()
                        .find(|r| r.version == pv);
                    let (consumer_cpv, consumer_installed, consumer_use) = match &installed_ref {
                        Some(r) => (
                            format!("{pc}/{pp}-{pv}:{}/{}::{}", r.slot, r.sub_slot, r.repo),
                            true,
                            skipped_update_installed_use_display_for(root, config, &pc, &pp, &pv),
                        ),
                        None => (
                            slot_conflict_puller_cpv(repos, &pc, &pp, &pv),
                            false,
                            skipped_update_use_display_for(repos, config, &pc, &pp, &pv),
                        ),
                    };
                    // Backlog #242 Slice D: per-node roots — the missed
                    // instance rides the rejecting parent's edge, a merge
                    // parent rides its entry; an installed parent rides
                    // the walk root's own vdb (always the target walk
                    // here, so target).
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
                        skipped_use: skipped_update_use_display_for(
                            repos, config, &cat, &pkg, &ver,
                        ),
                        atom,
                        consumer_cpv,
                        consumer_installed,
                        consumer_use,
                        missed_targets_running_root: missed_running,
                        consumer_targets_running_root: consumer_running,
                    });
                }
            }
            MaskReason::MissingDependency { atom, .. } => {
                if backtrack_masked_match(repos, params, &atom) {
                    missing.push(SkippedMissingDep {
                        category: cat.clone(),
                        package: pkg.clone(),
                        slot: slot.clone(),
                    });
                } else if let Some(row) = missing_dep_full_row(MissingDepFullRowArgs {
                    repos,
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
                }) {
                    full.push(row);
                }
            }
            // Unreachable: the collection loop above skips
            // `SlotOperatorBuilt` entries, so none ever reach `kept`.
            MaskReason::SlotOperatorBuilt => {}
        }
    }
    (skipped, missing, full)
}
