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
    let Some((pc, pp, pv, target_use)) =
        suggested_parent_use_candidate(&ctx.repos, &state.entries, unevaluated, owner_key, config)
    else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let Some((parent_cand, _piuse, parent_use, _pru)) =
        parent_use_state(&ctx.repos, &state.entries, owner_key, config)
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
    let Some(_re_parsed) = portage_dep::parse_atom(&re_atom) else {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    };
    let re_outcome = resolve_pretend(
        &SelectSource {
            repos: &ctx.repos,
            root: ctx.root,
            config,
            local_binpkg: &ctx.local_binpkg,
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

    if !ctx.autounmask_backtrack_enabled {
        // Default (`--autounmask-backtrack` off): record the flip and FAIL --
        // no merge rows, just the change block and the "terminated early"
        // notice. The rescued dep's key latches so its miss is suppressed.
        // Portage: depgraph.py, _apply_parent_use_changes
        let parent_cpv = format!("{pc}/{pp}-{pv}");
        let parent_all = list_candidates(&ctx.repos, &pc, &pp).unwrap_or_default();
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
            // Pre-fill the chain for a top-level parent (Portage never prints
            // the start node); other parents keep the generic fill.
            // Portage: depgraph.py, _get_dep_chain
            dep_chain: ctx
                .top_level
                .iter()
                .find(|t| {
                    portage_dep::parse_atom(t).is_some_and(|a| {
                        a.category == pc
                            && a.package == pp
                            && a.blocker == portage_dep::Blocker::None
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
            category: &pc,
            package: &pp,
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
    } else {
        // `--autounmask-backtrack=y`: fold the flip into `autounmask_use_config`
        // and let the driver re-walk, so `flag?`-gated deps re-evaluate. A
        // contradicting flip is `_autounmask_breakage` territory.
        // Portage: _needed_use_config_changes, _backtrack_depgraph
        let key_pp = (pc.clone(), pp.clone());
        // The opposite-flip check reads the effective (overlay-then-`bp`) value.
        if target_use.iter().any(|(f, want)| {
            overlay_use_want(&state.use_overlay, &bp.autounmask_use_config, &key_pp, f)
                .is_some_and(|p| p != *want)
        }) {
            state.use_broke = true;
            return Ok(ControlFlow::Continue((current_atom, atom)));
        }
        // `newly` is computed against the effective value too (a flip folded on
        // an earlier pass lives in `bp`, not the overlay); writes go to the
        // overlay only, `collect_feedback` + driver unions them into `bp`.
        let mut newly = false;
        for (f, want) in &target_use {
            if overlay_use_want(&state.use_overlay, &bp.autounmask_use_config, &key_pp, f)
                != Some(*want)
            {
                state
                    .use_overlay
                    .entry(key_pp.clone())
                    .or_default()
                    .insert(f.clone(), *want);
                newly = true;
            }
        }
        if !newly {
            // Already folded on an earlier pass yet still failing: let the
            // ordinary miss path render it.
            return Ok(ControlFlow::Continue((current_atom, atom)));
        }
        let parent_all = list_candidates(&ctx.repos, &pc, &pp).unwrap_or_default();
        let token = target_use
            .iter()
            .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
            .collect::<Vec<_>>()
            .join(" ");
        let atom_form = autounmask_use_atom_form(&parent_cand, &parent_all, &pc, &pp, config);
        // Dedup spans the union (overlay plus `bp`'s committed records).
        if !state
            .use_change_overlay
            .iter()
            .any(|c| c.atom == atom_form && c.token == token)
            && !bp
                .autounmask_use_change_records
                .iter()
                .any(|c| c.atom == atom_form && c.token == token)
        {
            state.use_change_overlay.push(AutounmaskChange {
                atom: atom_form,
                token,
                dep_chain: Vec::new(),
                // No single forcing parent: legacy fill.
                trigger: None,
            });
        }
        state.autounmask_grew = true;
        Ok(ControlFlow::Break(()))
    }
}
