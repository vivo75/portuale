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
    // Backtracking (real `backtracking.py` retry loop driven by
    // `_process_slot_conflicts`): if this attempt left any slot conflicts,
    // check each one for solvability -- is there a single version of the
    // conflicted `cat/pkg` that satisfies *every* atom text that targeted
    // it this pass? If so, the whole atom set becomes a config-feedback
    // node for the conflicted package. Unsolvable conflicts fall through
    // to the mask node below; anything still conflicting when the search
    // is exhausted is reported from the best run, exactly as before.
    if !pass.slot_conflicts.is_empty() {
        let mut progressed = false;
        for sc in &pass.slot_conflicts {
            let pkg_key = (sc.category.clone(), sc.package.clone());
            let wants = match pass.slot_want.get(&pkg_key) {
                Some(w) if w.len() >= 2 => w.clone(),
                _ => continue,
            };
            // Solvability pre-check against the raw candidate list (real
            // `_select_pkg_highest_available` over the full atom set). This
            // deliberately ignores keyword/mask visibility -- the rare case
            // where the one satisfying version is itself masked resolves to a
            // plain `NoVisibleCandidate` on the retry, which is
            // acceptable and documented for this slice. It does NOT
            // ignore USE-deps (real enforces them in selection): a
            // version only counts when every want-atom matches it on
            // version, slot, *and* USE, so a USE-mismatching slot reuse
            // falls through to the mask trial and the notice instead of
            // constraining a retry that can only end in swallowed
            // "no visible ebuild" lines.
            let candidates =
                list_candidates(&ctx.repos, &pkg_key.0, &pkg_key.1).unwrap_or_default();
            let solvable = candidates.iter().any(|c| {
                let cs = format!("{}/{}-{}:{}", pkg_key.0, pkg_key.1, c.version, c.slot);
                let (iuse, use_flags) =
                    slot_conflict_flag_sets(&ctx.repos, config, &pkg_key.0, &pkg_key.1, &c.version);
                wants.iter().all(|w| {
                    portage_dep::match_from_list(w, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
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
