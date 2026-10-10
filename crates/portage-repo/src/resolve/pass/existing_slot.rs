//! The slot was already resolved: keep, replace or report a slot conflict.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;

/// The values [`step_existing_slot`] reads from its caller.
pub(crate) struct StepExistingSlotArgs<'a> {
    pub(crate) state: &'a mut PassState,
    pub(crate) slot_key: &'a (String, String, String, bool),
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) slot: &'a str,
    pub(crate) current_atom: &'a str,
    pub(crate) atom: &'a Atom,
    pub(crate) config: &'a Config,
    pub(crate) version: &'a str,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) owner: &'a Option<(String, String)>,
    pub(crate) union_constraints: &'a HashMap<(String, String), Vec<String>>,
}

/// The slot was already resolved: keep, replace or report a slot conflict.
pub(crate) fn step_existing_slot(args: StepExistingSlotArgs<'_>) -> ControlFlow<()> {
    let StepExistingSlotArgs {
        state,
        slot_key,
        ctx,
        key,
        slot,
        current_atom,
        atom,
        config,
        version,
        bp,
        owner,
        union_constraints,
    } = args;
    if let Some(&existing_idx) = state.resolved_slots.get(slot_key) {
        // This exact category/package/slot was already resolved by
        // an earlier atom. If the current atom's own constraint
        // doesn't accept that already-resolved version, it's a real
        // slot conflict -- report it and move on, without a second,
        // independent resolution or any attempt to reconcile the two.
        let existing_version = match &state.entries[existing_idx].outcome {
            PretendOutcome::New { version } => version.to_string(),
            PretendOutcome::Upgrade { to, .. } => to.clone(),
            PretendOutcome::Downgrade { to, .. } => to.clone(),
            PretendOutcome::Reinstall { version, .. } => version.to_string(),
            _ => unreachable!(
                "resolved_slots only ever indexes New/Upgrade/Downgrade/Reinstall entries"
            ),
        };
        // Include the already-resolved version's own sub-slot: a
        // *built* slot-operator atom (`cat/pkg:0/2=`, common in a
        // binary package's recorded RDEPEND) carries an explicit
        // sub-slot and won't match a `:slot`-only string even when
        // the sub-slots agree -- which produced a spurious slot
        // conflict for a package two binary parents both depend on.
        let existing_sub = slot_conflict_meta(&ctx.repos, &key.0, &key.1, &existing_version).0;
        let existing_str = if existing_sub.is_empty() {
            format!("{}/{}-{existing_version}:{slot}", key.0, key.1)
        } else {
            format!(
                "{}/{}-{existing_version}:{slot}/{existing_sub}",
                key.0, key.1
            )
        };
        let satisfied = portage_dep::match_from_list(current_atom, &[existing_str.as_str()])
            .is_some_and(|m| !m.is_empty())
            && !(atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
                && atom.slot.is_none()
                && atom.sub_slot.is_none()
                && built_equals_shift_unrebuildable(BuiltEqualsShiftUnrebuildableArgs {
                    root: ctx.root,
                    repos: &ctx.repos,
                    category: &key.0,
                    package: &key.1,
                    existing_sub: &existing_sub,
                    pullers: &state.slot_pullers,
                    excluded: ctx.excluded,
                }));
        if !satisfied {
            // #290: the same rebuild-trigger forgiveness as the
            // AlreadyInstalled arm above. A provider pulled in only
            // by the slot-operator forced upgrade (`pprov` not
            // requested) reaches this merge arm, where the installed
            // `:=` consumer's built atom (`>=pprov-1:0/1=`) fails the
            // new sub-slot -- a rebuild trigger the scan schedules
            // (`rR pcons`), not a conflict (real 3.0.82.2, bt > 0).
            //
            // Only with an installed instance in this slot that the
            // atom still accepts (the instance the forced upgrade
            // replaces): with none installed (`abiprov`, both
            // versions merge-bound) the record is what drives the
            // conflict-fired ABI rebuild probe (#214), so it stays.
            let installed_accepts = installed_refs(ctx.root, &key.0, &key.1)
                .iter()
                .filter(|r| r.slot == slot)
                .any(|r| {
                    let inst = if r.sub_slot.is_empty() {
                        format!("{}/{}-{}:{slot}", key.0, key.1, r.version)
                    } else {
                        format!("{}/{}-{}:{slot}/{}", key.0, key.1, r.version, r.sub_slot)
                    };
                    portage_dep::match_from_list(current_atom, &[inst.as_str()])
                        .is_some_and(|m| !m.is_empty())
                });
            if !(ctx.backtrack_max > 0
                && installed_accepts
                && built_slot_operator_rebuild_trigger(current_atom, &existing_str))
            {
                record_slot_conflict(
                    &mut state.slot_conflicts,
                    build_slot_conflict(
                        &ctx.repos,
                        config,
                        ctx.root,
                        &key.0,
                        &key.1,
                        slot,
                        &existing_version,
                        current_atom,
                        version,
                        &state.slot_pullers,
                        None,
                    ),
                );
            }
            return ControlFlow::Break(());
        }
        // Backlog #90 (S1): a bare `:=` that *matches* the
        // already-resolved version still hides a sub-slot shift
        // when its built binding points elsewhere and the bound
        // consumer cannot be rebuilt -- the installed-bound
        // instance never gets pulled (the raw `:=` reuses the
        // just-selected one), so the two-instance record above
        // never fires. Record the bound installed instance
        // alongside instead of swallowing it: existing stays the
        // merge, the bound version files as the installed side
        // (the #57 installed-instance pattern), and the
        // need_rebuild scan sees the stuck parent. Rebuildable
        // consumers skip this (their update proceeds with a
        // rebuild, like real) -- see
        // `built_equals_shift_unrebuildable`.
        if atom.slot_operator == Some(portage_dep::SlotOperator::Equals)
            && atom.slot.is_none()
            && atom.sub_slot.is_none()
            && let Some((inst_ver, _)) = built_equals_binding(ctx.root, &ctx.repos, &key.0, &key.1)
            && inst_ver != existing_version
            && built_equals_shift_unrebuildable(BuiltEqualsShiftUnrebuildableArgs {
                root: ctx.root,
                repos: &ctx.repos,
                category: &key.0,
                package: &key.1,
                existing_sub: &existing_sub,
                pullers: &state.slot_pullers,
                excluded: ctx.excluded,
            })
        {
            record_slot_conflict(
                &mut state.slot_conflicts,
                build_slot_conflict(
                    &ctx.repos,
                    config,
                    ctx.root,
                    &key.0,
                    &key.1,
                    slot,
                    &existing_version,
                    current_atom,
                    &inst_ver,
                    &state.slot_pullers,
                    Some(InstalledSide::Current),
                ),
            );
        }

        // `match_from_list` above matched version + slot only. If
        // this atom carries a `[flag]` use-dep, the already-resolved
        // package still has to satisfy it -- and if it doesn't, but
        // a `package.use` flip would (real `--autounmask-use` for a
        // dep on a package already in the graph), fold the flip into
        // `autounmask_use_config` and re-run the whole walk so the
        // package is re-resolved with the flag and its `flag?`-gated
        // deps appear (real `_need_restart` after
        // `_needed_use_config_changes` grows).
        //
        // Real goes one step further first: a USE mismatch means
        // the graphed instance does NOT satisfy this atom, so real
        // pulls a second instance and reports the slot conflict
        // (live-verified: the notice then carries `("use", flag)`
        // parents). `version` above is the fresh USE-filtered pick,
        // so it becomes the conflict's second instance -- unless it
        // is the already-resolved version itself (selection
        // prefers the highest flippable version, so a flippable
        // mismatch resolves in place with an autounmask flip and
        // no second instance exists to report). No `continue`
        // here -- the flip suggestion below still runs (real
        // shows both the notice and the change block).
        if version != existing_version
            && let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty())
        {
            let all_cands = list_candidates(&ctx.repos, &key.0, &key.1).unwrap_or_default();
            if let Some(existing_cand) = all_cands.iter().find(|c| c.version == existing_version) {
                let declared: HashSet<String> = existing_cand
                    .iuse
                    .split_whitespace()
                    .map(|t| t.trim_start_matches(['+', '-']).to_string())
                    .collect();
                let iuse_set = valid_iuse(&declared, config);
                let existing_cand_str = format!(
                    "{}/{}-{existing_version}:{}/{}::{}",
                    key.0,
                    key.1,
                    existing_cand.slot,
                    existing_cand.sub_slot,
                    existing_cand.repo_name
                );
                let existing_use = effective_use_flags(
                    config,
                    &existing_cand.iuse,
                    &existing_cand.keywords,
                    &existing_cand_str,
                    &key.0,
                    &key.1,
                );
                if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use) {
                    record_slot_conflict(
                        &mut state.slot_conflicts,
                        build_slot_conflict(
                            &ctx.repos,
                            config,
                            ctx.root,
                            &key.0,
                            &key.1,
                            slot,
                            &existing_version,
                            current_atom,
                            version,
                            &state.slot_pullers,
                            None,
                        ),
                    );
                }
            }
        }
        if bp.autounmask_suggest_use
            && let Some(use_deps) = atom.use_deps.as_deref().filter(|d| !d.is_empty())
        {
            let all_ebuild_cands = list_candidates(&ctx.repos, &key.0, &key.1).unwrap_or_default();
            if let Some(existing_cand) = all_ebuild_cands
                .iter()
                .find(|c| c.version == existing_version)
            {
                let declared: HashSet<String> = existing_cand
                    .iuse
                    .split_whitespace()
                    .map(|t| t.trim_start_matches(['+', '-']).to_string())
                    .collect();
                let iuse_set = valid_iuse(&declared, config);
                let existing_cand_str = format!(
                    "{}/{}-{existing_version}:{}/{}::{}",
                    key.0,
                    key.1,
                    existing_cand.slot,
                    existing_cand.sub_slot,
                    existing_cand.repo_name
                );
                let existing_use = effective_use_flags(
                    config,
                    &existing_cand.iuse,
                    &existing_cand.keywords,
                    &existing_cand_str,
                    &key.0,
                    &key.1,
                );
                if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use)
                    && let Some(flip) =
                        suggested_use_flip(existing_cand, &key.0, &key.1, use_deps, config)
                {
                    // A3 overlay: the three-way match reads the effective
                    // (overlay-then-`bp`) value. Unlike the parent-flip
                    // site above this one inserts non-conflicting flags
                    // and flags only the conflicting one -- preserved.
                    // Writes go to the overlay only.
                    let mut newly = false;
                    for (f, on) in &flip {
                        match overlay_use_want(
                            &state.use_overlay,
                            &bp.autounmask_use_config,
                            key,
                            f,
                        ) {
                            Some(prev) if prev == *on => {}
                            // The accumulator already wants this
                            // flag the *other* way for a different
                            // atom -- flipping it now would just
                            // re-break that one. Real
                            // `_autounmask_breakage`: give up on
                            // autounmask entirely (handled by the
                            // driver below).
                            Some(_) => state.use_broke = true,
                            None => {
                                state
                                    .use_overlay
                                    .entry(key.clone())
                                    .or_default()
                                    .insert(f.clone(), *on);
                                newly = true;
                            }
                        }
                    }
                    if newly {
                        let token = flip
                            .iter()
                            .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let atom_form = autounmask_use_atom_form(
                            existing_cand,
                            &all_ebuild_cands,
                            &key.0,
                            &key.1,
                            config,
                        );
                        // A3 overlay: dedup spans the union (this pass's
                        // overlay plus `bp`'s committed records), like
                        // the old single vec.
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
                                // Backlog #244: unlike the parent-flip
                                // arm above, this dep's owner *is* the
                                // forcing parent -- its own `[flag]`
                                // atom needs the change on the
                                // already-resolved package (real
                                // `_get_dep_chain` starts at the
                                // requester, not at whoever pulled the
                                // package in first; oracle:
                                // `aubreaktop`, evidence
                                // 2026-09-28-244).
                                trigger: owner.clone(),
                            });
                        }
                        // Real `_backtrack_depgraph` finishes
                        // the attempt, then restarts with the
                        // grown config -- the driver at the
                        // bottom of `'backtrack` does the
                        // `continue`, not here. Only with
                        // `--autounmask-backtrack=y`; off (the
                        // default), the change is recorded and
                        // displayed but the graph is not
                        // re-driven (a post-loop pass refreshes
                        // this package's own USE line).
                        if ctx.autounmask_backtrack_enabled {
                            state.autounmask_grew = true;
                        }
                        // Backlog #218: real needs no re-drive here
                        // at all -- the flip lands while the flipped
                        // package is still stacked, so its own dep
                        // walk already sees it
                        // (`expand_resolved_slot_with_flipped_use`'s
                        // doc comment). Re-expand this pass's
                        // already-flattened deps under the flipped
                        // USE so the newly-gated atoms queue
                        // in-walk; a no-op when the flip reveals
                        // nothing new.
                        let mut delta_use: HashSet<String> = existing_use.as_ref().clone();
                        if let Some(overlay) = state.use_overlay.get(key) {
                            for (f, on) in overlay {
                                if *on {
                                    delta_use.insert(f.clone());
                                } else {
                                    delta_use.remove(f);
                                }
                            }
                        }
                        expand_resolved_slot_with_flipped_use(
                            ctx,
                            config,
                            union_constraints,
                            &mut *state,
                            bp,
                            key,
                            existing_idx,
                            existing_cand,
                            &existing_version,
                            &delta_use,
                        );
                    }
                }
            }
        }
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}
