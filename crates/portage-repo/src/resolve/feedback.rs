//! Turning a pass's problems into feedback for the next pass.

use crate::*;

/// Phase C1 (023): why one `cat/pkg` version is masked on retry,
/// mirroring real `backtracking.py`'s `runtime_pkg_mask[pkg][reason]`
/// dict shape (one entry per masked version, reason-annotated). C1 only
/// splits the storage -- readers still see the union (see
/// `BacktrackParams::union_constraints`); C2's `Backtracker` consumes
/// the reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskReason {
    /// Real `mask_info["slot conflict"]`: the masked version collided in
    /// its slot. `parents` are the pulling `((cat, pkg, ver), atom)`
    /// triples whose atom does NOT match the masked version (real's
    /// `conflict_atoms`, used by the bug-375573 discard check, which
    /// tests versioned parent packages -- hence the stored `ver`).
    /// Parent downgrades are `MissingDependency` masks on a later node,
    /// never empty-parent slot masks (C3 removed puller pre-masking).
    SlotConflict {
        parents: Vec<((String, String, String), String)>,
    },
    /// Real `mask_info["missing dependency"]`: the masked parent's
    /// subtree contained an unsatisfiable dep; `atom` is that dep's text
    /// (real also records the root; portuale resolves a single root).
    MissingDependency {
        parent: (String, String),
        atom: String,
    },
    /// Real `mask_info["slot_operator_mask_built"]` (backlog #212,
    /// v2 `#24c`): a non-installed binary whose built slot-operator dep
    /// cannot be satisfied the way it was built, recorded by
    /// `_slot_change_backtrack` (`depgraph.py:2383`),
    /// `_slot_operator_update_backtrack` (`:2427-2431`) and
    /// `_slot_operator_unsatisfied_backtrack` (`:2901-2903`) and
    /// enforced through the backtrack-config restart (`:5703-5712`,
    /// `resolver/backtracking.py::_feedback_config:237-242`, which
    /// files the mask under *both* `slot_operator_mask_built` *and*
    /// `runtime_pkg_mask`). Real keys by package object, so the mask
    /// hides only the binary instance -- the same-version ebuild stays
    /// visible. Portuale's string readers match binary and ebuild at
    /// one `cpv:slot::repo` key, so the rendered `neg` carries a
    /// `[binary]` marker (`!=cat/pkg-ver[binary]`): every
    /// string-constraint reader skips marked entries (see
    /// [`is_binary_only_mask`] -- `match_from_list` ignores USE-deps,
    /// so the marker would otherwise match the ebuild too), and only
    /// the binary-candidate pool filter honours them (via
    /// [`slot_operator_binary_masked_version`]). Only the slot-change
    /// arm is ported
    /// (see [`slot_operator_slot_change_probe`]); the update-probe and
    /// unsatisfied-probe arms have no portuale-visible diverging shape
    /// (both steer a parent the graph already replaces or fails on, so
    /// same-version binary-vs-ebuild steering is vacuous there).
    SlotOperatorBuilt,
}

/// One `!=cat/pkg-ver` negative with its reason. `neg` renders exactly
/// as the string used to live in `slot_constraints`, so every reader
/// that matches on it behaves identically. The masked version is
/// recoverable without parsing: strip the `!=cat/pkg-` prefix formed
/// from the bucket key (C2 does this when it needs per-cpv granularity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MaskEntry {
    pub(crate) neg: String,
    pub(crate) reason: MaskReason,
}

/// File one `_slot_operator_unsatisfied_probe` result into the working
/// backtrack accumulators: replace-set seeds grow
/// `slot_operator_replace_installed` (real
/// `backtrack_infos["config"]["slot_operator_replace_installed"]`),
/// binary parents become `[binary]`-marked `SlotOperatorBuilt`
/// negatives (real `:2896-2903`). Returns true when anything grew --
/// the caller restarts with the same budget-free Config feedback the
/// #211 scan returns. Single filing point for both probe sites (the
/// missing-dep trigger arm, which real runs first per failing edge, and
/// the no-trigger dead-end fallback); the replace set therefore
/// expands in exactly one place here, and neither arm keeps restart
/// bookkeeping of its own. Under `--usepkgonly` the binary result is
/// discarded (real's `_iter_similar_available` yields binaries only,
/// so no ebuild replacement exists -- see the trigger site).
pub(crate) fn file_unsat_probe_results(
    grown: &mut BacktrackParams,
    fresh: BTreeSet<(String, String)>,
    binary_masks: BTreeSet<(String, String, String)>,
    usepkgonly: bool,
) -> bool {
    let mut masks_new = false;
    // `--usepkgonly`: no ebuild replacement (see the trigger site).
    let binary_masks = if usepkgonly {
        BTreeSet::new()
    } else {
        binary_masks
    };
    for (cat, pkg, ver) in &binary_masks {
        let neg = format!("!={cat}/{pkg}-{ver}[binary]");
        let bucket = grown
            .runtime_pkg_mask
            .entry((cat.clone(), pkg.clone()))
            .or_default();
        if !bucket.iter().any(|m| m.neg == neg) {
            bucket.push(MaskEntry {
                neg,
                reason: MaskReason::SlotOperatorBuilt,
            });
            masks_new = true;
        }
    }
    if masks_new
        || fresh
            .iter()
            .any(|cp| !grown.slot_operator_replace_installed.contains(cp))
    {
        grown.slot_operator_replace_installed.extend(fresh);
        return true;
    }
    false
}

/// One pass's outcome in real `_backtrack_depgraph` terms -- see the
/// type docs above. The overlay merge runs first (steps read the merged
/// accumulators, as they always did); each arm then packages its effect
/// without touching the search state.
pub(crate) fn collect_feedback(
    ctx: &ResolveCtx<'_>,
    params: &BacktrackParams,
    pass: &mut PassResult,
    config: &portage_profile::Config,
) -> PassDecision {
    // C2: the A3 overlay union lands in a working copy, not the live
    // search state -- the driver applies the packaged effect through
    // `Backtracker::feedback`.
    // A3: fold this pass's overlay into the cross-pass accumulators
    // before deciding. The old code wrote `bp` directly during the walk,
    // so at this point `bp` already held everything; the union below
    // reproduces that state exactly (idempotent per-flag insert with the
    // same `bucket.get(f) != Some(want)` rule, `(atom, token)`-deduped
    // record append, `|=` latch for the contradiction). Placed before the
    // chain rather than in step 5: an earlier step (solvable growth, mask
    // trial, breakage) may fire on the same pass that folded flips, and
    // the retry must see them -- as it always did.
    let mut grown = params.clone();
    for (cp, flags) in &pass.use_overlay {
        let bucket = grown.autounmask_use_config.entry(cp.clone()).or_default();
        for (f, want) in flags {
            if bucket.get(f) != Some(want) {
                bucket.insert(f.clone(), *want);
            }
        }
    }
    for rec in &pass.use_change_overlay {
        if !grown
            .autounmask_use_change_records
            .iter()
            .any(|c| c.atom == rec.atom && c.token == rec.token)
        {
            grown.autounmask_use_change_records.push(rec.clone());
        }
    }
    grown.autounmask_use_broke |= pass.use_broke;

    // #57 S1: the *dropped* half of the reverse-dependency scan (#54,
    // documented at its enforcement site further down) is not feedback
    // at all -- it is the input `assemble_result` renders the residual
    // installed-instance block from. A pass that ends in slot-conflict
    // mask feedback returns long before the scan's own site, so those
    // pins never reached `grown` and a later `get_best_run` re-pass
    // reported the collision without its installed consumers (the
    // keeper-reachable triangle lost keeper's `=dev-libs/paired-1.0`
    // parent line the moment the walk started recording that collision
    // itself). Collected here for exactly that case -- a pass that found
    // a slot conflict -- so the ordinary settle path pays for one scan,
    // as it always did, not two.
    if !pass.slot_conflicts.is_empty() {
        let (_, dropped) = reverse_dependency_constraints(
            &ctx.repos,
            ctx.root,
            &pass.entries,
            ctx.with_bdeps,
            ctx.excluded,
            &pass.slot_want,
            &ctx.slot_op_reachable,
            ctx.dynamic_deps,
            ctx.ignore_built_slot_operator_deps,
            &ctx.top_level_cps,
            &ctx.world_reachable,
            ctx.config,
            ctx.complete,
        );
        for pin in dropped {
            if !grown.dropped_pins.contains(&pin) {
                grown.dropped_pins.push(pin);
            }
        }
    }

    // Backlog #216: real `_serialize_tasks`' circular dead-end
    // (`depgraph.py:10262-10289`). The walk finished but the merge-bound
    // graph does not serialize: a hard build-time cycle -- now including
    // a true self-loop (see `find_hard_cycles`) -- leaves nodes
    // unselectable. Real records every (node → its predecessor in the
    // cycle) edge into the `circular_dependency` backtrack map and
    // retries, and the next pass's `dep_zapdeps` demotes the matching
    // `||` branch to `other` (see `disjunction_preference`'s
    // `circular_dependency` argument). A cycle that adds nothing new is
    // unsolved (real's `unsolved_cycle` → `_skip_restart`): a dead end,
    // reported from the best run, not retried.
    //
    // Scope (`cycle_restartable`): only the fresh self-pick shape
    // retries HERE. Every other hard cycle falls through to the later
    // arms exactly as before (a walk-time NVC still beats it below;
    // multi-node rings reach backlog #221's general restart arm, which
    // shares this map), so their reports are byte-identical with and
    // without this slice.
    //
    // Placed before the slot-conflict arms: a deadlocked graph pre-empts
    // conflict repair (real never reaches conflict processing with a
    // failed serialize); a re-pass whose demotion clears the cycle
    // re-runs those arms normally.
    let hard_cycles = find_hard_cycles(
        &pass.entries,
        &pass.edge_kind_map,
        ctx.root,
        &grown.circular_dependency,
        // Pre-sort pass: only the single-node `cycle_restartable` gate
        // reads the result, so the insertion rank (a post-sort notion)
        // is meaningless here; `None` falls back to `discovery` order.
        None,
    );
    if hard_cycles.len() == 1
        && cycle_restartable(
            &pass.entries,
            &kept_alt_branches(&pass.entries, ctx.root, &grown.circular_dependency),
            &hard_cycles[0],
            ctx.root,
        )
    {
        let mut new_info = false;
        for cycle in &hard_cycles {
            for (index, node_cpv) in cycle.iter().enumerate() {
                let Some((node_cat, node_pkg, _)) = split_cpv(node_cpv) else {
                    continue;
                };
                // Track X Slice C (#242): the cycle cpv may name two
                // merge-bound instances (one per root) -- real records
                // the node that actually dead-ends
                // (`circular_dependency.setdefault(node, ...)` keyed by
                // package object). The true cycler is the instance with
                // the hard self edge in the root-qualified walk map;
                // the other instance's self pick resolves to it, not to
                // itself. No hard self pair (the slot-op-only shapes)
                // keeps the legacy target-root key.
                let cycler_running = pass
                    .edge_kind_map
                    .get(&(
                        (true, node_cat.clone(), node_pkg.clone()),
                        (true, node_cat.clone(), node_pkg.clone()),
                    ))
                    .is_some_and(|(has_hard, _)| *has_hard);
                // Real's own predecessor formula: `cycle[-1]` for index
                // 0, `cycle[index-1]` otherwise.
                let child_cpv = if index == 0 {
                    &cycle[cycle.len() - 1]
                } else {
                    &cycle[index - 1]
                };
                let Some((child_cat, child_pkg, child_ver)) = split_cpv(child_cpv) else {
                    continue;
                };
                // An empty version builds a `cat/pkg-` candidate that no
                // atom ever matches (M1): skip the record instead of
                // recording a dead edge that would still count as
                // `new_info` and force a futile retry.
                if child_ver.is_empty() {
                    continue;
                }
                // Slot fidelity like #221's ring records below: the
                // merge-bound entry behind the child cpv carries the
                // slot/sub-slot an atom's `:slot` pin matches against.
                let (child_slot, child_sub_slot) = pass
                    .entries
                    .iter()
                    .find(|e| merge_bound_cpv(e).as_deref() == Some(child_cpv.as_str()))
                    .map(|e| (e.slot.clone(), e.sub_slot.clone()))
                    .unwrap_or((None, None));
                let record = CircularDepChild {
                    category: child_cat,
                    package: child_pkg,
                    version: child_ver,
                    slot: child_slot,
                    sub_slot: child_sub_slot,
                };
                let bucket = grown
                    .circular_dependency
                    .entry((cycler_running, node_cat, node_pkg))
                    .or_default();
                if !bucket.contains(&record) {
                    bucket.push(record);
                    new_info = true;
                }
            }
        }
        if new_info {
            return PassDecision::Feedback(BacktrackFeedback::Circular {
                params: Box::new(grown),
            });
        }
        return PassDecision::DeadEnd {
            params: Box::new(grown),
        };
    }

    // Backlog #214 (v2 `#24e`): real `_slot_conflict_backtrack_abi`
    // (`lib/_emerge/depgraph.py:2282-2315`) runs inside
    // `_process_slot_conflict`, BEFORE the masking backtrack below --
    // a built parent whose soname/`:=` conflict atom the update probe
    // can resolve is rebuilt instead of reported. Same order here: the
    // probe below fires before the solvability/mask arms, and its
    // restart carries no masks (real's `remaining` skip). Gated on a
    // live search like the scan's own call site (`backtrack_max > 0`
    // is real's `_allow_backtracking`; `ignore_built_slot_operator_deps`
    // drops the built atoms the probe reads; no unsatisfiable
    // dependency outstanding, like the `has_nvc` dead-end arm below --
    // real never reaches conflict processing with a failed
    // `_create_graph`).
    // Deliberately NOT gated on `rebuild_if_new_slot` (real's conflict
    // path never is -- the scan's call-site gate is the pre-existing
    // narrowing for its own arms, not this one).
    if !pass.slot_conflicts.is_empty()
        && ctx.backtrack_max > 0
        && !ctx.ignore_built_slot_operator_deps
        && !pass.suppressed_nvc
        && !pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate))
    {
        // Same inputs the scan builds for its own refusal map: every
        // cp this pass walked, the merge-bound set, and the R2 parents.
        let walked: HashSet<(String, String)> = pass
            .entries
            .iter()
            .map(|e| (e.category.clone(), e.package.clone()))
            .collect();
        let mut in_graph: HashSet<(String, String)> = HashSet::new();
        for e in &pass.entries {
            if !matches!(
                e.outcome,
                PretendOutcome::AlreadyInstalled { .. }
                    | PretendOutcome::NoVisibleCandidate
                    | PretendOutcome::Uninstall { .. }
            ) {
                in_graph.insert((e.category.clone(), e.package.clone()));
            }
        }
        let probe_parents = collect_probe_parents(
            ctx.root,
            &pass.entries,
            &ctx.slot_op_reachable,
            &walked,
            &in_graph,
            &grown.slot_operator_replace_installed,
            ctx.with_bdeps,
            ctx.excluded,
        );
        let fresh = slot_conflict_abi_probe(
            ctx.root,
            &ctx.repos,
            &pass.entries,
            &pass.slot_conflicts,
            &grown.slot_operator_replace_installed,
            &grown.slot_operator_undone,
            ctx.with_bdeps,
            &walked,
            ctx.excluded,
            &probe_parents,
            config,
        );
        if !fresh.is_empty() {
            grown.slot_operator_replace_installed.extend(fresh);
            return PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(grown),
            });
        }
    }

    // Backtracking (real `backtracking.py` retry loop driven by
    // `_process_slot_conflicts`): if this attempt left any slot conflicts,
    // check each one for solvability -- is there a single version of the
    // conflicted `cat/pkg` that satisfies *every* atom text that targeted
    // it this pass? If so, the whole atom set becomes a config-feedback
    // node for the conflicted package. Unsolvable conflicts fall through
    // to the mask node below; anything still conflicting when the search
    // is exhausted is reported from the best run, exactly as before.
    if !pass.slot_conflicts.is_empty() {
        let mut progressed = false;
        for sc in &pass.slot_conflicts {
            let pkg_key = (sc.category.clone(), sc.package.clone());
            let wants = match pass.slot_want.get(&pkg_key) {
                Some(w) if w.len() >= 2 => w.clone(),
                _ => continue,
            };
            // Solvability pre-check against the raw candidate list (real
            // `_select_pkg_highest_available` over the full atom set). This
            // deliberately ignores keyword/mask visibility -- the rare case
            // where the one satisfying version is itself masked resolves to a
            // plain `NoVisibleCandidate` on the retry, which is
            // acceptable and documented for this slice. It does NOT
            // ignore USE-deps (real enforces them in selection): a
            // version only counts when every want-atom matches it on
            // version, slot, *and* USE, so a USE-mismatching slot reuse
            // falls through to the mask trial and the notice instead of
            // constraining a retry that can only end in swallowed
            // "no visible ebuild" lines.
            let candidates =
                list_candidates(&ctx.repos, &pkg_key.0, &pkg_key.1).unwrap_or_default();
            let solvable = candidates.iter().any(|c| {
                let cs = format!("{}/{}-{}:{}", pkg_key.0, pkg_key.1, c.version, c.slot);
                let (iuse, use_flags) =
                    slot_conflict_flag_sets(&ctx.repos, config, &pkg_key.0, &pkg_key.1, &c.version);
                wants.iter().all(|w| {
                    portage_dep::match_from_list(w, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                        && portage_dep::parse_atom(w).is_none_or(|a| match &a.use_deps {
                            None => true,
                            Some(ud) => portage_dep::use_deps_satisfied(ud, &iuse, &use_flags),
                        })
                })
            });
            if !solvable {
                continue;
            }
            let slot = grown.slot_constraints.entry(pkg_key).or_default();
            for w in wants {
                if !slot.contains(&w) {
                    slot.push(w);
                    progressed = true;
                }
            }
        }
        if progressed {
            return PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(grown),
            });
        }
    }

    // C3: real `_slot_confict_backtrack` (one node per ranked choice,
    // first conflicting slot only -- real `_feedback_slot_conflicts`
    // takes `conflicts_data[0]`). The old bundled trial (highest child +
    // every lower-alternative puller) is gone: puller-masking now happens
    // only through a later pass's own missing-dep feedback, exactly like
    // real, where a downgraded parent is abandoned via its unsatisfiable
    // atom rather than pre-masked.
    if !pass.slot_conflicts.is_empty() {
        let choices = slot_conflict_mask_choices(
            &ctx.repos,
            ctx.excluded,
            &grown,
            &pass.slot_pullers,
            &pass.slot_conflicts[0],
            config,
        );
        // Novelty mirrors the old trial-dedup, compared on the full
        // entry (neg + reason, the same granularity the node-dedup
        // uses): at least one group member new to the search state. A
        // repeat with recomputed parents is a distinct state and
        // explores -- the mask budget still caps it.
        let added = choices.iter().any(|c| {
            c.similar
                .iter()
                .map(|s| (&s.target, &s.parents))
                .chain(std::iter::once((&c.target, &c.parents)))
                .any(|((cat, pkg, ver), parents)| {
                    let entry = MaskEntry {
                        neg: format!("!={cat}/{pkg}-{ver}"),
                        reason: MaskReason::SlotConflict {
                            parents: parents.clone(),
                        },
                    };
                    !grown
                        .runtime_pkg_mask
                        .get(&(cat.clone(), pkg.clone()))
                        .is_some_and(|bucket| bucket.contains(&entry))
                })
        });
        if added {
            // #297: real's `_process_slot_conflicts` also runs
            // `_slot_operator_trigger_reinstalls` in the same pass, so
            // the replace set rides the conflict's node (`infos` carries
            // `config` and `slot conflict` together) instead of costing a
            // separate restart once the masks settle.
            let nvc = pass.suppressed_nvc
                || pass
                    .entries
                    .iter()
                    .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));
            if !nvc
                && ctx.backtrack_max > 0
                && !ctx.ignore_built_slot_operator_deps
                && ctx.rebuild_if_new_slot
            {
                let top_atoms: HashSet<String> =
                    ctx.top_level.iter().map(|s| (*s).to_string()).collect();
                let (mut scheduled, _abi, _masked) = slot_operator_rebuild_scan(
                    ctx.root,
                    &ctx.repos,
                    &pass.entries,
                    &ctx.slot_op_reachable,
                    &grown.slot_operator_replace_installed,
                    &grown.slot_operator_undone,
                    ctx.with_bdeps,
                    ctx.update,
                    &ctx.top_level_cps,
                    &top_atoms,
                    ctx.excluded,
                    config,
                );
                let (force_consumers, forced) = slot_operator_update_force_scan(
                    ctx.root,
                    &ctx.repos,
                    config,
                    &pass.entries,
                    &ctx.slot_op_reachable,
                    &scheduled,
                    &grown.slot_operator_undone,
                    ctx.with_bdeps,
                    ctx.update,
                    &ctx.top_level_cps,
                    ctx.excluded,
                );
                scheduled.extend(force_consumers);
                grown.slot_operator_forced_upgrade.extend(forced);
                grown.slot_operator_replace_installed = scheduled;
            }
            return PassDecision::Feedback(BacktrackFeedback::SlotConflict {
                base: Box::new(grown),
                choices,
            });
        }
    }

    // Real `_autounmask_breakage` (depgraph.py:12262-12280): an
    // accumulated autounmask USE change made another use-dep
    // unsatisfiable and the two can't be reconciled (a flag wanted
    // both ways). Drop every autounmask change, turn suggestion fully
    // off, and re-resolve one final clean pass -- exactly real's
    // `myparams["autounmask"] = False` retry. The `autounmask_disabled`
    // latch keeps it to a single extra pass.
    if grown.autounmask_use_broke && !grown.autounmask_disabled {
        grown.autounmask_disabled = true;
        grown.autounmask_suggest_keywords = false;
        grown.autounmask_suggest_use = false;
        grown.autounmask_suggest_license = false;
        grown.autounmask_suggest_masks = false;
        grown.autounmask_use_config.clear();
        grown.autounmask_use_change_records.clear();
        grown.backtrack_config = None;
        return PassDecision::Feedback(BacktrackFeedback::Config {
            params: Box::new(grown),
        });
    }

    // Backtracking slice: this pass folded a new `--autounmask-use`
    // flip into `autounmask_use_config` (real `_feedback_config`
    // growing `_needed_use_config_changes`). Re-run the whole walk
    // with the grown config so the flipped package is re-resolved
    // with the flag on and its `flag?`-gated deps appear. Converges
    // because `autounmask_grew` is only set on a *newly*-added
    // `(cp, flag)` entry. Config feedback is budget-free (real's
    // `_feedback_config` doesn't count toward `--backtrack=N`).
    if pass.autounmask_grew {
        let mut c = ctx.config.clone();
        c.autounmask_use = autounmask_use_tier(&grown.autounmask_use_config);
        grown.backtrack_config = Some(c);
        return PassDecision::Feedback(BacktrackFeedback::Config {
            params: Box::new(grown),
        });
    }

    // Real `backtracking.py::_feedback_missing_dep`: this pass hit a
    // dependency with no matching package; mask its parent (`!=cpv` in
    // `runtime_pkg_mask`, latched in `missing_dep_masked` so the
    // same parent is never re-masked) and re-run. Combined with the
    // `||` closure honouring `slot_constraints`, a `|| ( a b )` whose
    // `a` has an unsatisfiable subtree now yields to `b`. A trigger the
    // latch already covers, or a top-level/USE-only mismatch with no
    // trigger at all, falls through below (dead end unless other
    // feedback fired first).
    if let Some(((pc, pp), neg, dep_atom)) = pass.missing_dep_trigger.take() {
        // #257 + #276: real tries the whole
        // `_slot_operator_unsatisfied_probe` before the
        // missing-dependency mask for a built slot-operator atom
        // (`_emerge/depgraph.py:3447-3455` returns ahead of the
        // `runtime_pkg_mask` fallback at `:3456-3470`): a hit seeds the
        // replace set (an installed parent with a resolving
        // replacement) or masks the binary (a non-installed built
        // parent, `slot_operator_mask_built`), and the restart takes
        // the replacement; only a miss falls through to the mask
        // below. Filed through the shared helper (one expansion point,
        // no new restart bookkeeping). Under `--usepkgonly` real's
        // `_iter_similar_available` yields binaries only, so no ebuild
        // replacement exists and the probe is skipped (the
        // missing-dependency path below stands).
        if ctx.backtrack_max > 0
            && !ctx.usepkgonly
            && portage_dep::parse_atom(&dep_atom).is_some_and(|a| is_built_slot_op(&a))
        {
            let (fresh, binary_masks) = slot_operator_unsatisfied_probe_full(
                ctx.root,
                &ctx.repos,
                &pass.entries,
                &pass.slot_want,
                &pass.slot_pullers,
                &grown.runtime_pkg_mask,
                ctx.with_bdeps,
                ctx.excluded,
                config,
            );
            if file_unsat_probe_results(&mut grown, fresh, binary_masks, ctx.usepkgonly) {
                return PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(grown),
                });
            }
        }
        // C2: the latch moves into the node with the mask (see
        // `Backtracker::feedback`); the trigger here only packages.
        return PassDecision::Feedback(BacktrackFeedback::MissingDep {
            base: Box::new(grown),
            owner: (pc.clone(), pp.clone()),
            entry: MaskEntry {
                neg,
                reason: MaskReason::MissingDependency {
                    parent: (pc, pp),
                    atom: dep_atom,
                },
            },
        });
    }

    // Real `_complete_graph`'s installed-universe walk reaching
    // `_slot_operator_check_reverse_dependencies`: an upgrade this
    // pass picked breaks a dependency an *installed* package -- one
    // nowhere near the requested atom's own tree -- already records
    // against it. Real sees such an atom because complete mode
    // (auto-enabled by any installed-package version/USE change)
    // deep-walks `@world`/`@selected`/`@system` and pulls every
    // *reachable* installed package into the graph as a nomerge node
    // (#54: not the whole installed universe -- see
    // `reverse_dependency_constraints`'s doc comment), contributing its
    // recorded atoms to `_parent_atoms`;
    // portuale finds them with a direct vdb reverse scan instead and
    // feeds them back here as positive enforcement atoms (C1: they stay
    // in the positives bucket -- they are not masks), which is
    // precisely what parent atoms are. The re-resolve then picks the
    // highest candidate satisfying them -- normally the installed
    // version, so the entry settles as `AlreadyInstalled` -- and
    // drops whatever the rejected version had dragged into the
    // graph. `reverse_dep_masked` latches every constraint already
    // added, so a pass that finds nothing new falls through and the
    // loop terminates.
    //
    // A pin jointly unsatisfiable with this pass's hard requirements
    // (an explicit versioned request, or a dependency only a newer
    // version satisfies) is dropped instead of enforced: real merges
    // the hard-required version anyway and reports the broken
    // consumer against the installed instance. Dropped pins
    // accumulate in `dropped_pins` for that residual report (see
    // `build_residual_slot_conflicts`); they never enter
    // `slot_constraints`, so the hard-required version is never
    // masked into invisibility.
    let mut added = false;
    let (enforced, dropped) = reverse_dependency_constraints(
        &ctx.repos,
        ctx.root,
        &pass.entries,
        ctx.with_bdeps,
        ctx.excluded,
        &pass.slot_want,
        &ctx.slot_op_reachable,
        ctx.dynamic_deps,
        ctx.ignore_built_slot_operator_deps,
        &ctx.top_level_cps,
        &ctx.world_reachable,
        ctx.config,
        ctx.complete,
    );
    // #24 S4: rule 4 of `_eliminate_rebuilds` checks every parent atom of
    // the rebuilt pkg, and real's complete-graph nomerge consumers are
    // part of `_parent_atoms`; these are the pins carrying their atoms.
    // Cloned before the loops consume the vectors.
    let reverse_pins: Vec<RevDepPin> = enforced.iter().chain(dropped.iter()).cloned().collect();
    for pin in enforced {
        // Backlog #90 (S2) + #92: retain every enforced pin (deduped
        // like `dropped_pins`) so `assemble_result` can report the
        // upgrades they withhold -- no conflict ever forms on that
        // path, so without this the withhold would stay silent.
        if !grown.reverse_dep_pins.contains(&pin) {
            grown.reverse_dep_pins.push(pin.clone());
        }
        if grown
            .reverse_dep_masked
            .insert((pin.cp.clone(), pin.atom.clone()))
        {
            // Backlog #205: real's "Record missed updates" tail
            // (`lib/_emerge/depgraph.py:2085-2106`) -- a version the
            // re-resolve drops for the enforced pin is reported
            // through `_show_missed_update_slot_conflicts`. Record the
            // rejection as a slot-conflict-style mask on the chosen
            // version (parents = the enforcing consumer), so the
            // existing `backtrack_missed_updates` derivation renders
            // the `WARNING` row. The positive pin above still drives
            // selection; the negative excludes nothing the pin does
            // not already exclude. Gated on a live search like every
            // mask (real `_allow_backtracking`); budget-free like the
            // RevDep feedback it rides.
            if ctx.backtrack_max > 0 {
                for e in &pass.entries {
                    if e.category != pin.cp.0 || e.package != pin.cp.1 {
                        continue;
                    }
                    let (ver, slot, sub, repo) = match &e.outcome {
                        PretendOutcome::New { version }
                        | PretendOutcome::Reinstall { version, .. } => (
                            version.as_str(),
                            e.slot.as_deref().unwrap_or("0"),
                            e.sub_slot.as_deref().unwrap_or("0"),
                            e.repo_name.as_deref().unwrap_or(""),
                        ),
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } => (
                            to.as_str(),
                            e.slot.as_deref().unwrap_or("0"),
                            e.sub_slot.as_deref().unwrap_or("0"),
                            e.repo_name.as_deref().unwrap_or(""),
                        ),
                        _ => continue,
                    };
                    let candidate = format!("{}/{}-{ver}:{slot}/{sub}::{repo}", pin.cp.0, pin.cp.1);
                    if portage_dep::match_from_list(&pin.atom, &[candidate.as_str()])
                        .is_some_and(|m| !m.is_empty())
                    {
                        continue;
                    }
                    let neg = format!("!={}/{}-{ver}", pin.cp.0, pin.cp.1);
                    let bucket = grown.runtime_pkg_mask.entry(pin.cp.clone()).or_default();
                    if !bucket.iter().any(|m| m.neg == neg) {
                        bucket.push(MaskEntry {
                            neg: neg.clone(),
                            reason: MaskReason::SlotConflict {
                                parents: vec![(pin.consumer.clone(), pin.atom.clone())],
                            },
                        });
                        grown.mask_order.push((pin.cp.clone(), neg));
                    }
                }
            }
            // C1: enforced pins are positive enforcement atoms (the
            // consumer's recorded atom must keep matching), not
            // masks -- they stay in the positives bucket, exactly
            // like the solvable-conflict wants above.
            grown
                .slot_constraints
                .entry(pin.cp)
                .or_default()
                .push(pin.atom);
            added = true;
        }
    }
    for pin in dropped {
        if !grown.dropped_pins.contains(&pin) {
            grown.dropped_pins.push(pin);
        }
    }
    if added {
        // Backlog #209: a distinct variant (see its doc comment) so the
        // driver can feed these pins back in-pass under `--backtrack=0`,
        // where every other feedback kind still settles.
        return PassDecision::Feedback(BacktrackFeedback::RevDep {
            params: Box::new(grown),
        });
    }

    // C2: no feedback fired. A pass that still carries an unsatisfiable
    // dependency is a dead end (see `DeadEnd`), not a report -- settling
    // here would abort on the masked graph instead of the best run.
    // A conflict-only fall-through still settles (standing informational
    // convention, exit 0).
    let has_nvc = pass.suppressed_nvc
        || pass
            .entries
            .iter()
            .any(|e| matches!(e.outcome, PretendOutcome::NoVisibleCandidate));

    // #24 S3: real `_process_slot_conflicts` -> `_slot_operator_trigger_reinstalls`
    // (2131-2132, gated on `_allow_backtracking`) -> `_slot_operator_*_backtrack`
    // (2361-2452) writing `backtrack_infos["config"]
    // ["slot_operator_replace_installed"]` + `_need_restart`, consumed by
    // `resolver/backtracking.py::_feedback_config` (237-257). A consumer the
    // scan finds becomes a *walked node* on the next pass (`run_pass`'s seed
    // + flip), so its deps, its `:=` binding and its merge-order edges are
    // real -- that is the whole point of the slice; nothing is synthesised
    // after the settle any more.
    //
    // Placed last in the chain, on the settled graph: real runs the trigger
    // after the walk, and the old synthesiser ran on `assemble_result`'s
    // entries, so this sees exactly the same input it always did. A pass
    // carrying an unsatisfiable dependency is a dead end first (real never
    // reaches `_process_slot_conflicts`' tail with a failed `_create_graph`).
    // Config feedback is budget-free, and the set only ever grows -- bounded
    // by the installed cp count -- so the search terminates.
    //
    // `--backtrack=0` turns the whole trigger off, matching real's
    // `allow_backtracking = max_retries > 0` (`_backtrack_depgraph` 12190)
    // gating `_slot_operator_trigger_reinstalls` at 2131: with no search
    // there is no restart to apply the replace set on, so real schedules
    // nothing and `_forced_rebuilds` stays empty (no "causing rebuilds"
    // block either). `--nodeps` is the same via `max_retries = 0`.
    //
    // `abi_rebuilds` is always recorded (empty when the trigger is off) so
    // `assemble_result` knows the scan ran for this pass; only the
    // `get_best_run` re-pass, which never comes through here, leaves it
    // `None`.
    if has_nvc
        || ctx.backtrack_max == 0
        || ctx.ignore_built_slot_operator_deps
        || !ctx.rebuild_if_new_slot
    {
        pass.abi_rebuilds = Some(Vec::new());
    } else {
        // #268: the raw request atoms behind `top_level_cps`, for the
        // greedy-eligibility half of the `#252` gate (explicit
        // slot/version atoms pin nothing).
        let top_atoms: HashSet<String> = ctx.top_level.iter().map(|s| (*s).to_string()).collect();
        let (scheduled, abi_rebuilds, masked_binaries) = slot_operator_rebuild_scan(
            ctx.root,
            &ctx.repos,
            &pass.entries,
            &ctx.slot_op_reachable,
            &grown.slot_operator_replace_installed,
            &grown.slot_operator_undone,
            ctx.with_bdeps,
            ctx.update,
            &ctx.top_level_cps,
            &top_atoms,
            ctx.excluded,
            config,
        );
        pass.abi_rebuilds = Some(abi_rebuilds);
        // #236 A: the update probe's forcing half, for a child the walk
        // left installed (see the function's doc comment).
        let mut scheduled = scheduled;
        let (force_consumers, forced) = slot_operator_update_force_scan(
            ctx.root,
            &ctx.repos,
            config,
            &pass.entries,
            &ctx.slot_op_reachable,
            &scheduled,
            &grown.slot_operator_undone,
            ctx.with_bdeps,
            ctx.update,
            &ctx.top_level_cps,
            ctx.excluded,
        );
        scheduled.extend(force_consumers);
        let forced_new = !forced.is_subset(&grown.slot_operator_forced_upgrade);
        grown.slot_operator_forced_upgrade.extend(forced);
        // #212 (v2 `#24c`): file the scan's stale-binary triples as
        // `slot_operator_mask_built` negatives (real
        // `_slot_change_backtrack`, `depgraph.py:2381-2398`, recorded
        // into `backtrack_infos["config"]["slot_operator_mask_built"]`
        // and enforced through the backtrack-config restart,
        // `depgraph.py:5703-5712` /
        // `resolver/backtracking.py::_feedback_config:237-242`, which
        // files the mask under *both* `slot_operator_mask_built` *and*
        // `runtime_pkg_mask`). The `neg` carries a `[binary]` marker
        // (`!=cat/pkg-ver[binary]`): real keys by package object so
        // only the binary instance hides, while portuale's string
        // readers match binary and ebuild alike -- every
        // string-constraint reader skips marked entries (see
        // [`is_binary_only_mask`]) and only the binary-candidate pool
        // filter honours them. A repeated mask replaces nothing (the
        // set semantics are real's dict assignment); `mask_order`
        // takes no entry, since a same-version steer never renders a
        // missed-update row (see `backtrack_missed_updates`).
        // Mask-only growth still restarts: real sets `_need_restart`
        // unconditionally, even with an empty reinstall set.
        let mut masks_new = false;
        for (cat, pkg, ver) in &masked_binaries {
            let neg = format!("!={cat}/{pkg}-{ver}[binary]");
            let bucket = grown
                .runtime_pkg_mask
                .entry((cat.clone(), pkg.clone()))
                .or_default();
            if !bucket.iter().any(|m| m.neg == neg) {
                bucket.push(MaskEntry {
                    neg,
                    reason: MaskReason::SlotOperatorBuilt,
                });
                masks_new = true;
            }
        }
        if scheduled != grown.slot_operator_replace_installed || masks_new || forced_new {
            grown.slot_operator_replace_installed = scheduled;
            return PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(grown),
            });
        }

        // Backlog #213 (v2 `#24d`): real `_resolve`'s `prune_rebuilds`
        // check (`depgraph.py:5763-5780`) -- between the replace-set
        // restart above (real `:5703-5712` returns first when the set
        // grew this pass) and `_eliminate_rebuilds` below (real
        // `:5777-5780`, skipped on the prune pass by the early return).
        // On a pass where the replace set is non-empty and missed
        // updates exist (real `_get_missed_updates`,
        // `depgraph.py:1529-1565`; here the mask half of
        // [`backtrack_missed_updates`] -- built-binary steers never
        // render rows there, the same outcome as real's falsy-`None`
        // skip at `:1556` -- chained with the conflict half below,
        // exactly real's `:1533-1536`), real records
        // `config["prune_rebuilds"]` and restarts, and the re-resolve
        // drops the unnecessary rebuilds. `_ENABLE_PRUNE_REBUILDS`
        // (`:628`) is unconditionally true upstream. One deliberate
        // cut: the `_ignored_binaries_autounmask_backtrack` disjunct
        // (`:5764`) needs the ignored-binaries/autounmask-USE state
        // portuale's A3 overlay does not track at this layer (no pin
        // diverges on it). Fires once (the latch); Config feedback is
        // budget-free, and the re-walk re-schedules genuine rebuilds
        // through the probe above, so the search terminates with the
        // same rows and two extra passes.
        //
        // Backlog #253 (half 1): real's second missed-update source,
        // `_conflict_missed_update` (the slot-conflict handler's
        // removals, `:2087-2106`), rides `pass.skipped_updates` -- the
        // direct solve's removal rows, filed during this pass's walk
        // before this trigger runs (the withhold and residual rows join
        // only later, at settle) -- so a non-empty row set here IS the
        // conflict-source read. The per-slot-highest chaining only
        // shapes the display (`collapse_skipped_updates` at settle);
        // the trigger is a bare OR over both halves, like real's
        // truthiness check.
        //
        // Gated on no live slot conflict like `_eliminate_rebuilds`
        // below: real never reaches the check with one (its
        // `:5685-5690` conflict return precedes it); a conflict-owning
        // pass belongs to the conflict machinery above.
        if !grown.prune_rebuilds
            && !grown.slot_operator_replace_installed.is_empty()
            && pass.slot_conflicts.is_empty()
        {
            let (skipped, missing, full) =
                backtrack_missed_updates(&ctx.repos, ctx.root, config, &pass.entries, &grown, &[]);
            if !skipped.is_empty()
                || !missing.is_empty()
                || !full.is_empty()
                || !pass.skipped_updates.is_empty()
            {
                apply_prune_rebuilds(&mut grown);
                return PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(grown),
                });
            }
        }

        // #24 S4: real `_resolve`'s `_eliminate_rebuilds` call
        // (`depgraph.py:5771-5779`), which runs after the
        // `_process_slot_conflicts` trigger settled and after `altlist()`
        // succeeded -- i.e. on the pass where the replace set did not
        // grow. For each walked rebuild whose re-evaluated tree deps
        // equal its installed deps, real re-adds the installed instance
        // in place and re-serialises; portuale drops the cp from the set
        // and latches it (`slot_operator_undone`), so the next pass walks
        // the consumer as ordinary `AlreadyInstalled` again and the S3
        // scan cannot re-add it. Config feedback is budget-free, so the
        // demotion lands in place of the rebuild rather than costing a
        // backtrack step.
        //
        // Rule 0 skip (real 3867-3872, bug 922038): under `--emptytree`
        // (`ctx.empty`, checked inside) or with any live slot conflict
        // the merge list is not valid to compare against, so real returns
        // early. The report-only fall-through (`has_nvc` false, conflicts
        // present but no feedback fired) is exactly where that guard
        // matters.
        if pass.slot_conflicts.is_empty() {
            let demoted = slot_operator_eliminate_rebuilds(
                ctx.root,
                &ctx.repos,
                &pass.entries,
                &grown.slot_operator_replace_installed,
                &pass.slot_want,
                &reverse_pins,
                ctx.selective,
                ctx.changed_slot,
                &ctx.top_level_cps,
                ctx.empty,
            );
            if !demoted.is_empty() {
                for cp in &demoted {
                    grown.slot_operator_replace_installed.remove(cp);
                    grown.slot_operator_undone.insert(cp.clone());
                }
                return PassDecision::Feedback(BacktrackFeedback::Config {
                    params: Box::new(grown),
                });
            }
        }
    }

    // Backlog #221: real `_serialize_tasks`' no-leaf restart
    // (`_emerge/depgraph.py:10264-10296`, bugs 703440, 705986). A pass
    // that settled everywhere else but stranded the merge-order walk on
    // a dependency cycle records the cycle's edges into
    // `circular_dependency` and re-runs the whole walk, on which
    // `disjunction_preference` demotes the cycle-closing `||` branches
    // (see its `circular_atom` site). Placed after every other feedback:
    // real only reaches serialization once graph creation (slot
    // conflicts, missing deps, autounmask growth, reverse-dep pins, the
    // slot-operator trigger) settled. Gated on `--backtrack=0` (real's
    // `_allow_backtracking` -- the `--backtrack=0` run reports the
    // circular error exactly as before), on no unsatisfiable dependency
    // (real never serializes a failed `_create_graph`), and on no
    // surviving slot conflict or orphan blocker (real backtracks those
    // first; a conflict-only fall-through still settles below).
    // `unsolved_cycle`: a cycle re-using an already-recorded node means
    // the demotion did not break it -- settle into the circular report
    // instead of retrying, exactly like real's `_skip_restart`. Every
    // retry therefore strictly grows the map, so the search terminates.
    // `Circular` feedback is budget-free (the driver treats it like
    // `Config`: no mask step, and under `--backtrack=0` the pass
    // settles into the circular report) -- shared with backlog #216's
    // fresh-self-loop arm above, which emits the same variant.
    if ctx.backtrack_max > 0
        && !has_nvc
        && pass.slot_conflicts.is_empty()
        && pass.orphan_blockers.is_empty()
    {
        let cycles = find_restart_cycles(
            &pass.entries,
            &pass.edge_kind_map,
            ctx.root,
            &grown.circular_dependency,
        );
        if !cycles.is_empty() {
            // Track X Slice C (#242): node-identity keys (real's
            // package-object keying) -- a both-roots cp reusing an
            // already-recorded node in the *other* root is a new edge,
            // not an unsolved cycle.
            let unsolved = cycles.iter().flatten().any(|&idx| {
                grown.circular_dependency.contains_key(&(
                    pass.entries[idx].targets_running_root,
                    pass.entries[idx].category.clone(),
                    pass.entries[idx].package.clone(),
                ))
            });
            if !unsolved {
                for cycle in &cycles {
                    for (i, &idx) in cycle.iter().enumerate() {
                        // Real's rotation: `circular_child = cycle[-1]`
                        // for index 0, `cycle[index - 1]` otherwise --
                        // each node's in-cycle dependency.
                        let child = &pass.entries[cycle[(i + cycle.len() - 1) % cycle.len()]];
                        let parent = &pass.entries[idx];
                        // Merge-bound by construction (both trigger arms
                        // only link merge-bound entries), so the version
                        // is never empty in practice; skip belt-and-braces
                        // (M1): an empty version would build a `cat/pkg-`
                        // candidate no atom matches.
                        let Some(child_version) = merge_bound_version(&child.outcome).cloned()
                        else {
                            continue;
                        };
                        if child_version.is_empty() {
                            continue;
                        }
                        let record = CircularDepChild {
                            category: child.category.clone(),
                            package: child.package.clone(),
                            version: child_version,
                            slot: child.slot.clone(),
                            sub_slot: child.sub_slot.clone(),
                        };
                        let bucket = grown
                            .circular_dependency
                            .entry((
                                parent.targets_running_root,
                                parent.category.clone(),
                                parent.package.clone(),
                            ))
                            .or_default();
                        if !bucket.contains(&record) {
                            bucket.push(record);
                        }
                    }
                }
                return PassDecision::Feedback(BacktrackFeedback::Circular {
                    params: Box::new(grown),
                });
            }
        }
    }

    // Backlog #215 (v2 `#24f`): real `_slot_operator_unsatisfied_probe`
    // (`_emerge/depgraph.py:2817`, fired in-walk at `:3447-3458`) as a
    // dead-end-pass trigger -- see [`slot_operator_unsatisfied_probe`].
    // A pass whose only way forward is an installed parent's
    // unsatisfiable built `:=` dep restarts with that parent seeded
    // into `slot_operator_replace_installed` (real's
    // `backtrack_infos["config"]` + `_need_restart`, the same
    // `BacktrackFeedback::Config` the #211 scan returns) instead of
    // aborting. Backtrack-gated like real's `_allow_backtracking`
    // (`:3447`): under `--backtrack=0` the abort stands. The seed only
    // ever grows the set, so the search terminates; a pass with no
    // replacement stays a dead end below.
    if has_nvc && ctx.backtrack_max > 0 {
        let (fresh, binary_masks) = slot_operator_unsatisfied_probe_full(
            ctx.root,
            &ctx.repos,
            &pass.entries,
            &pass.slot_want,
            &pass.slot_pullers,
            &grown.runtime_pkg_mask,
            ctx.with_bdeps,
            ctx.excluded,
            config,
        );
        // #257: a non-installed built (binary) parent is masked like
        // #212's stale binaries (`[binary]`-marked `SlotOperatorBuilt`
        // negative, `runtime_pkg_mask` bucket), real `:2896-2903`.
        // Filed through the shared helper with the trigger arm above.
        if file_unsat_probe_results(&mut grown, fresh, binary_masks, ctx.usepkgonly) {
            return PassDecision::Feedback(BacktrackFeedback::Config {
                params: Box::new(grown),
            });
        }
    }

    if has_nvc {
        PassDecision::DeadEnd {
            params: Box::new(grown),
        }
    } else {
        PassDecision::Settle {
            params: Box::new(grown),
        }
    }
}

/// Phase A2 (023): the settle tail of `backtracking_resolve` --
/// rebuild passes, trace dump, merge-order sort, autounmask
/// coalesce, residual conflicts, masked chains, `abort_outcome`,
/// and the `GraphResult`. Residual conflicts (`slot_conflicts`
/// extension) live here, not in `PassResult`: no `continue`
/// follows them, so they cannot influence a retry.
/// Backlog #205: USE changes start their chain at the forcing
/// parent, not the first requirer. Real `_get_dep_chain(pkg,
/// unsatisfied_dependency=True)` (`depgraph.py:6257`) picks the
/// parent whose atom the package still does not satisfy
/// (`best_match_to_list`); when every parent is satisfied
/// post-change -- the ordinary flip shape, the change fixed them
/// all -- it walks the digraph parents preferring the last merge
/// parent (bug #354747), i.e. the deepest forcing edge, then the
/// argument. The flip site records its forcing owner in
/// `AutounmaskChange::trigger`; the fill starts there when it is
/// still a requirer, else the legacy first-requirer start stands
/// (stale triggers after a masking retry, parent-flip rescues).
/// (Keyword/license/mask changes keep the legacy start: real
/// renders those without the unsatisfied filter -- see
/// [`fill_non_use_change_chains`].) The ascent above that start is
/// unchanged (single-branch, first-requirer hops).
/// Backlog #248: no requirer at all means the change sits on a start
/// node (an argument package) -- real `_get_dep_chain` never prints
/// the start node, so the fill passes `None` and
/// [`autounmask_dep_chain`] renders just the `(argument)` line. (The
/// old `.or(Some(own))` fallback started the walk at the change's own
/// package, printing its `# required by <cpv>::<repo>` row first.)
pub(crate) fn fill_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| {
                    change
                        .trigger
                        .clone()
                        .filter(|t| e.required_by.contains(t))
                        .or_else(|| e.required_by.first().cloned())
                });
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}

/// Backlog #248: keyword/license/mask twin of
/// [`fill_use_change_chains`]. Re-derive from the change's own atom:
/// its left-hand `=<cpv>` / `>=<cpv>` names the package the change is
/// on, whose requirer chain is what real `_get_dep_chain_as_comment`
/// prints (real renders those without the unsatisfied filter, so no
/// `trigger` narrowing here). Same no-start-node rule: a change on an
/// argument package (no requirers) passes `None`, yielding just the
/// `(argument)` line.
pub(crate) fn fill_non_use_change_chains(
    entries: &[GraphEntry],
    changes: &mut [AutounmaskChange],
    top_level: &std::collections::HashSet<&str>,
    root: &Path,
) {
    for change in changes.iter_mut() {
        if change.dep_chain.is_empty()
            && let Some(atom) = portage_dep::parse_atom(&change.atom)
        {
            let owner = entries
                .iter()
                .find(|e| e.category == atom.category && e.package == atom.package)
                .and_then(|e| e.required_by.first().cloned());
            change.dep_chain = autounmask_dep_chain(&owner, &change.atom, top_level, entries, root);
        }
    }
}
