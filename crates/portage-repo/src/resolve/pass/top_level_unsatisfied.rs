//! A top-level atom with no visible candidate: Portage's drop-or-abort for arguments.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`step_top_level_unsatisfied`] reads from its caller.
pub(crate) struct StepTopLevelUnsatisfiedArgs<'a> {
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) current_atom: &'a str,
    pub(crate) outcome: &'a mut PretendOutcome,
    pub(crate) state: &'a mut PassState,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
    pub(crate) key: &'a (String, String),
    pub(crate) config: &'a Config,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) atom: &'a Atom,
}

/// A top-level atom with no visible candidate: Portage's drop-or-abort for arguments.
pub(crate) fn step_top_level_unsatisfied(
    args: StepTopLevelUnsatisfiedArgs<'_>,
) -> Result<ControlFlow<()>, Error> {
    let StepTopLevelUnsatisfiedArgs {
        ctx,
        current_atom,
        outcome,
        state,
        union_constraints,
        key,
        config,
        unevaluated_atom,
        bp,
        atom,
    } = args;
    // A top-level atom with no visible candidate aborts the whole call (not
    // the "report and keep going" dependency treatment). Top-level atoms
    // dequeue in argv order before any dependency, so the first bad one wins.
    // Portage: depgraph.py
    if ctx.top_level.contains(current_atom) && matches!(outcome, PretendOutcome::NoVisibleCandidate)
    {
        // A `@world`/`@selected` set member that selects nothing is dropped into
        // `missing_args` and the walk continues; only explicit args abort.
        // Portage: _resolve. Deduped across re-passes.
        if ctx.set_args.contains(current_atom) {
            if !state.missing_args.iter().any(|s| s == current_atom) {
                state.missing_args.push(current_atom.to_string());
            }
            return Ok(ControlFlow::Break(()));
        }
        // A top-level argument left with no mergeable candidate by an enforced
        // parent-atom pin stays installed instead of aborting (complete-mode
        // withhold). Gated on required-set semantics; otherwise the abort stands.
        let withheld_version = (!ctx.slot_op_reachable.is_empty()
            && union_constraints
                .get(key)
                .is_some_and(|v| v.iter().any(|c| !c.starts_with('!'))))
        .then(|| {
            let positives: Vec<String> = union_constraints.get(key).map_or(Vec::new(), |v| {
                v.iter().filter(|c| !c.starts_with('!')).cloned().collect()
            });
            complete_mode_withheld_version(ctx.root, &key.0, &key.1, current_atom, &positives)
        })
        .flatten();
        if let Some(version) = withheld_version {
            *outcome = PretendOutcome::AlreadyInstalled { version };
        } else {
            // An atom matching only masked ebuilds reports the "all masked"
            // block, not the "no ebuilds" one. Portage: _show_unsatisfied_dep
            if let Some(report) = all_masked_report(
                &ctx.repos,
                current_atom,
                config,
                &format!("{current_atom:?}"),
            ) {
                return Err(Error::Detail(report));
            }
            // A USE miss aborts with the "no ebuilds built with USE flags" block
            // (with `(Change USE:)` / `(Missing IUSE:)` rows, no dep lines for
            // an argument parent), ordered after the masked block like Portage.
            // Portage: depgraph.py (_show_unsatisfied_dep, show_missing_use first)
            if let Some(rows) = use_unsat_candidates_for_atom(UseUnsatCandidatesForAtomArgs {
                repos: &ctx.repos,
                root: ctx.root,
                entries: &state.entries,
                display_atom: unevaluated_atom.as_deref().unwrap_or(current_atom),
                evaluated_atom: current_atom,
                owner: None,
                config,
            }) {
                let mut block = format!(
                    "there are no ebuilds built with USE flags to satisfy {current_atom:?}.\n\
                         !!! One of the following packages is required to complete your request:"
                );
                for (cpv, reasons) in rows {
                    block.push_str(&format!("\n- {cpv} ({})", reasons.join(", ")));
                }
                return Err(Error::Detail(block));
            }
            let mut message = format!("there are no ebuilds to satisfy {current_atom:?}.");
            // Only suggest a change that would actually fix it (KEYWORDS alone).
            // Portage's "only suggest a working change" spirit.
            if bp.autounmask_suggest_keywords
                && let Some((version, keyword)) =
                    suggested_keyword_candidate(&ctx.repos, &atom.category, &atom.package, config)
            {
                message.push_str(&format!(
                    "\nnote: {}/{}-{version} exists but is masked by KEYWORDS; \
                     --autounmask-keep-keywords=n suggests adding \"{}/{} {keyword}\" \
                     to package.accept_keywords",
                    atom.category, atom.package, atom.category, atom.package,
                ));
            }
            // Same gating for `package.use` (message mirrors the `=cat/pkg-ver
            // flag -flag` suggestion syntax).
            if bp.autounmask_suggest_use
                && let Some((version, flip)) = suggested_use_candidate(
                    &ctx.repos,
                    &atom.category,
                    &atom.package,
                    atom.use_deps.as_deref(),
                    config,
                )
            {
                let adjustments: Vec<String> = flip
                    .iter()
                    .map(|(flag, enabled)| {
                        if *enabled {
                            flag.clone()
                        } else {
                            format!("-{flag}")
                        }
                    })
                    .collect();
                message.push_str(&format!(
                    "\nnote: {}/{}-{version} exists but its USE flags don't satisfy \
                     this atom; --autounmask-use suggests adding \"={}/{}-{version} {}\" \
                     to package.use",
                    atom.category,
                    atom.package,
                    atom.category,
                    atom.package,
                    adjustments.join(" "),
                ));
            }
            // Same gating for `package.license`.
            if bp.autounmask_suggest_license
                && let Some((version, licenses)) =
                    suggested_license_candidate(&ctx.repos, &atom.category, &atom.package, config)
            {
                message.push_str(&format!(
                    "\nnote: {}/{}-{version} exists but its LICENSE is not accepted; \
                     --autounmask-license suggests adding \"={}/{}-{version} {licenses}\" \
                     to package.license",
                    atom.category, atom.package, atom.category, atom.package,
                ));
            }
            // Same gating for `package.unmask`.
            if bp.autounmask_suggest_masks
                && let Some(version) =
                    suggested_mask_candidate(&ctx.repos, &atom.category, &atom.package, config)
            {
                message.push_str(&format!(
                    "\nnote: {}/{}-{version} exists but is package.mask'd; \
                     --autounmask-keep-masks=n suggests adding \"={}/{}-{version}\" \
                     to package.unmask",
                    atom.category, atom.package, atom.category, atom.package,
                ));
            }
            return Err(Error::Detail(message));
        }
    }
    Ok(ControlFlow::Continue(()))
}
