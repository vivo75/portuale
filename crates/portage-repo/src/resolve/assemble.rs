//! Assembling the final result.

use crate::*;
use portage_profile::Config;

pub(crate) fn assemble_result(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    mut pass: PassResult,
    config: &portage_profile::Config,
    // The search's restart count (Portage `_backtrack_depgraph`'s `backtracked`).
    backtrack_restarts: u64,
) -> GraphResult {
    // Slot-operator auto-rebuild: the consumers are walked nodes now, so
    // nothing is synthesised here -- only the `_forced_rebuilds` display
    // pairs are recomputed (the `get_best_run` re-pass needs the fallback).
    // Portage: depgraph.py.
    let bt = std::env::var("PORTUALE_BT_TRACE").is_ok_and(|value| value == "1");
    let taken = match &pass.abi_rebuilds {
        None => "none".to_string(),
        Some(pairs) => format!("Some({})", pairs.len()),
    };
    let mut abi_rebuilds = match pass.abi_rebuilds.take() {
        Some(pairs) => pairs,
        None if ctx.ignore_built_slot_operator_deps || !ctx.rebuild_if_new_slot => Vec::new(),
        None => {
            // Raw request atoms for the scan's top-level gate.
            let top_atoms: HashSet<String> =
                ctx.top_level.iter().map(|s| (*s).to_string()).collect();
            let scan_scope = ctx.scan_reachable(&pass.entries);
            // Portage files the walked-consumer rebuild only while it may backtrack.
            // Portage: depgraph.py, _slot_conflict_backtrack_abi (_allow_backtracking)
            let walked = match ctx.backtrack_max {
                0 => HashSet::new(),
                _ => installed_walked_cps(&pass.installed_walked),
            };
            slot_operator_rebuild_scan(SlotOperatorRebuildScanArgs {
                root: ctx.root,
                entries: &pass.entries,
                reachable: &ctx.slot_op_reachable,
                scope: Some(&scan_scope),
                walked_installed: &walked,
                already: &params.slot_operator_replace_installed,
                undone: &params.slot_operator_undone,
                with_bdeps: ctx.with_bdeps,
                update: ctx.update,
                top_level_cps: &ctx.top_level_cps,
                top_level_atoms: &top_atoms,
                excluded: ctx.excluded,
                config,
                gate: ctx.gate(),
            })
            .1
        }
    };
    // Continuity pairs for conflict-fired rebuilds (the firing pass
    // restarts, so the scan never sees the edge on the settling pass).
    // Unioned with the scan's pairs and deduped.
    let scanned = abi_rebuilds.len();
    abi_rebuilds.extend(slot_conflict_abi_display_pairs(
        ctx.root,
        &pass.entries,
        &params.slot_operator_replace_installed,
        ctx.with_bdeps,
    ));
    let extended = abi_rebuilds.len();
    super::feedback::slot_conflict_backtrack::drop_masked_pairs(
        ctx.root,
        params,
        &mut abi_rebuilds,
    );
    if bt {
        // `PORTUALE_BT_TRACE=1` only: which `assemble_result` call reports
        // which pair list, and how much `drop_masked_pairs` removed.
        eprintln!(
            "BT-ASSEMBLE abi taken={taken} scanned={scanned} extended={extended} kept={} {abi_rebuilds:?}",
            abi_rebuilds.len()
        );
    }
    abi_rebuilds.sort();
    abi_rebuilds.dedup();

    // `--rebuild-if-unbuilt` / `--rebuild-if-new-rev` / `--rebuild-if-new-ver`:
    // an installed package whose build-time dep is merged this run gets its
    // own `[ebuild R]` rebuild entry.
    // Portage: _rebuild_config.trigger_rebuilds.
    pass.entries
        .extend(rebuild_if_entries(RebuildIfEntriesArgs {
            root: ctx.root,
            entries: &pass.entries,
            unbuilt: ctx.rebuild_if_unbuilt,
            new_rev: ctx.rebuild_if_new_rev,
            new_ver: ctx.rebuild_if_new_ver,
            rebuild_exclude: ctx.rebuild_exclude,
            rebuild_ignore: ctx.rebuild_ignore,
        }));

    // The `--rebuild-if-*` restart record: seeded parents that settled as
    // merges carry the red `r` (Portage's `__auto_rebuild__`
    // `force_reinstall`), the merged dep stays untagged.
    let mut rebuild_if_reinstalls: Vec<(String, String)> = params
        .rebuild_if_replace_installed
        .iter()
        .filter(|cp| {
            pass.entries.iter().any(|e| {
                e.category == cp.0
                    && e.package == cp.1
                    && matches!(
                        e.outcome,
                        PretendOutcome::New { .. }
                            | PretendOutcome::Upgrade { .. }
                            | PretendOutcome::Downgrade { .. }
                            | PretendOutcome::Reinstall { .. }
                    )
            })
        })
        .cloned()
        .collect();
    rebuild_if_reinstalls.sort();
    rebuild_if_reinstalls.dedup();

    // `emerge --pretend --debug` stages 2/4/6: the per-package resolution
    // narration, emitted here on the final entries (before the merge-order
    // sort / `digraph:` dump, matching Portage's order).
    resolver_trace::dump_resolution_walk(&pass.entries, ctx.root, &pass.parent_atoms, &ctx.repos);

    // Portage's `mylist` is dependency-first; portuale's BFS builds `entries`
    // the other way. Re-sort into merge order now that every `required_by`
    // edge is known; the insertion rank travels alongside for the cycle
    // reporter below.
    let (sorted_entries, _) = topological_merge_order(TopologicalMergeOrderArgs {
        entries: pass.entries,
        top_level_atoms: ctx.atoms,
        config,
        root: ctx.root,
        implicit_system_deps: ctx.implicit_system_deps,
        repos: &ctx.repos,
        dynamic_deps: ctx.dynamic_deps,
        circular: &params.circular_dependency,
    });
    pass.entries = sorted_entries;

    let BuildCycleDisplayOut {
        buildpkgonly_deps_unsatisfied,
        circular_deps,
        large_cycle_count,
        report_cycles,
        cycle_display,
        cycle_display_idx,
    } = build_cycle_display(ctx, &pass, params);

    collect_autounmask_and_conflicts(params, &mut pass, ctx, config);

    collect_withheld_updates(ctx, params, config, &mut pass);

    finalize_dep_reports(&mut pass, ctx);

    // Classify the settled graph the way Portage's abandon sites would
    // have (see `abort_outcome`); the virtual-cycle probe runs on the same
    // settled graph, unconditionally but with a cheap gate.
    // Portage: _virt_deps_visible.
    let virtual_cycle = find_virtual_cycles(&pass.entries, ctx.root);
    let outcome = abort_outcome(AbortOutcomeArgs {
        entries: &pass.entries,
        masked_deps: &pass.masked_deps,
        circular_deps: &circular_deps,
        cycle_display: &cycle_display,
        cycle_display_idx: &cycle_display_idx,
        nvc_dep_atoms: &pass.nvc_dep_atoms,
        virtual_cycle: &virtual_cycle,
    });

    // Portage shows missed updates only with no unresolved conflicts: the
    // settled backtrack masks become skip records only on a clean `Complete`
    // graph. A mask-free search (including every `--backtrack=0` run)
    // derives nothing here. Portage: display_problems.
    let mut skipped_missing_deps: Vec<SkippedMissingDep> = Vec::new();
    let mut skipped_missing_dep_full: Vec<SkippedMissingDepFull> = Vec::new();
    if matches!(outcome, ResolveOutcome::Complete)
        && pass.slot_conflicts.is_empty()
        && pass.orphan_blockers.is_empty()
        && !pass
            .entries
            .iter()
            .any(|e| e.blockers.iter().any(|b| b.unsolvable))
    {
        let (mask_skipped, mask_missing, mask_full) =
            backtrack_missed_updates(BacktrackMissedUpdatesArgs {
                root: ctx.root,
                config,
                entries: &pass.entries,
                params,
                missing_args: &pass.missing_args,
                gate: ctx.gate(),
            });
        // Mask rows lead in first-seen order; both sources collapse per
        // `(root, slot_atom)` keeping the highest missed pkg. No re-sort:
        // alphabetical would scramble the mask insertion order Portage
        // renders in. Portage: depgraph.py.
        //
        // A slot the pass already holds is Portage's single
        // `_conflict_missed_update` entry at the solver's position, not a
        // leading `_runtime_pkg_mask` one: such a slot's rows move to where
        // its first pass row sits; slots only the mask knows keep leading.
        let slot_of = |s: &SkippedUpdate| (s.category.clone(), s.package.clone(), s.slot.clone());
        let mask_len = mask_skipped.len();
        let mut anchor: HashMap<(String, String, String), usize> = HashMap::new();
        for (i, s) in pass.skipped_updates.iter().enumerate() {
            anchor.entry(slot_of(s)).or_insert(mask_len + i);
        }
        let mut combined = mask_skipped;
        combined.append(&mut pass.skipped_updates);
        let mut keyed: Vec<(usize, SkippedUpdate)> = combined
            .into_iter()
            .enumerate()
            .map(|(i, s)| (anchor.get(&slot_of(&s)).copied().unwrap_or(i), s))
            .collect();
        keyed.sort_by_key(|(k, _)| *k);
        let combined: Vec<SkippedUpdate> = keyed.into_iter().map(|(_, s)| s).collect();
        pass.skipped_updates = collapse_skipped_updates(combined);
        skipped_missing_deps = mask_missing;
        skipped_missing_dep_full = mask_full;
        for row in &mut skipped_missing_dep_full {
            if let SkippedDepDisclosure::Plain(report) = &mut row.dep {
                report.binary_only = ctx.usepkgonly;
            }
        }
    }

    // The success-path ignored-user-patches rows: for every
    // merging cp, the filter-dropped binaries Portage accumulates in
    // `ignored_binaries["user_patches"]`, minus `_eliminate_ignored_binaries`.
    // Recomputed here from the settled merge list (Portage's displayed-list
    // filter) because selection's retains are pure and memoized.
    let mut merging: Vec<((String, String), Vec<SelectedPackage<'_>>)> = Vec::new();
    let mut merging_idx: HashMap<(String, String), usize> = HashMap::new();
    if ctx.binpkg_respect_user_patches {
        for e in &pass.entries {
            let version = match &e.outcome {
                PretendOutcome::New { version } => Some(version.as_str()),
                PretendOutcome::Upgrade { to, .. } => Some(to.as_str()),
                PretendOutcome::Downgrade { to, .. } => Some(to.as_str()),
                PretendOutcome::Reinstall { version, .. } => Some(version.as_str()),
                PretendOutcome::Uninstall { .. }
                | PretendOutcome::NoVisibleCandidate
                | PretendOutcome::AlreadyInstalled { .. } => None,
            };
            let Some(version) = version else {
                continue;
            };
            let key = (e.category.clone(), e.package.clone());
            let idx = *merging_idx.entry(key.clone()).or_insert_with(|| {
                merging.push((key, Vec::new()));
                merging.len() - 1
            });
            merging[idx].1.push(SelectedPackage {
                version,
                source: e.source,
                slot: e.slot.as_deref().unwrap_or("0"),
            });
        }
    }
    let ignored_user_patch_binaries =
        ignored_user_patches_for_merging(IgnoredUserPatchesForMergingArgs {
            usepkg: ctx.usepkg,
            usepkgonly: ctx.usepkgonly,
            getbinpkg: ctx.getbinpkg,
            respect: ctx.binpkg_respect_user_patches,
            local_binpkg: &ctx.local_binpkg,
            binrepos: &config.binrepos,
            root: ctx.root,
            config,
            merging: &merging,
        });
    let slot_conflict_solutions = solve_slot_conflict_records(SolveRecordsArgs {
        conflicts: &pass.slot_conflicts,
        config,
        root: ctx.root,
        gate: ctx.gate(),
    });
    GraphResult {
        entries: pass.entries,
        backtrack_restarts,
        backtrack_max: ctx.backtrack_max,
        outcome,
        slot_conflicts: pass.slot_conflicts,
        slot_conflict_solutions,
        skipped_updates: pass.skipped_updates,
        skipped_missing_deps,
        skipped_missing_dep_full,
        orphan_blockers: pass.orphan_blockers,
        changed_deps_report: pass.changed_deps_report_entries,
        ignored_user_patch_binaries,
        buildpkgonly_deps_unsatisfied,
        prune_unresolved: false,
        pprovided_atoms: pass.pprovided_atoms,
        missing_args: pass.missing_args,
        autounmask_keyword_changes: pass.autounmask_keyword_changes,
        autounmask_use_changes: pass.autounmask_use_changes,
        autounmask_license_changes: pass.autounmask_license_changes,
        autounmask_mask_changes: pass.autounmask_mask_changes,
        // The settled accumulator may still hold a flip folded for an
        // already-graphed package (or a parent-flip rescue): either way the
        // clean tail was pre-empted, so report no clean tail.
        autounmask_no_clean_tail: !params.autounmask_use_config.is_empty()
            || pass.parent_flip_rescued,
        settled_with_backtracking: false,
        abi_rebuilds,
        rebuild_if_reinstalls,
        slot_operator_replaced: params.probe_replaced.iter().cloned().collect(),
        circular_deps,
        circular_dependency: params.circular_dependency.clone(),
        masked_deps: pass.masked_deps,
        use_unsat_deps: pass.use_unsat_deps,
        plain_miss_deps: pass.plain_miss_deps,
        large_cycle_count,
        report_cycles,
        cycle_display,
        virtual_cycle,
    }
}

/// What [`build_cycle_display`] hands back to its caller.
pub(crate) struct BuildCycleDisplayOut {
    pub(crate) buildpkgonly_deps_unsatisfied: bool,
    pub(crate) circular_deps: Vec<Vec<String>>,
    pub(crate) large_cycle_count: bool,
    pub(crate) report_cycles: Vec<Vec<String>>,
    pub(crate) cycle_display: Vec<String>,
    pub(crate) cycle_display_idx: Vec<usize>,
}

/// Check the buildpkgonly dependencies and find the unbreakable cycles with their display order.
pub(crate) fn build_cycle_display(
    ctx: &ResolveCtx<'_>,
    pass: &PassResult,
    params: &BacktrackParams,
) -> BuildCycleDisplayOut {
    // See GraphResult::buildpkgonly_deps_unsatisfied's doc comment.
    let buildpkgonly_deps_unsatisfied = ctx.buildpkgonly && {
        let needs_action: HashSet<(String, String)> = pass
            .entries
            .iter()
            .filter(|e| {
                // A removal is not an action that satisfies a buildpkgonly
                // dependency.
                !matches!(
                    e.outcome,
                    PretendOutcome::AlreadyInstalled { .. }
                        | PretendOutcome::NoVisibleCandidate
                        | PretendOutcome::Uninstall { .. }
                )
            })
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let merge_edge = pass.entries.iter().any(|e| {
            needs_action.contains(&(e.category.clone(), e.package.clone()))
                && e.required_by
                    .iter()
                    .any(|owner| needs_action.contains(owner))
        });
        // A blocker-paired uninstall ordered after a merge-bound entry is a
        // pending hard dependency, so `--buildpkgonly` refuses to resolve.
        // Portage: depgraph.py.
        let blocker_edge = pass.entries.iter().any(|e| {
            matches!(e.outcome, PretendOutcome::Uninstall { .. })
                && e.required_by
                    .iter()
                    .any(|owner| needs_action.contains(owner))
        });
        merge_edge || blocker_edge
    };

    // An unbreakable build-time cycle among the merge-bound packages, for
    // `pretend.rs` to render the fatal `* Error: circular dependencies:`
    // block. Portage: _serialize_tasks -> _show_circular_deps.
    let mut circular_deps = find_hard_cycles(FindHardCyclesArgs {
        entries: &pass.entries,
        edge_kind_map: &pass.edge_kind_map,
        root: ctx.root,
        circular: &params.circular_dependency,
        top_level_atoms: ctx.atoms,
        onlydeps: ctx.onlydeps,
    });

    // A persisting restart cycle the hard reporter cannot see: backtracking
    // never ran (`--backtrack=0`) or a retry ran and the demotion did not
    // break it. Reported in the same `circular_deps` shape instead of
    // settling `Complete` over an unlinearizable graph -- gated to
    // otherwise-clean passes (an unsatisfiable dep, slot conflict or orphan
    // blocker is what Portage reports first) and to an empty hard report.
    // Portage: get_cycles over the `medium_soft` rung; `unsolved_cycle`.
    let pass_has_nvc = pass.suppressed_nvc
        || pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));
    if circular_deps.is_empty()
        && (ctx.backtrack_max == 0 || !params.circular_dependency.is_empty())
        && !pass_has_nvc
        && pass.slot_conflicts.is_empty()
        && pass.orphan_blockers.is_empty()
    {
        for cycle in find_restart_cycles(
            &pass.entries,
            &pass.edge_kind_map,
            ctx.root,
            &params.circular_dependency,
        ) {
            let mut cpvs: Vec<String> = cycle
                .iter()
                .filter_map(|&i| merge_bound_cpv(&pass.entries[i]))
                .collect();
            if cpvs.len() != cycle.len() {
                continue;
            }
            // Portage's `get_cycles` prints from the earliest-inserted node:
            // the ring starts at its successor and ends with it (the
            // shortest child-`>`node path for the earliest node). Rotate the
            // index cycle so the lowest-`discovery` node is last;
            // this dedupes repeats into one ring.
            let min_pos = cycle
                .iter()
                .enumerate()
                .min_by_key(|&(_, &i)| (pass.entries[i].discovery, i))
                .map(|(p, _)| p)
                .unwrap_or(0);
            let len = cpvs.len();
            cpvs.rotate_left((min_pos + 1) % len);
            if !circular_deps.contains(&cpvs) {
                circular_deps.push(cpvs);
            }
        }
    }

    // Cycle enumeration + reduced display order for the `large_cycle_count`
    // trailer and the cycle-only re-display. Built on demand: acyclic
    // resolves pay nothing. Portage: circular_dependency_handler.
    let (large_cycle_count, report_cycles, cycle_display, cycle_display_idx) =
        if circular_deps.is_empty() {
            (false, Vec::new(), Vec::new(), Vec::new())
        } else {
            let (cycles, display) = merge_order::cycle_report(
                &pass.entries,
                ctx.atoms,
                ctx.root,
                &params.circular_dependency,
                ctx.onlydeps,
            );
            let display_idx: Vec<usize> = display
                .into_iter()
                .filter(|&i| merge_bound_cpv(&pass.entries[i]).is_some())
                .collect();
            let display = display_idx
                .iter()
                .filter_map(|&i| merge_bound_cpv(&pass.entries[i]))
                .collect();
            let report_cycles = cpv_cycles(&pass.entries, &cycles);
            (cycles.len() > 3, report_cycles, display, display_idx)
        };
    BuildCycleDisplayOut {
        buildpkgonly_deps_unsatisfied,
        circular_deps,
        large_cycle_count,
        report_cycles,
        cycle_display,
        cycle_display_idx,
    }
}

/// The elementary cycles (entry indices) as merge-bound CPV lists, in
/// enumeration order.
pub(crate) fn cpv_cycles(entries: &[GraphEntry], cycles: &[Vec<usize>]) -> Vec<Vec<String>> {
    cycles
        .iter()
        .map(|c| {
            c.iter()
                .filter_map(|&i| merge_bound_cpv(&entries[i]))
                .collect()
        })
        .collect()
}

/// Fold in the autounmask USE changes and the residual installed-instance slot conflicts.
pub(crate) fn collect_autounmask_and_conflicts(
    params: &BacktrackParams,
    pass: &mut PassResult,
    ctx: &ResolveCtx<'_>,
    config: &Config,
) {
    // `--autounmask-use` changes from the already-resolved-slot re-check:
    // the graph settled with those flips applied, so surface them in the
    // same block as the fresh-path ones. Deduped by `(atom, token)`.
    for rec in &params.autounmask_use_change_records {
        if !pass
            .autounmask_use_changes
            .iter()
            .any(|c| c.atom == rec.atom && c.token == rec.token)
        {
            pass.autounmask_use_changes.push(rec.clone());
        }
    }

    // Portage emits one line per package (joining every flipped flag of
    // that pkg); portuale accumulates one change per flip site, so coalesce
    // by `atom` in first-seen order.
    {
        let mut merged: Vec<AutounmaskChange> = Vec::new();
        for ch in pass.autounmask_use_changes.drain(..) {
            if let Some(existing) = merged.iter_mut().find(|m| m.atom == ch.atom) {
                let mut flags: Vec<String> = existing
                    .token
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                for f in ch.token.split_whitespace() {
                    if !flags.iter().any(|g| g == f) {
                        flags.push(f.to_string());
                    }
                }
                existing.token = flags.join(" ");
            } else {
                merged.push(ch);
            }
        }
        pass.autounmask_use_changes = merged;
    }

    // With `--autounmask-backtrack` off the driver did NOT re-drive the walk
    // after the backward-cascade fold, so the affected entry still shows its
    // pre-flip USE: re-render just that USE line here (its `flag?`-gated
    // deps still do not appear).
    if !ctx.autounmask_backtrack_enabled && !params.autounmask_use_config.is_empty() {
        let tier = autounmask_use_tier(&params.autounmask_use_config);
        let mut tier_config = ctx.config.clone();
        tier_config.autounmask_use = tier;
        for cp in params.autounmask_use_config.keys() {
            refresh_entry_use_display(ctx.gate(), &mut pass.entries, ctx.root, cp, &tier_config);
        }
    }

    // Residual installed-instance slot conflicts for reverse-dep pins
    // dropped as jointly unsatisfiable: the merge proceeds with the
    // hard-required version, and the broken installed consumers are
    // reported against the installed instance. Appended after every
    // merge-vs-merge record, so those keep their order.
    //
    // Fed through `record_slot_conflict`, not appended blind, so a record
    // the walker's own check already produced for the same triple keeps the
    // union and the single block.
    //
    // Skipped when the walk stopped at an unsatisfied dependency: Portage's
    // stopped graph never reaches the installed instances, so only the
    // complete-graph re-walk may show them here.
    // Portage: depgraph.py, _resolve
    if ctx.complete || !super::feedback::pass_has_unsatisfied_dep(pass) {
        for sc in build_residual_slot_conflicts(BuildResidualSlotConflictsArgs {
            config,
            root: ctx.root,
            entries: &pass.entries,
            slot_pullers: &pass.slot_pullers,
            dropped: &params.dropped_pins,
            gate: ctx.gate(),
        }) {
            record_slot_conflict(&mut pass.slot_conflicts, sc);
        }
    }
}

/// Add the enforced pins that withhold an upgrade to the skipped updates, deduplicated.
pub(crate) fn collect_withheld_updates(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    config: &Config,
    pass: &mut PassResult,
) {
    // Enforced holdable pins that withhold an upgrade join the direct
    // solve's removal rows in the same skip block. Exact-dedupe covers the
    // pathological both-paths shape.
    //
    // Under `--backtrack=0` a slot-operator withhold stays silent, while a
    // plain-pin withhold still synthesizes its rows. `raw_atom` (verbatim
    // recorded text) is what classifies the path: the built `atom` is
    // normalised and strips the slot operator away.
    // Portage: _show_missed_update; the slot-conflict solver's "Record
    // missed updates" tail.
    let bt0_plain_pins: Vec<RevDepPin> = if ctx.backtrack_max == 0 {
        params
            .reverse_dep_pins
            .iter()
            .filter(|pin| {
                !parse_dep_atom(&pin.raw_atom, "assemble")
                    .is_some_and(|a| a.slot_operator.is_some())
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let enforced_pins: &[RevDepPin] = if ctx.backtrack_max == 0 {
        &bt0_plain_pins
    } else {
        &params.reverse_dep_pins
    };
    // The withhold rows keep alphabetical order; the direct solve's rows
    // ahead of them keep Portage's first-seen block order.
    let mut withheld =
        constraint_withheld_updates(ctx.gate(), ctx.root, config, &pass.entries, enforced_pins);
    withheld.sort_by(|a, b| {
        (
            &a.category,
            &a.package,
            &a.skipped_version,
            &a.atom,
            &a.consumer_cpv,
        )
            .cmp(&(
                &b.category,
                &b.package,
                &b.skipped_version,
                &b.atom,
                &b.consumer_cpv,
            ))
    });
    pass.skipped_updates.extend(withheld);
    // First-seen order, like Portage's `_conflict_missed_update` dict --
    // exact duplicates collapse to their first row.
    let mut unique: Vec<SkippedUpdate> = Vec::with_capacity(pass.skipped_updates.len());
    for row in std::mem::take(&mut pass.skipped_updates) {
        if !unique.contains(&row) {
            unique.push(row);
        }
    }
    pass.skipped_updates = unique;
}

/// Fill in the dependency chains and per-entry details of the final dependency reports.
pub(crate) fn finalize_dep_reports(pass: &mut PassResult, ctx: &ResolveCtx<'_>) {
    // Masked-dependency chains are walked out of the final entries
    // (`required_by` is only complete post-pass).
    for rep in &mut pass.masked_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            argument_sets: ctx.top_level.sets,
            root: ctx.root,
            atom: &atom,
        });
    }
    // Same walk for the `[use]`-unsatisfied disclosures.
    // Portage: _get_dep_chain.
    for rep in &mut pass.use_unsat_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            argument_sets: ctx.top_level.sets,
            root: ctx.root,
            atom: &atom,
        });
    }
    // Plain-miss post-pass: every dependency `NoVisibleCandidate` entry
    // neither sibling block claimed gets a disclosure. Placed after the
    // walk so it also catches NVC entries created outside `run_pass`'s own
    // push (e.g. the binary-dep path).
    let mut extra_plain_miss: Vec<PlainMissDepReport> = Vec::new();
    for e in &pass.entries {
        if !matches!(e.outcome, PretendOutcome::NoVisibleCandidate) {
            continue;
        }
        // A dependency collected into a parent USE flip (record-and-fail)
        // is not a miss at all -- the sweep must not re-add it here.
        if e.parent_use_suggestion.is_some() {
            continue;
        }
        let cat = e.category.as_str();
        let pkg = e.package.as_str();
        if pass
            .masked_deps
            .iter()
            .any(|r| r.category == cat && r.package == pkg)
            || pass
                .use_unsat_deps
                .iter()
                .any(|r| r.category == cat && r.package == pkg)
            || pass
                .plain_miss_deps
                .iter()
                .any(|r| r.category == cat && r.package == pkg)
            || extra_plain_miss
                .iter()
                .any(|r| r.category == cat && r.package == pkg)
        {
            continue;
        }
        let key = (e.category.clone(), e.package.clone());
        let atom = pass
            .nvc_dep_atoms
            .get(&key)
            .cloned()
            .unwrap_or_else(|| format!("{}/{}", e.category, e.package));
        extra_plain_miss.push(PlainMissDepReport {
            category: e.category.clone(),
            package: e.package.clone(),
            atom,
            chain: Vec::new(),
            binary_only: false,
        });
    }
    pass.plain_miss_deps.extend(extra_plain_miss);
    for rep in &mut pass.plain_miss_deps {
        rep.binary_only = ctx.usepkgonly;
    }
    // The `#required by …` chains are walked out of the final entries;
    // the walk-time call sites record `Vec::new()`. A change on an
    // argument package yields just the `(argument)` line.
    fill_non_use_change_chains(
        &pass.entries,
        &mut pass.autounmask_keyword_changes,
        &ctx.top_level,
        ctx.root,
    );
    fill_non_use_change_chains(
        &pass.entries,
        &mut pass.autounmask_license_changes,
        &ctx.top_level,
        ctx.root,
    );
    fill_non_use_change_chains(
        &pass.entries,
        &mut pass.autounmask_mask_changes,
        &ctx.top_level,
        ctx.root,
    );
    fill_use_change_chains(
        &pass.entries,
        &mut pass.autounmask_use_changes,
        &ctx.top_level,
        ctx.root,
    );
    // Same walk for the plain-miss disclosures.
    for rep in &mut pass.plain_miss_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            argument_sets: ctx.top_level.sets,
            root: ctx.root,
            atom: &atom,
        });
    }
}
