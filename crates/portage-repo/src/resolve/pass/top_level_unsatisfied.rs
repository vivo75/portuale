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
    // A top-level atom (as opposed to a dependency reached while
    // recursing) with no visible candidate aborts the whole call --
    // matching real portage's own depgraph.py behavior for an
    // unsatisfiable target, not the "report and keep going" treatment
    // a dependency's own NoVisibleCandidate gets a few lines down.
    // Top-level atoms are always dequeued (and so reach this point)
    // in argv order, before any dependency, so this also guarantees
    // the *first* bad top-level atom is the one that aborts.
    if ctx.top_level.contains(current_atom) && matches!(outcome, PretendOutcome::NoVisibleCandidate)
    {
        // Backlog #286: real `_resolve` (`depgraph.py:5577-5605`) drops
        // a `@world`/`@selected` set member that selects no package
        // into `_missing_args` and continues the walk, where an
        // explicit argument fails the run. The missing parent is an
        // installed one with no merge-bound replacement (the #276
        // shape: both instances masked by now), so the healed rest of
        // the graph settles and the display layer reports the skipped
        // update plus the world-file problem from `missing_args`.
        // Deduped: a repeat visit of the same atom text (e.g. the
        // best-run re-pass) records once.
        if ctx.set_args.contains(current_atom) {
            if !state.missing_args.iter().any(|s| s == current_atom) {
                state.missing_args.push(current_atom.to_string());
            }
            return Ok(ControlFlow::Break(()));
        }
        // Backlog #210: complete-mode withhold of a phase-1 merge
        // (see `complete_mode_withheld_version`): a top-level
        // argument an enforced parent-atom pin leaves with no
        // mergeable candidate stays installed instead of aborting.
        // Without positive pins (a genuine absence) or on a pass
        // without required-set semantics (no seeds fed -- plain
        // phase 1) the abort below stands. `slot_op_reachable` is
        // exactly that gate: it is populated on the CLI's complete
        // pass and on its `--deep` re-run alike, and empty
        // otherwise.
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
            // Real `_show_unsatisfied_dep`: an atom that matches
            // ebuilds which all turned out masked reports the "All
            // ebuilds that could satisfy … have been masked" block
            // (+ each masked candidate and why), not the "there are
            // no ebuilds to satisfy" one.
            if let Some(report) = all_masked_report(
                &ctx.repos,
                current_atom,
                config,
                &format!("{current_atom:?}"),
            ) {
                return Err(Error::Detail(report));
            }
            // Backlog #265(b) (with #273): a top-level atom's own
            // USE miss aborts with real `_show_unsatisfied_dep`'s
            // "no ebuilds built with USE flags to satisfy" block
            // (`depgraph.py:6969-6994`), not the bare miss -- the
            // `(Change USE: …)` / `(Missing IUSE: …)` rows and, like
            // real for an `AtomArg` parent (`:7080-7090`), no
            // `(dependency required by …)` lines. Real's staging
            // `for <root>.` suffix stays absent (the long-standing
            // `fixture-miss-message-unsuffixed` class). Ordered
            // after the masked block like real's own
            // `show_missing_use`-before-`masked_packages` precedence
            // (the dependency arm renders the same way).
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
            // --autounmask's own keyword-suggestion sub-feature (see
            // this function's own doc comment for the full on/off
            // default-resolution logic): only even attempted when
            // enabled, and only ever finds something to suggest when a
            // real candidate exists that's masked by KEYWORDS alone
            // (see `keyword_masked_only`'s own doc comment) -- a
            // candidate masked by package.mask/license/etc. too gets no
            // suggestion here, matching real portage's own "only
            // suggest a change that would actually fix it" spirit,
            // even though portuale doesn't yet combine multiple
            // simultaneous suggestion kinds the way real portage can.
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
            // `--autounmask-use`'s own suggestion sub-feature -- same
            // gating/"only suggest a fix that would actually work"
            // spirit as the keyword one just above. Message format
            // mirrors real `package.use` suggestion syntax
            // (`=category/package-version flag -flag`).
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
            // `--autounmask-license`'s own suggestion sub-feature -- same
            // "only suggest a fix that would actually work" gating.
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
            // `--autounmask-keep-masks=n`'s own suggestion sub-feature.
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
