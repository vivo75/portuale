//! A slot conflict one version can satisfy: retry with that version.

use crate::resolve::feedback::*;
use portage_profile::Config;

/// A slot conflict one version can satisfy: retry with that version.
pub(crate) fn step_solvable_slot_conflicts(
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    config: &Config,
    grown: &mut BacktrackParams,
) -> Option<PassDecision> {
    // Portage: backtracking.py, _process_slot_conflicts. For each conflict, if one
    // version satisfies every atom text that targeted it, the atom set becomes a
    // config-feedback node. Unsolvable conflicts fall through to the mask arm.
    if !pass.slot_conflicts.is_empty() {
        let mut progressed = false;
        for sc in &pass.slot_conflicts {
            let pkg_key = (sc.category.clone(), sc.package.clone());
            let wants = match pass.slot_want.get(&pkg_key) {
                Some(w) if w.len() >= 2 => w.clone(),
                _ => continue,
            };
            // Ignores keyword/mask visibility (a masked sole satisfier resolves to a
            // plain NVC on retry); enforces USE deps so a USE-mismatching reuse falls
            // through to the mask trial instead of constraining a doomed retry.
            let candidates =
                list_candidates(&ctx.repos, &pkg_key.0, &pkg_key.1).unwrap_or_default();
            let solvable = candidates.iter().any(|c| {
                let cs = format!("{}/{}-{}:{}", pkg_key.0, pkg_key.1, c.version, c.slot);
                let (iuse, use_flags) =
                    slot_conflict_flag_sets(&ctx.repos, config, &pkg_key.0, &pkg_key.1, &c.version);
                wants.iter().all(|w| {
                    portage_dep::match_from_list(w, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                        // drop-ok: kept
                        && portage_dep::parse_atom(w).is_none_or(|a| match &a.use_deps {
                            None => true,
                            Some(ud) => portage_dep::use_deps_satisfied(ud, &iuse, &use_flags),
                        })
                })
            });
            if !solvable {
                continue;
            }
            let slot = grown.slot_constraints.entry(pkg_key).or_default();
            for w in wants {
                if !slot.contains(&w) {
                    slot.push(w);
                    progressed = true;
                }
            }
        }
        if progressed {
            return Some(PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(std::mem::take(grown)),
            }));
        }
    }
    None
}
