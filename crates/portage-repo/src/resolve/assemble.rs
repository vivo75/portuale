//! Assembling the final result.

use crate::*;

pub(crate) fn assemble_result(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    mut pass: PassResult,
    config: &portage_profile::Config,
    // #59 S2: the search's restart count (`passes - 1` inside
    // `backtracking_resolve`'s loop), real `_backtrack_depgraph`'s
    // `backtracked`.
    backtrack_restarts: u64,
) -> GraphResult {
    // Real depgraph's slot-operator auto-rebuild (#24 S3): the consumers
    // themselves are walked nodes now (seeded + flipped in `run_pass`
    // from `params.slot_operator_replace_installed`), so nothing is
    // synthesised here -- only the `_forced_rebuilds` display pairs are
    // recomputed for `_show_abi_rebuild_info`. `collect_feedback` already
    // ran the scan for every pass it decided; the `get_best_run` re-pass
    // does not go through it, hence the fallback.
    let mut abi_rebuilds = match pass.abi_rebuilds.take() {
        Some(pairs) => pairs,
        None if ctx.ignore_built_slot_operator_deps || !ctx.rebuild_if_new_slot => Vec::new(),
        None => {
            // #268: raw request atoms for the `#252` gate (see above).
            let top_atoms: HashSet<String> =
                ctx.top_level.iter().map(|s| (*s).to_string()).collect();
            slot_operator_rebuild_scan(
                ctx.root,
                &ctx.repos,
                &pass.entries,
                &ctx.slot_op_reachable,
                &params.slot_operator_replace_installed,
                &params.slot_operator_undone,
                ctx.with_bdeps,
                ctx.update,
                &ctx.top_level_cps,
                &top_atoms,
                ctx.excluded,
                config,
            )
            .1
        }
    };
    // Backlog #214 (v2 `#24e`): continuity pairs for conflict-fired
    // rebuilds (see [`slot_conflict_abi_display_pairs`]) -- the firing
    // pass restarts, so the scan never sees the edge on the settling
    // pass. Unioned with the scan's pairs and deduped, so shapes the
    // scan already covers render exactly once.
    abi_rebuilds.extend(slot_conflict_abi_display_pairs(
        ctx.root,
        &pass.entries,
        &params.slot_operator_replace_installed,
        ctx.with_bdeps,
    ));
    abi_rebuilds.sort();
    abi_rebuilds.dedup();

    // Real `_rebuild_config.trigger_rebuilds()` (`--rebuild-if-unbuilt`
    // / `--rebuild-if-new-rev` / `--rebuild-if-new-ver`): an installed
    // package whose build-time dep is being merged this run gets its
    // own `[ebuild R]` rebuild entry. Same `all_installed_packages`
    // scan / entry shape as the slot-operator pass above.
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

    // `emerge --pretend --debug` stages 2/4/6: the per-package
    // resolution narration, emitted here (final `entries`, successful
    // pass) rather than threaded through the BFS -- see
    // `resolver_trace::dump_resolution_walk`. Before the merge-order
    // sort / `digraph:` dump, matching real's order (walk, then
    // graph).
    resolver_trace::dump_resolution_walk(&pass.entries, ctx.root, &pass.parent_atoms, &ctx.repos);

    // Real portage's `mylist` is dependency-first (its Scheduler installs
    // a package only after everything it depends on); portuale's BFS
    // builds `entries` the other way (a package's entry is pushed before
    // its dependencies are ever queued). Re-sort into merge order now
    // that every `required_by` edge is known -- see
    // `topological_merge_order`. The insertion rank travels alongside
    // for the cycle reporter below (backlog #278: real `get_cycles`
    // iterates digraph insertion order).
    let (sorted_entries, insertion_rank) = topological_merge_order(
        pass.entries,
        ctx.atoms,
        config,
        ctx.root,
        ctx.implicit_system_deps,
        &ctx.repos,
        ctx.dynamic_deps,
        &params.circular_dependency,
    );
    pass.entries = sorted_entries;

    // Real depgraph.py:5706-5717 -- see GraphResult::
    // buildpkgonly_deps_unsatisfied's own doc comment.
    let buildpkgonly_deps_unsatisfied = ctx.buildpkgonly && {
        let needs_action: HashSet<(String, String)> = pass
            .entries
            .iter()
            .filter(|e| {
                // #72 B3: a removal is not an action that satisfies a
                // buildpkgonly dependency.
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
        // #143 S1: real's hard blocker edge defeats the all-zeros
        // check the same way — a blocker-paired uninstall ordered
        // after a merge-bound entry (its `required_by` anchor) is a
        // pending hard dependency, so `--buildpkgonly` refuses to
        // resolve (upstream pg1 A/B: the three rows print, then the
        // two `!!!` lines, rc 1). After the S1 gate the only
        // uninstalls that can exist under `buildpkgonly` are
        // strong-buildtime ones, exactly real's abort shape.
        let blocker_edge = pass.entries.iter().any(|e| {
            matches!(e.outcome, PretendOutcome::Uninstall { .. })
                && e.required_by
                    .iter()
                    .any(|owner| needs_action.contains(owner))
        });
        merge_edge || blocker_edge
    };

    // Real `_serialize_tasks` -> `_show_circular_deps`: an
    // unbreakable build-time dependency cycle among the merge-bound
    // packages. `topological_merge_order` above already left such a
    // cycle in discovery order (it can't linearize one); this
    // records it for `pretend.rs` to render the fatal
    // `* Error: circular dependencies:` block.
    let mut circular_deps = find_hard_cycles(
        &pass.entries,
        &pass.edge_kind_map,
        ctx.root,
        &params.circular_dependency,
        Some(&insertion_rank),
    );

    // Backlog #221: a persisting restart cycle the hard reporter cannot
    // see -- either backtracking never ran (`--backtrack=0`, where real
    // fails instead of retrying: pg5's runtime-closing ring) or a retry
    // ran and the demotion did not break it (real's `unsolved_cycle` --
    // e.g. a single-branch `||` closing the ring, demoted but still
    // picked as last resort). Real's own error block covers these (its
    // `get_cycles` runs over the `medium_soft` rung, which keeps
    // unsatisfied runtime edges), so report the persisting ring in the
    // same `circular_deps` shape instead of settling `Complete` over an
    // unlinearizable graph. Gated to otherwise-clean passes (an
    // unsatisfiable dep, slot conflict or orphan blocker is what real
    // reports first -- same precedence as `collect_feedback`'s own
    // trigger), to an empty hard report (no double render), and to
    // passes that could not have retried past this ring: with
    // backtracking on, a clean pass carrying a trigger cycle always
    // retries out of `collect_feedback`, so a settled one either broke
    // the ring (nothing to report) or is unsolved (recorded map).
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
            // Backlog #278 leaves this leg on the old lowest-index
            // rotation on purpose: it only fires when the hard reporter
            // above is empty (persisting soft-containing rings real
            // prints from its own fuller-graph `shortest_cycle[0]`),
            // and rotating those to entries order would move pins (e.g.
            // `sbrA`) real grounds elsewhere. It also dedupes the
            // one-cycle-per-hard-edge repeats into a single ring.
            let min_pos = cpvs
                .iter()
                .enumerate()
                .min_by_key(|&(p, _)| cycle[p])
                .map(|(p, _)| p)
                .unwrap_or(0);
            cpvs.rotate_left(min_pos);
            if !circular_deps.contains(&cpvs) {
                circular_deps.push(cpvs);
            }
        }
    }

    // Elementary-cycle enumeration + reduced display order for the
    // `large_cycle_count` trailer and the cycle-only re-display
    // (real `circular_dependency_handler`, fed by `get_cycles` over
    // the `medium_soft` rung). Built on demand: only a reported hard
    // cycle consumes either number, so acyclic resolves pay nothing.
    let (large_cycle_count, cycle_display, cycle_display_idx) = if circular_deps.is_empty() {
        (false, Vec::new(), Vec::new())
    } else {
        let (cycles, display) = merge_order::cycle_report(
            &pass.entries,
            ctx.atoms,
            ctx.root,
            &params.circular_dependency,
        );
        let display_idx: Vec<usize> = display
            .into_iter()
            .filter(|&i| merge_bound_cpv(&pass.entries[i]).is_some())
            .collect();
        let display = display_idx
            .iter()
            .filter_map(|&i| merge_bound_cpv(&pass.entries[i]))
            .collect();
        (cycles.len() > 3, display, display_idx)
    };

    // Backtracking slice: the `--autounmask-use` changes recorded
    // during the already-resolved-slot re-check (see
    // `autounmask_use_change_records`) -- the graph has settled with
    // those flips applied, so surface them in the same "necessary to
    // proceed" block as the fresh-path ones. Deduped by `(atom,
    // token)`.
    for rec in &params.autounmask_use_change_records {
        if !pass
            .autounmask_use_changes
            .iter()
            .any(|c| c.atom == rec.atom && c.token == rec.token)
        {
            pass.autounmask_use_changes.push(rec.clone());
        }
    }

    // Real `_display_autounmask` emits **one line per package** --
    // it iterates `_needed_use_config_changes.items()` (one entry per
    // `pkg`) and joins every flipped flag of that pkg into a single
    // `>=<cpv> flag -flag …` line, in the order the flips were
    // discovered. Portuale accumulates one `AutounmaskChange` per
    // flip site, so `>=dev-qt/qtbase-6.11.1` needing both `cups` and
    // `vulkan` (each from a different consumer) landed on two lines.
    // Coalesce by `atom`, first-seen order, joining tokens and
    // dropping a flag already present.
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

    // Real `--autounmask-backtrack` off (the default): the backward-
    // cascade re-check folded a `package.use` change into
    // `autounmask_use_config` but the driver did NOT re-drive the walk,
    // so the affected package's entry still shows its pre-flip USE. Real
    // portage's `_pkg_use_enabled` consults `_needed_use_config_changes`
    // for the display regardless -- so re-render just that USE line here
    // (its `flag?`-gated deps still, correctly, do not appear).
    if !ctx.autounmask_backtrack_enabled && !params.autounmask_use_config.is_empty() {
        let tier = autounmask_use_tier(&params.autounmask_use_config);
        let mut tier_config = ctx.config.clone();
        tier_config.autounmask_use = tier;
        for cp in params.autounmask_use_config.keys() {
            refresh_entry_use_display(&mut pass.entries, &ctx.repos, ctx.root, cp, &tier_config);
        }
    }

    // Residual installed-instance slot conflicts for reverse-dep pins
    // dropped as jointly unsatisfiable with the hard requirements
    // (see the feed above and `build_residual_slot_conflicts`): the
    // merge proceeds with the hard-required version, and the broken
    // installed consumers are reported against the installed
    // instance -- real's nomerge-node collision report. Appended
    // after every merge-vs-merge record, so those keep their
    // long-standing order.
    //
    // #57: fed through `record_slot_conflict`, not appended blind. The
    // walker's own installed-instance check can now produce a record for
    // the very same `(package, merge version, installed version)` triple
    // (the keeper-reachable triangle: othermod's `<2.0` against the
    // merged 2.0, *and* keeper's dropped pin against the same pair).
    // Real renders one block whose installed side lists both parents,
    // which is exactly the residual record -- it carries the dropped
    // consumer pins on top of the same hard pullers -- so letting it
    // replace the walker's record keeps the union and the single block.
    for sc in build_residual_slot_conflicts(
        &ctx.repos,
        config,
        ctx.root,
        &pass.entries,
        &pass.slot_pullers,
        &params.dropped_pins,
    ) {
        record_slot_conflict(&mut pass.slot_conflicts, sc);
    }

    // Backlog #90 (S2) + #92: enforced holdable pins that withhold an
    // upgrade join the direct solve's removal rows in the same skip
    // block (same shape, same rc 0; the renderer cannot tell the
    // producers apart). Exact-dedupe against the removal rows covers
    // the pathological both-paths shape.
    //
    // Backlog #209: under `--backtrack=0` a slot-operator withhold
    // stays silent. Real never reports that path: the minimizer/probe
    // selection writes no `_runtime_pkg_mask` and no
    // `_conflict_missed_update`, so `_show_missed_update`
    // (`_emerge/depgraph.py:1566`) has nothing to render (the r25 shape
    // merges silently at `backtrack: 0/20`, both bound forms of the
    // consumer's pin). A plain-pin withhold still synthesizes its rows:
    // real reaches it through the slot-conflict solver, whose "Record
    // missed updates" tail (`:2089-2110`) runs with no
    // `_allow_backtracking` gate, so the notice prints even with
    // backtracking off (the #90 S2 two-target cell pins it).
    // `raw_atom` is the verbatim recorded text: the live `:=` form and
    // the recorded `:S/SS=` form both carry a slot operator there,
    // while `atom` is normalised (the built form strips to a bare
    // version pin), so the raw text is what classifies the path.
    let bt0_plain_pins: Vec<RevDepPin> = if ctx.backtrack_max == 0 {
        params
            .reverse_dep_pins
            .iter()
            .filter(|pin| {
                !portage_dep::parse_atom(&pin.raw_atom).is_some_and(|a| a.slot_operator.is_some())
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
    // The withhold rows keep their established deterministic order
    // (alphabetical by package, then atom and consumer); the direct
    // solve's rows ahead of them keep real's first-seen block order.
    let mut withheld =
        constraint_withheld_updates(&ctx.repos, ctx.root, config, &pass.entries, enforced_pins);
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
    // #270: first-seen order, like real's `_conflict_missed_update` dict
    // (insertion order of the solver's removals, `depgraph.py:1529-1565`),
    // not alphabetical -- exact duplicates collapse to their first row.
    let mut unique: Vec<SkippedUpdate> = Vec::with_capacity(pass.skipped_updates.len());
    for row in std::mem::take(&mut pass.skipped_updates) {
        if !unique.contains(&row) {
            unique.push(row);
        }
    }
    pass.skipped_updates = unique;

    // Masked-dependency chains are walked out of the final entries
    // (`required_by` is only complete post-pass); the atom+masked
    // data was recorded at each `NoVisibleCandidate` push above.
    for rep in &mut pass.masked_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            root: ctx.root,
            atom: &atom,
        });
    }
    // Same walk for the `[use]`-unsatisfied disclosures (#20): the chain
    // shape is real `_get_dep_chain`'s, shared with the masked block.
    for rep in &mut pass.use_unsat_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            root: ctx.root,
            atom: &atom,
        });
    }
    // #135 (d) post-pass: every dependency `NoVisibleCandidate` entry
    // that neither sibling block claimed gets a plain-miss disclosure.
    // Placed here, after the walk, so it also catches NVC entries
    // created outside `run_pass`'s own NVC push (the binary-dep path
    // creates them at the merge-entry push, where the disclosure block
    // does not run) -- the reason `--usepkgonly`'s `newpkg` miss used
    // to fall through to the bare line with `plain_miss_deps` empty.
    let mut extra_plain_miss: Vec<PlainMissDepReport> = Vec::new();
    for e in &pass.entries {
        if !matches!(e.outcome, PretendOutcome::NoVisibleCandidate) {
            continue;
        }
        // Backlog #195 (round 2): a dependency collected into a parent
        // USE flip (record-and-fail) is not a miss at all -- real drops
        // the collected item from `_unsatisfied_deps_for_display`, so
        // the sweep must not re-add it here (same gate as the walk-time
        // disclosure block and the abort renderer). Invariant: the
        // `parent_flip_recorded` latch is only ever inserted on the
        // record-and-fail path, after `suggested_parent_use_candidate`
        // returned `Some` -- so every latched key has
        // `parent_use_suggestion.is_some()`, and the latch-keyed
        // walk-time gate suppresses exactly the entries these
        // `is_some()` gates suppress. One corner breaks the converse:
        // when the re-probe fails after the suggestion returned `Some`
        // (the `NoVisibleCandidate` break in the `'parent_flip` arm),
        // the entry keeps `Some` with no latch -- walk-time records a
        // miss block for it, but the sweep and the abort renderer still
        // skip it. Benign (no corpus cell hits it) and deliberate: the
        // flip genuinely did not rescue the dep.
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
        });
    }
    pass.plain_miss_deps.extend(extra_plain_miss);
    // #135 (e): the `#required by …` chain is walked out of the final
    // entries (post `required_by` fill) exactly like the disclosure
    // chains above -- the walk-time call sites record `Vec::new()`.
    // Backlog #248: same no-start-node rule as the USE fill (a change
    // on an argument package yields just the `(argument)` line).
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
    // Same walk for the plain-miss disclosures (#135 (d)).
    for rep in &mut pass.plain_miss_deps {
        let atom = rep.atom.clone();
        rep.chain = masked_dep_chain(MaskedDepChainArgs {
            repos: &ctx.repos,
            config: ctx.config,
            entries: &pass.entries,
            category: &rep.category,
            package: &rep.package,
            atoms: ctx.atoms,
            root: ctx.root,
            atom: &atom,
        });
    }

    // Backlog #19 Slice 3: classify the settled graph the way real's
    // abandon sites would have (see `abort_outcome`). Backlog #193:
    // the virtual-cycle probe runs on the same settled graph (real's
    // `_virt_deps_visible` recursion over the selected providers);
    // like `find_hard_cycles` it is unconditional -- its own cheap
    // gate (no merge-bound virtual, no probe) keeps ordinary resolves
    // free.
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

    // Backlog #129 (S1): real `display_problems` shows missed updates
    // only with no unresolved conflicts (`depgraph.py:11127-11132` --
    // slot collisions or unsatisfied blockers hide them as possibly
    // irrelevant): the settled backtrack masks become skip records only
    // on a clean `Complete` graph with no surviving slot conflicts, no
    // unsolvable blocker rows and no orphan rows. A mask-free search
    // (including every `--backtrack=0` run) derives nothing here.
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
                repos: &ctx.repos,
                root: ctx.root,
                config,
                entries: &pass.entries,
                params,
                missing_args: &pass.missing_args,
            });
        // Real chains the mask dict before the handler removals
        // (`:1533-1536`), so mask rows lead in first-seen order and the
        // direct-solve rows (first-seen block order) and the sorted
        // withhold rows keep their established order behind them. No
        // re-sort: alphabetical would scramble the mask insertion order
        // real renders in. Both sources
        // collapse per `(root, slot_atom)` keeping the highest missed
        // pkg (`:1553-1562`) -- see `collapse_skipped_updates`.
        //
        // #292: a slot the pass already holds (portuale restarted over a
        // conflict real settles in its in-pass solver, so the same record
        // sits in both sources) is real's single `_conflict_missed_update`
        // entry, at the solver's position -- not a leading
        // `_runtime_pkg_mask` one. Move such a slot's rows to where its
        // first pass row sits (rows inside the slot keep their order);
        // slots only the mask knows keep leading.
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
    }

    GraphResult {
        entries: pass.entries,
        backtrack_restarts,
        backtrack_max: ctx.backtrack_max,
        outcome,
        slot_conflicts: pass.slot_conflicts,
        skipped_updates: pass.skipped_updates,
        skipped_missing_deps,
        skipped_missing_dep_full,
        orphan_blockers: pass.orphan_blockers,
        changed_deps_report: pass.changed_deps_report_entries,
        buildpkgonly_deps_unsatisfied,
        prune_unresolved: false,
        pprovided_atoms: pass.pprovided_atoms,
        missing_args: pass.missing_args,
        autounmask_keyword_changes: pass.autounmask_keyword_changes,
        autounmask_use_changes: pass.autounmask_use_changes,
        autounmask_license_changes: pass.autounmask_license_changes,
        autounmask_mask_changes: pass.autounmask_mask_changes,
        // Backlog #217: the settled accumulator still holds a flip
        // folded for an already-graphed package (the Settle/Feedback
        // params carry the pass's overlay union -- see
        // `collect_feedback`'s `grown`), i.e. real's final pass grew
        // `_needed_use_config_changes` for a digraph node and wanted a
        // restart instead of reaching the `_success_without_autounmask`
        // tail -- or the pass rescued a failed dep via a parent USE
        // flip (real `_apply_parent_use_changes` collecting
        // post-failure). Either way the tail was pre-empted.
        autounmask_no_clean_tail: !params.autounmask_use_config.is_empty()
            || pass.parent_flip_rescued,
        settled_with_backtracking: false,
        abi_rebuilds,
        circular_deps,
        circular_dependency: params.circular_dependency.clone(),
        masked_deps: pass.masked_deps,
        use_unsat_deps: pass.use_unsat_deps,
        plain_miss_deps: pass.plain_miss_deps,
        large_cycle_count,
        cycle_display,
        virtual_cycle,
    }
}
