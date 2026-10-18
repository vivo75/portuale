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

/// Whether an explicitly requested top-level atom is skipped silently
/// because it is `--exclude`d: Portage skips the argument when *any*
/// match is excluded (`if excluded: continue` in the argument loop), but
/// only when selection found nothing (`if not pkg:`) — a surviving
/// selectable match means the walk never reaches this probe. Ebuild,
/// local-binary, installed and (under `--getbinpkg`) remote matches all
/// count, like Portage's `_iter_match_pkgs_any`.
// Portage: lib/_emerge/depgraph.py, the argument loop (`if not pkg:`
// ... `if excluded: continue`); S0 cell `g07-partly-excluded`.
/// Candidate strings mirror the `cat/pkg-ver:slot/sub::repo` shape
/// `resolve_pretend` matches against.
fn top_level_atom_excluded(
    ctx: &ResolveCtx<'_>,
    config: &Config,
    current_atom: &str,
    atom: &Atom,
) -> Result<bool, Error> {
    if ctx.excluded.is_empty() {
        return Ok(false);
    }
    let excluded_match = |candidate_str: &str| {
        portage_dep::match_from_list(current_atom, &[candidate_str])
            .is_some_and(|m| !m.is_empty())
            .then(|| {
                ctx.excluded
                    .iter()
                    .any(|ex| matches_cli_option_atom(ex, candidate_str))
            })
    };
    // Portage skips on any excluded match; without any match at all the
    // atom stays unsatisfiable (the miss path below reports it).
    let mut any_match = false;
    let mut any_excluded = false;
    let mut fold = |candidate_str: String| match excluded_match(&candidate_str) {
        None => {}
        Some(excluded) => {
            any_match = true;
            any_excluded = any_excluded || excluded;
        }
    };
    for c in list_candidates(&ctx.repos, &atom.category, &atom.package)?.iter() {
        fold(format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
        ));
    }
    for c in list_binary_candidates(&ctx.local_binpkg, &atom.category, &atom.package) {
        fold(format!(
            "{}/{}-{}:{}/{}::{}",
            atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
        ));
    }
    if ctx.getbinpkg {
        for c in list_remote_binary_candidates(
            &config.binrepos,
            ctx.root,
            &ctx.local_binpkg,
            &atom.category,
            &atom.package,
        ) {
            fold(format!(
                "{}/{}-{}:{}/{}::{}",
                atom.category, atom.package, c.version, c.slot, c.sub_slot, c.repo_name
            ));
        }
    }
    for (version, slot, sub_slot) in installed_candidates(ctx.root, &atom.category, &atom.package) {
        fold(format!(
            "{}/{}-{version}:{slot}/{sub_slot}",
            atom.category, atom.package
        ));
    }
    Ok(any_match && any_excluded)
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
        // `--exclude`: an explicitly requested argument left with no
        // selectable candidate, where at least one match is excluded,
        // resolves to nothing silently (`Total: 0`, exit 0) instead of
        // aborting with "there are no ebuilds". Set members never take
        // this path (their miss lands in `missing_args` below, like
        // Portage's `SetArg` handling, which never consults the excluded
        // set).
        // Portage: depgraph.py, the `if excluded: continue` in the
        // argument loop (inside `if not pkg:`, so a selectable match
        // never reaches it — see `top_level_atom_excluded`).
        if !ctx.set_args.contains(current_atom)
            && top_level_atom_excluded(ctx, config, current_atom, atom)?
        {
            return Ok(ControlFlow::Break(()));
        }
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
        .flatten()
        // Without an ebuild pool (`--usepkgonly`, or `=n`'s dropped database)
        // an `--update` argument that matches an installed instance selects
        // it: nothing to merge, not an error.
        // Portage: _emerge/depgraph.py, _select_files (installed db in `dbs`)
        .or_else(|| {
            (ctx.update && (ctx.usepkgonly || crate::globals::ebuild_db_dropped()))
                .then(|| {
                    complete_mode_withheld_version(ctx.root, &key.0, &key.1, current_atom, &[])
                })
                .flatten()
        });
        if let Some(version) = withheld_version {
            *outcome = PretendOutcome::AlreadyInstalled { version };
        } else {
            // Under `--usepkgonly` ebuilds are ineligible rather than
            // masked, so a masked-only binary pool reports its own rows
            // (S4; Portage lists the binary, e.g. S0 `F-binary-rewritten`).
            // Otherwise the ebuild-side report below applies unchanged.
            if ctx.usepkgonly {
                let binaries = list_binary_candidates(&ctx.local_binpkg, &key.0, &key.1);
                if let Some(report) = all_binary_masked_report(
                    &binaries,
                    current_atom,
                    config,
                    &format!("{current_atom:?}"),
                ) {
                    return Err(Error::Detail(report));
                }
            }
            // An atom matching only masked ebuilds reports the "all masked"
            // block, not the "no ebuilds" one. A set member additionally
            // names its set (`(dependency required by "@set" [argument])`).
            // Portage: _show_unsatisfied_dep
            if let Some(report) = all_masked_report(
                &ctx.repos,
                current_atom,
                config,
                &format!("{current_atom:?}"),
                ctx.top_level.sets.get(current_atom).map(String::as_str),
            ) {
                return Err(Error::Detail(report));
            }
            // Under `--getbinpkg` an atom matching only masked *remote*
            // binaries reports the same block over the remote pool (the
            // ebuild pool above has no version match here, or the walk
            // would have selected it). Local `PKGDIR` binaries keep the
            // miss text below (no cell covers that shape).
            // Portage: _show_unsatisfied_dep over the bintree;
            // S0 cell `g07-partly-excluded`.
            if ctx.getbinpkg {
                let remote = list_remote_binary_candidates(
                    &config.binrepos,
                    ctx.root,
                    &ctx.local_binpkg,
                    &key.0,
                    &key.1,
                );
                if let Some(report) = all_binary_masked_report(
                    &remote,
                    current_atom,
                    config,
                    &format!("{current_atom:?}"),
                ) {
                    return Err(Error::Detail(report));
                }
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
                    "there are no ebuilds built with USE flags to satisfy {current_atom:?}{}.\n\
                         !!! One of the following packages is required to complete your request:",
                    crate::display_root_suffix()
                );
                for (cpv, reasons) in rows {
                    block.push_str(&format!("\n- {cpv} ({})", reasons.join(", ")));
                }
                return Err(Error::Detail(block));
            }
            let noun = if ctx.usepkgonly {
                "binary packages"
            } else {
                "ebuilds"
            };
            let mut message = format!(
                "there are no {noun} to satisfy {current_atom:?}{}.",
                crate::display_root_suffix()
            );
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
