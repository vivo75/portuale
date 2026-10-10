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
        // Already resolved by an earlier atom. If the current atom rejects the
        // resolved version, report a slot conflict and move on.
        let existing_version = match &state.entries[existing_idx].outcome {
            PretendOutcome::New { version } => version.to_string(),
            PretendOutcome::Upgrade { to, .. } => to.clone(),
            PretendOutcome::Downgrade { to, .. } => to.clone(),
            PretendOutcome::Reinstall { version, .. } => version.to_string(),
            _ => unreachable!(
                "resolved_slots only ever indexes New/Upgrade/Downgrade/Reinstall entries"
            ),
        };
        // Match with the resolved version's own sub-slot: a built slot-operator
        // atom (`cat/pkg:0/2=`) carries an explicit sub-slot and rejects a
        // `:slot`-only string even when the sub-slots agree.
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
            // Rebuild-trigger forgiveness: an installed `:=` consumer whose built
            // atom fails the new sub-slot is a scheduled rebuild, not a conflict --
            // but only with an installed instance the atom still accepts and a
            // backtrack budget (the record drives the ABI probe otherwise).
            // Backlog #290, #214.
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
                    build_slot_conflict(BuildSlotConflictArgs {
                        repos: &ctx.repos,
                        config,
                        root: ctx.root,
                        category: &key.0,
                        package: &key.1,
                        slot,
                        existing_version: &existing_version,
                        current_atom,
                        current_version: version,
                        slot_pullers: &state.slot_pullers,
                        installed_side: None,
                    }),
                );
            }
            return ControlFlow::Break(());
        }
        // A matching bare `:=` can still hide a sub-slot shift when its built
        // binding points at an unrebuildable consumer: record the bound
        // installed instance alongside so the rebuild scan sees the parent.
        // Backlog #90 (S1).
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
                build_slot_conflict(BuildSlotConflictArgs {
                    repos: &ctx.repos,
                    config,
                    root: ctx.root,
                    category: &key.0,
                    package: &key.1,
                    slot,
                    existing_version: &existing_version,
                    current_atom,
                    current_version: &inst_ver,
                    slot_pullers: &state.slot_pullers,
                    installed_side: Some(InstalledSide::Current),
                }),
            );
        }

        // `match_from_list` matched version + slot only. A `[flag]` use-dep the
        // resolved package fails means the instance does NOT satisfy this atom:
        // pull a second instance and report the conflict (Portage carries
        // `("use", flag)` parents); the flip suggestion below still runs, so
        // both the notice and the change block show. No second instance when
        // the fresh pick IS the resolved version (it resolves in place).
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
                let existing_use = effective_use_flags(EffectiveUseFlagsArgs {
                    config,
                    iuse: &existing_cand.iuse,
                    keywords: &existing_cand.keywords,
                    candidate_str: &existing_cand_str,
                    category: &key.0,
                    package: &key.1,
                });
                if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use) {
                    record_slot_conflict(
                        &mut state.slot_conflicts,
                        build_slot_conflict(BuildSlotConflictArgs {
                            repos: &ctx.repos,
                            config,
                            root: ctx.root,
                            category: &key.0,
                            package: &key.1,
                            slot,
                            existing_version: &existing_version,
                            current_atom,
                            current_version: version,
                            slot_pullers: &state.slot_pullers,
                            installed_side: None,
                        }),
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
                let existing_use = effective_use_flags(EffectiveUseFlagsArgs {
                    config,
                    iuse: &existing_cand.iuse,
                    keywords: &existing_cand.keywords,
                    candidate_str: &existing_cand_str,
                    category: &key.0,
                    package: &key.1,
                });
                if !portage_dep::use_deps_satisfied(use_deps, &iuse_set, &existing_use)
                    && let Some(flip) =
                        suggested_use_flip(existing_cand, &key.0, &key.1, use_deps, config)
                {
                    // The match reads the effective (overlay-then-`bp`) value;
                    // non-conflicting flags insert, conflicting ones break.
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
                            // The accumulator already wants this flag the other way:
                            // flipping it would re-break that atom (Portage
                            // `_autounmask_breakage`; the driver gives up).
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
                        // Dedup spans the union (overlay plus `bp`'s records).
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
                                // This dep's owner IS the forcing parent (Portage's
                                // `_get_dep_chain` starts at the requester).
                                // Backlog #244.
                                trigger: owner.clone(),
                            });
                        }
                        // The driver re-drives, not here; only with
                        // `--autounmask-backtrack=y`, else the change is only
                        // recorded and displayed. Portage: _backtrack_depgraph
                        if ctx.autounmask_backtrack_enabled {
                            state.autounmask_grew = true;
                        }
                        // Re-expand this pass's already-flattened deps under the flipped
                        // USE so newly-gated atoms queue in-walk (a no-op when the
                        // flip reveals nothing new). Backlog #218.
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
                            ExpandResolvedSlotWithFlippedUseArgs {
                                ctx,
                                config,
                                union_constraints,
                                state: &mut *state,
                                bp,
                                key,
                                existing_idx,
                                existing_cand,
                                existing_version: &existing_version,
                                new_use: &delta_use,
                            },
                        );
                    }
                }
            }
        }
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}
