//! Solve the pass's slot conflicts directly where one instance can go, and drop the loser.

use crate::resolve::pass::*;
use portage_profile::Config;

/// Solve the pass's slot conflicts directly where one instance can go, and drop the loser.
pub(crate) fn solve_pass_slot_conflicts(
    state: &mut PassState,
    ctx: &ResolveCtx<'_>,
    bp: &BacktrackParams,
    config: &Config,
) {
    if !state.slot_conflicts.is_empty() {
        let solved = direct_solve_slot_conflicts(DirectSolveInput {
            conflicts: &state.slot_conflicts,
            entries: &state.entries,
            top_level: &ctx.top_level_cps,
            excluded: ctx.excluded,
            selective: ctx.selective,
            replace_cps: &bp.slot_operator_replace_installed,
            root: ctx.root,
            repos: &ctx.repos,
            config,
            gate: ctx.gate(),
        });
        if !solved.removed.is_empty() {
            // The kept instance takes over the removed row (one row per cp);
            // a removal with no keeper deletes it. Repointing keeps
            // `required_by`, merge order and USE wiring stable.
            for (c, p, r, k) in &solved.removed {
                let at = state.entries.iter().position(|e| {
                    e.category == *c
                        && e.package == *p
                        && merge_bound_version(&e.outcome) == Some(r)
                });
                let Some(idx) = at else {
                    continue;
                };
                if k.is_empty() {
                    state.entries.remove(idx);
                    continue;
                }
                // An installed keeper becomes AlreadyInstalled (no row); a merge
                // keeper stays New. Slot data comes from the vdb or the tree.
                let vdb_hit = installed_candidates(ctx.root, c, p)
                    .into_iter()
                    .find(|(v, _, _)| v == k);
                let (slot, sub_slot, repo) = match &vdb_hit {
                    Some((_, s, ss)) => {
                        let repo = installed_pkg_repo(ctx.root, c, p, k);
                        (s.clone(), ss.clone(), repo)
                    }
                    None => {
                        let (sub, repo, slot) = slot_conflict_meta(ctx.gate(), c, p, k);
                        (slot, sub, repo)
                    }
                };
                let entry = &mut state.entries[idx];
                entry.slot = Some(slot);
                entry.sub_slot = Some(sub_slot);
                entry.repo_name = Some(repo);
                // Per-flag `(flag, enabled)` pairs: vdb record for an installed
                // keeper, tree candidate + effective USE for a merge keeper.
                if vdb_hit.is_some() {
                    let (iuse, use_flags) = installed_pkg_iuse_and_use(ctx.root, c, p, k);
                    entry.use_flags_display = iuse
                        .into_iter()
                        .map(|f| {
                            let on = use_flags.contains(&f);
                            (f, on)
                        })
                        .collect();
                    entry.outcome = PretendOutcome::AlreadyInstalled { version: k.clone() };
                } else {
                    let (declared, enabled) = slot_conflict_flag_sets(ctx.gate(), config, c, p, k);
                    entry.use_flags_display = declared
                        .into_iter()
                        .map(|f| {
                            let on = enabled.contains(&f);
                            (f, on)
                        })
                        .collect();
                    entry.outcome = PretendOutcome::New { version: k.clone() };
                }
            }
            state.slot_conflicts = solved.surviving;
            state.skipped_updates.extend(solved.skipped);
        }
        // A repointed keeper was never walked, so its blockers are not pending:
        // flatten the keeper's ebuild dep strings for blocker tokens before
        // `resolve_blockers` runs. Only blockers (a merge dep would need a
        // second walk); installed keepers are exempt (vdb scan covers them).
        // Portage: depgraph.py, _validate_blockers
        for (c, p, _removed, k) in &solved.removed {
            if installed_candidates(ctx.root, c, p)
                .into_iter()
                .any(|(v, _, _)| v == *k)
            {
                continue;
            }
            let Some(keeper_cand) = ctx.repo_candidates(c, p).ok().and_then(|cs| {
                cs.iter()
                    .filter(|cand| cand.version == *k && cand.source == CandidateSource::Ebuild)
                    .max_by_key(|cand| cand.repo_priority)
                    .cloned()
            }) else {
                continue;
            };
            let pf = format!("{p}-{k}");
            let Ok(meta) = repo_aux_metadata(&keeper_cand.repo_location, c, &pf) else {
                continue;
            };
            let (_declared, enabled) = slot_conflict_flag_sets(ctx.gate(), config, c, p, k);
            let owner_key = (c.clone(), p.clone());
            for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
                let Some(dep_str) = meta.get(dep_key) else {
                    continue;
                };
                let toks: Vec<String> = dep_str.split_whitespace().map(String::from).collect();
                let Ok(flat) = portage_use_reduce::use_reduce_flat(
                    &toks,
                    &enabled,
                    portage_use_reduce::MatchMode::Normal,
                ) else {
                    continue;
                };
                for tok in flat {
                    if tok == "||" {
                        continue;
                    }
                    if let Some(dep_atom) = parse_dep_atom(&tok, "pass_slot_conflicts")
                        && dep_atom.blocker != portage_dep::Blocker::None
                    {
                        let duplicate = state.pending_blockers.iter().any(|pb| {
                            pb.owner_key == owner_key
                                && pb.owner_version == *k
                                && pb.atom_str == tok
                        });
                        if !duplicate {
                            state.pending_blockers.push(PendingBlocker {
                                atom_str: tok,
                                strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                                buildtime: dep_key == "DEPEND" || dep_key == "BDEPEND",
                                target_category: dep_atom.category,
                                target_package: dep_atom.package,
                                owner_key: owner_key.clone(),
                                owner_version: k.clone(),
                                owner_merging: true,
                                owner_installed: false,
                            });
                        }
                    }
                }
            }
        }
    }
}
