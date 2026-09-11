//! Updates withheld by constraints, and missed updates to backtrack on.

use crate::*;

/// An enforced holdable pin settling the installed version while a higher
/// visible candidate exists is a withhold, reported with the removal rows
/// (no slot conflict forms: the re-resolve picks installed up front).
// Portage: depgraph.py, _conflict_missed_update
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
                    // A same-version slot/sub-slot change without a revbump is
                    // a missed update for the slot too.
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
                // A structural match is not enough when the recorded pin carries
                // `[use]` deps: re-check them against the candidate's own
                // effective flags (parsed from the verbatim recorded text --
                // `pin.atom` never carries them). A structural miss withholds
                // regardless of USE.
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
                            let enabled = effective_use_flags(EffectiveUseFlagsArgs {
                                config,
                                iuse: &cand.iuse,
                                keywords: &cand.keywords,
                                candidate_str: &cpv,
                                category: &pin.cp.0,
                                package: &pin.cp.1,
                            });
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
