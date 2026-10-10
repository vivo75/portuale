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
            ..ctx.select_options(bp)
        },
    )?;
    if matches!(re_outcome, PretendOutcome::NoVisibleCandidate) {
        return Ok(ControlFlow::Continue((current_atom, atom)));
    }

    if !ctx.autounmask_backtrack_enabled {
        // Default (real `--autounmask-backtrack` off): the flip
        // is RECORDED into `_needed_use_config_changes` and the
        // resolve FAILS (real `_apply_parent_use_changes` runs
        // only after `_create_graph` already failed,
        // `depgraph.py:5677`, and the `_success_without_
        // autounmask` tail `:5793` returns False) -- no merge
        // rows. The change block plus the "terminated early"
        // notice are the whole output (rc 1). The rescued dep's
        // own key is latched in `parent_flip_recorded` so its
        // masked/use-unsat/plain-miss disclosure is suppressed
        // (real drops the collected item from
        // `_unsatisfied_deps_for_display`, `:5820`
        // `remaining_items`) and the abort renderer skips its
        // row (see the `pretend` NVC loop's own gate).
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
            // A parent-flip rescue names the flipped parent, not
            // the failed dep: no single forcing parent, legacy
            // fill (backlog #205).
            trigger: None,
            // When the change's own package is itself a
            // top-level argument, real's dep chain is just the
            // argument line: `_get_dep_chain` never prints the
            // start node (`elif node is not start_node`,
            // `depgraph.py:6357`), and the failed graph walks
            // straight to the argument (oracle: the aup0b
            // container probe prints a single `# required by
            // =dev-libs/aup0b-1 (argument)`). Pre-fill it (the
            // post-loop fill leaves non-empty chains alone); a
            // non-argument parent keeps the generic fill, which
            // already matches real there (oracle:
            // `abort-au-plain`'s parent-plus-argument rows).
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
                .map(|t| vec![format!("required by {t} (argument)")])
                .unwrap_or_default(),
        });
        // Backlog #217: a failed dep rescued by a parent flip
        // (real `_apply_parent_use_changes` collecting
        // post-failure) pre-empts the `_success_without_
        // autounmask` tail -- the notice stays.
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
        let forced = forced_or_masked_flags(
            &parent_cand.iuse,
            &parent_cand.keywords,
            &parent_cpv,
            &pc,
            &pp,
            config,
        );
        let no_reinst = HashSet::new();
        let pv_disp = build_use_expand_display(&disp, config, None, &forced, true, &no_reinst);
        let p_disp = build_use_expand_display(&disp, config, None, &forced, false, &no_reinst);
        if let Some(pe) = state
            .entries
            .iter_mut()
            .find(|e| e.category == pc && e.package == pp)
        {
            pe.use_flags_display = disp;
            pe.use_expand_display = pv_disp;
            pe.use_expand_display_p = p_disp;
        }
        // Record-and-fail: the dependency keeps its
        // `NoVisibleCandidate` outcome (real's failed
        // `_create_graph`), so no merge rows render -- only the
        // change block and the notice.
        Ok(ControlFlow::Continue((current_atom, atom)))
    } else {
        // `--autounmask-backtrack=y`: fold the flip into
        // `autounmask_use_config` (a `package.use` change on the
        // parent) and let the driver re-walk the whole graph so
        // `flag?`-gated deps of the parent re-evaluate against
        // the flipped state (real `_needed_use_config_changes`
        // -> `_backtrack_depgraph`). A parent flip that
        // contradicts an accumulated change is real
        // `_autounmask_breakage` territory (Slice 3).
        let key_pp = (pc.clone(), pp.clone());
        // A3 overlay: the opposite-flip check reads the effective
        // (overlay-then-`bp`) value, exactly like the old direct
        // read of the single map.
        if target_use.iter().any(|(f, want)| {
            overlay_use_want(&state.use_overlay, &bp.autounmask_use_config, &key_pp, f)
                .is_some_and(|p| p != *want)
        }) {
            state.use_broke = true;
            return Ok(ControlFlow::Continue((current_atom, atom)));
        }
        // A3 overlay: `newly` is computed against the effective
        // value too (a flip folded on an *earlier* pass lives in
        // `bp`, not the overlay -- without this the same flip
        // would re-fire every pass and eat the backtrack budget).
        // Writes go to the overlay only; `collect_feedback` + driver unions them
        // into `bp` before deciding.
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
            // Already folded on an earlier pass yet the dep
            // still failed this one -- let the ordinary
            // NoVisibleCandidate path (with
            // `parent_use_suggestion`) render it.
            return Ok(ControlFlow::Continue((current_atom, atom)));
        }
        let parent_all = list_candidates(&ctx.repos, &pc, &pp).unwrap_or_default();
        let token = target_use
            .iter()
            .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
            .collect::<Vec<_>>()
            .join(" ");
        let atom_form = autounmask_use_atom_form(&parent_cand, &parent_all, &pc, &pp, config);
        // A3 overlay: dedup spans the union (this pass's overlay
        // plus `bp`'s committed records), like the old single vec.
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
                // Backward-cascade overlay: the flip serves an
                // already-resolved slot, and the current dep's
                // owner is often not the forcing parent (e.g.
                // the bare-depending top package) -- no single
                // forcing parent, legacy fill (backlog #205).
                trigger: None,
            });
        }
        state.autounmask_grew = true;
        Ok(ControlFlow::Break(()))
    }
}
