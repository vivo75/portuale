//! A dependency only a parent's USE flip can satisfy: record the flip and re-walk.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`parent_use_flip`] reads from its caller.
pub(crate) struct ParentUseFlipArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) depth: u32,
    pub(crate) current_atom: String,
    pub(crate) atom: Atom,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) state: &'a mut PassState,
    pub(crate) config: &'a Config,
    pub(crate) key: &'a (String, String),
    pub(crate) buildtime_hard: bool,
}

/// A dependency only a parent's USE flip can satisfy: record the flip and re-walk.
pub(crate) fn parent_use_flip(
    args: ParentUseFlipArgs<'_>,
) -> Result<ControlFlow<(), (String, Atom)>, Error> {
    let ParentUseFlipArgs {
        outcome,
        bp,
        depth,
        current_atom,
        atom,
        owner,
        unevaluated_atom,
        ctx,
        state,
        config,
        key,
        buildtime_hard,
    } = args;
    if !(matches!(outcome, PretendOutcome::NoVisibleCandidate)
        && bp.autounmask_suggest_use
        && depth > 0)
    {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    }
    let (Some(owner_key), Some(unevaluated)) = (owner.as_ref(), unevaluated_atom.as_deref()) else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let Some((pc, pp, _pv, target_use)) =
        suggested_parent_use_candidate(ctx.gate(), &state.entries, unevaluated, owner_key, config)
    else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let Some((parent_cand, _piuse, parent_use, _pru)) =
        parent_use_state(ctx.gate(), &state.entries, owner_key, config)
    else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let mut new_parent_use = parent_use.as_ref().clone();
    for (flag, want) in &target_use {
        if *want {
            new_parent_use.insert(flag.clone());
        } else {
            new_parent_use.remove(flag);
        }
    }
    let Some(re_atom) = portage_dep::evaluate_atom_conditionals(unevaluated, &new_parent_use)
    else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let Some(_re_parsed) = parse_dep_atom(&re_atom, "pass_parent_use_flip") else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let re_outcome = resolve_pretend(
        &SelectSource {
            root: ctx.root,
            config,
            local_binpkg: &ctx.local_binpkg,
            gate: ctx.gate(),
        },
        &re_atom,
        &SelectOptions {
            is_top_level: depth == 0,
            depth,
            buildtime_hard,
            ..ctx.select_options(bp)
        },
    )?;
    if matches!(re_outcome, PretendOutcome::NoVisibleCandidate) {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    }

    // Record the flip and FAIL, with `--autounmask-backtrack` on or off:
    // no merge rows, just the change block (and the "terminated early"
    // notice when backtracking is off). Portage never restarts for a parent
    // flip, `_apply_parent_use_changes` only turns the failed dependency's
    // suggestions into changes (bed `l0-fixture-oracle-438.txt`). The rescued
    // dep's key latches so its miss is suppressed.
    // Portage: depgraph.py, _apply_parent_use_changes
    let parent_all = ctx.repo_candidates(&pc, &pp).unwrap_or_default();
    let token = target_use
        .iter()
        .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
        .collect::<Vec<_>>()
        .join(" ");
    state.autounmask_use_changes.push(AutounmaskChange {
        atom: autounmask_use_atom_form(&parent_cand, &parent_all, &pc, &pp, config),
        token,
        // No single forcing parent: legacy fill.
        trigger: None,
        mask_entry: None,
        // Pre-fill the chain for a top-level parent (Portage never prints
        // the start node); other parents keep the generic fill.
        // Portage: depgraph.py, _get_dep_chain
        dep_chain: ctx
            .top_level
            .iter()
            .find(|t| {
                parse_dep_atom(t, "pass_parent_use_flip").is_some_and(|a| {
                    a.category == pc && a.package == pp && a.blocker == portage_dep::Blocker::None
                })
            })
            .map(|t| vec![format!("required by {} (argument)", ctx.top_level.label(t))])
            .unwrap_or_default(),
    });
    // A parent-flip rescue pre-empts the `_success_without_autounmask`
    // tail; the notice stays.
    state.parent_flip_rescued = true;
    state
        .parent_flip_recorded
        .insert((key.0.clone(), key.1.clone()));
    let (disp, pv_disp, p_disp) =
        flipped_parent_display(&parent_cand, &pc, &pp, &new_parent_use, config);
    if let Some(pe) = state
        .entries
        .iter_mut()
        .find(|e| e.category == pc && e.package == pp)
    {
        pe.use_flags_display = disp;
        pe.use_expand_display = pv_disp;
        pe.use_expand_display_p = p_disp;
    }
    // Record-and-fail: the dep keeps its miss, so only the change
    // block and the notice render.
    Ok(ControlFlow::Continue((current_atom, atom)))
}

/// The flag list and the `-pv` / `-p` `USE_EXPAND` renderings.
pub(crate) type FlippedParentDisplay = (
    Vec<(String, bool)>,
    Vec<(String, String)>,
    Vec<(String, String)>,
);

/// The parent's USE columns once the recorded flip is applied (`new_parent_use`),
/// for the record-and-fail display: the flag list, then the two `USE_EXPAND`
/// renderings.
// Portage: depgraph.py, _apply_parent_use_changes
pub(crate) fn flipped_parent_display(
    parent_cand: &Candidate,
    pc: &str,
    pp: &str,
    new_parent_use: &std::collections::HashSet<String>,
    config: &Config,
) -> FlippedParentDisplay {
    let parent_cpv = format!("{pc}/{pp}-{}", parent_cand.version);
    let mut disp_seen: HashSet<String> = HashSet::new();
    let mut disp: Vec<(String, bool)> = parent_cand
        .iuse
        .split_whitespace()
        .map(|t| t.trim_start_matches(['+', '-']).to_string())
        .filter(|f| disp_seen.insert(f.clone()))
        .map(|f| {
            let on = new_parent_use.contains(&f);
            (f, on)
        })
        .collect();
    disp.sort_by_key(|p| alnum_sort_key(&p.0));
    let forced = forced_or_masked_flags(ForcedOrMaskedFlagsArgs {
        iuse: &parent_cand.iuse,
        keywords: &parent_cand.keywords,
        candidate_str: &parent_cpv,
        build_id: parent_cand.build_id.as_deref(),
        category: pc,
        package: pp,
        config,
    });
    let no_reinst = HashSet::new();
    let pv_disp = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &disp,
        config,
        installed: None,
        forced: &forced,
        all_flags: true,
        reinst_flags: &no_reinst,
    });
    let p_disp = build_use_expand_display(BuildUseExpandDisplayArgs {
        use_flags_display: &disp,
        config,
        installed: None,
        forced: &forced,
        all_flags: false,
        reinst_flags: &no_reinst,
    });
    (disp, pv_disp, p_disp)
}
