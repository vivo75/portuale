//! One resolver pass over the graph.

use crate::*;
use std::ops::ControlFlow;

mod candidate_pool;
mod enqueue_deps;
mod entry_fetch;
mod entry_use_display;
mod entry_use_flags;
mod existing_slot;
mod graph_entry;
mod parent_use_flip;
mod slot_conflicts;
mod top_level_unsatisfied;
mod unresolved_dep;
pub(crate) use candidate_pool::*;
pub(crate) use enqueue_deps::*;
pub(crate) use entry_fetch::*;
pub(crate) use entry_use_display::*;
pub(crate) use entry_use_flags::*;
pub(crate) use existing_slot::*;
pub(crate) use graph_entry::*;
pub(crate) use parent_use_flip::*;
pub(crate) use slot_conflicts::*;
pub(crate) use top_level_unsatisfied::*;
pub(crate) use unresolved_dep::*;

/// One full BFS walk over the current `bp` (Phase A3). Takes `&bp`, never
/// `&mut bp`: flips record into the pass-local overlay, which `collect_feedback`
/// and the driver fold into `bp` before deciding. The `REQUIRED_USE` early-`Err`
/// is the only failure mode.
pub(crate) fn run_pass(
    ctx: &ResolveCtx,
    bp: &BacktrackParams,
    first_pass: bool,
) -> Result<PassResult, Error> {
    // The effective config for this pass (cloned out of `bp`, so only `&bp`
    // is borrowed here).
    let config_owned: Option<portage_profile::Config> = bp.backtrack_config.clone();
    let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
    let mut state = PassState::default();
    // Pass-constant: `bp` is never mutated mid-walk (only the overlay).
    let union_constraints = bp.union_constraints();
    if resolver_debug() && first_pass {
        for a in ctx.atoms {
            resolver_trace::tr!("\n      Arg: {a}\n     Atom: {a}\n");
        }
    }
    for a in ctx.atoms.iter().rev() {
        // A top-level atom has no parent to flip a flag on, so no unevaluated
        // form (Portage never suggests a parent-flag fix for one either).
        // Reversed declaration order: Portage feeds same-level atoms through a
        // stack, so the last declared resolves first; selection reuses an
        // already-graphed satisfier, making order visible.
        state.queue.push_back(QueueItem {
            atom: a.clone(),
            depth: 0,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }
    // The slot-operator auto-replace set rides as root nodes after the user args
    // (which is why the rebuilt consumer's deps re-walk at all). `owner: None`
    // mirrors a set arg having no parent; `depth: 1` keeps them out of `--deep`
    // accounting and the `(Argument)` puller line.
    // Portage: depgraph.py, _gen_reinstall_sets; _eliminate_rebuilds rule 5
    for (cat, pkg) in &bp.slot_operator_replace_installed {
        state.queue.push_back(QueueItem {
            atom: format!("{cat}/{pkg}"),
            depth: 1,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }
    // The probe's upgraded child rides the same auto set, seeded at the exact
    // candidate the probe chose (Portage's `abi_reinstalls`).
    for (cat, pkg, ver) in &bp.slot_operator_forced_upgrade {
        state.queue.push_back(QueueItem {
            atom: format!("={cat}/{pkg}-{ver}"),
            depth: 1,
            owner: None,
            owner_targets_running_root: false,
            unevaluated: None,
            targets_running_root: false,
            buildtime_hard: false,
            from_disjunction: false,
        });
    }

    let pprovided_refs: Vec<&str> = config.package_provided.iter().map(String::as_str).collect();

    // Portage pops its `_dep_stack` LIFO, so the last argument's whole subtree
    // walks before the previous argument's deps are looked at; a FIFO queue
    // would interleave the subtrees. Children each seed enqueues drain as one
    // block, in seed order. Portage: depgraph.py, _create_graph
    let mut seeds_left = state.queue.len();
    let mut seed_split_at: Option<usize> = None;
    let mut held_blocks: VecDeque<VecDeque<QueueItem>> = VecDeque::new();
    'queue: loop {
        if let Some(n) = seed_split_at.take()
            && state.queue.len() > n
        {
            held_blocks.push_back(state.queue.split_off(n));
        }
        if state.queue.is_empty() {
            match held_blocks.pop_front() {
                Some(block) => state.queue = block,
                None => break,
            }
        }
        let Some(QueueItem {
            atom: current_atom,
            depth,
            owner,
            owner_targets_running_root: item_owner_targets_running_root,
            unevaluated: unevaluated_atom,
            targets_running_root: item_targets_running_root,
            buildtime_hard,
            from_disjunction,
        }) = state.queue.pop_front()
        else {
            break;
        };
        if seeds_left > 0 {
            seeds_left -= 1;
            seed_split_at = Some(state.queue.len());
        }
        let Some(atom) = portage_dep::parse_atom(&current_atom) else {
            if owner.is_some() {
                note_unparsed_dep_token(&current_atom, "resolver queue");
            }
            continue;
        };
        if atom.blocker != portage_dep::Blocker::None {
            continue;
        }
        // `package.provided`: an atom satisfied by a listed CPV is already
        // installed (deps drop silently; top-level atoms record for the
        // `WARNING:` block). Portage: dep_check.py; depgraph.py
        if !pprovided_refs.is_empty()
            && portage_dep::match_from_list(&current_atom, &pprovided_refs)
                .is_some_and(|m| !m.is_empty())
        {
            if depth == 0 && !state.pprovided_atoms.contains(&current_atom) {
                state.pprovided_atoms.push(current_atom.clone());
            }
            continue;
        }
        let key = (atom.category.clone(), atom.package.clone());
        // This atom's resolving root (Portage `_dep_expand`'s `root_config`); the
        // flag implies a cross root (set only when roots differ).
        let atom_root: &Path = if item_targets_running_root {
            ctx.cross_root()
                .expect("running-root atom without a cross root")
        } else {
            ctx.root
        };
        // A soft running edge back into a visited atom (a running-`BDEPEND`
        // bootstrap cycle) cuts with no edge and no entry; a hard running edge
        // back still records its edge so the cycle reports, as does a same-root
        // soft revisit (both owners list in `--json` `required_by`).
        if owner.is_some()
            && state.visited_atoms.contains(&current_atom)
            && item_targets_running_root
            && !buildtime_hard
        {
            continue 'queue;
        }
        if let Some(owner) = owner.clone() {
            state
                .required_by_map
                .entry(key.clone())
                .or_default()
                .insert(owner.clone());
            // Edge ends are root-qualified (Portage's `(root, cp)` keys); the child
            // root is the atom's own resolving root.
            let kinds = state
                .edge_kind_map
                .entry((
                    (item_targets_running_root, key.0.clone(), key.1.clone()),
                    (
                        item_owner_targets_running_root,
                        owner.0.clone(),
                        owner.1.clone(),
                    ),
                ))
                .or_insert((false, false));
            // Only a genuinely unsatisfied build-time dep counts as `has_hard`: a
            // satisfied one (`DepPrioritySatisfiedRange` can ignore it) is not
            // unbreakable. Satisfaction honours use-deps against the installed
            // instance's USE and reads this atom's own resolving root.
            // Portage: DepPriority.satisfied (vardb.match_pkgs).
            if buildtime_hard && best_installed_matching(atom_root, &current_atom, config).is_none()
            {
                kinds.0 = true;
            } else {
                kinds.1 = true;
            }
        }
        if !state.visited_atoms.insert(current_atom.clone()) {
            // Already resolved, but Portage's live `--debug` trace still narrates
            // the re-visit: re-record the edge for the dump's bounded
            // re-narration. `--debug` only; production runs are bit-identical.
            if resolver_debug()
                && let Some(prior) = state
                    .parent_atoms
                    .iter()
                    .find(|r| r.atom == current_atom)
                    .map(|r| {
                        (
                            r.child_category.clone(),
                            r.child_package.clone(),
                            r.child_version.clone(),
                            r.child_installed,
                        )
                    })
            {
                let row = resolver_trace::ParentAtom {
                    child_category: prior.0,
                    child_package: prior.1,
                    child_version: prior.2,
                    child_installed: prior.3,
                    parent: owner.clone(),
                    atom: current_atom.clone(),
                    unevaluated: unevaluated_atom.clone(),
                };
                if !state.parent_atoms.contains(&row) {
                    state.parent_atoms.push(row);
                }
            }
            continue;
        }
        // `--debug` Stage 3: every candidate matching this atom, in Portage's
        // `dbs` order. First pass only. Portage: depgraph.py,
        // _wrapped_select_pkg_highest_available_imp
        if resolver_debug() && first_pass {
            resolver_trace::dump_atom_candidates(
                &ctx.repos,
                ctx.root,
                &current_atom,
                &key.0,
                &key.1,
            );
        }
        // Record this atom as a constraint pulling `cat/pkg` (Portage sees the
        // whole atom set, not just the first).
        state
            .slot_want
            .entry(key.clone())
            .or_default()
            .push(current_atom.clone());
        // A top-level atom is an "Argument" puller for the slot-collision block;
        // `depth == 0` excludes the auto-replace seeds (an internal set, never
        // an argument). Portage: slot_collision_handler.
        if owner.is_none() && depth == 0 {
            state.slot_pullers.entry(key.clone()).or_default().push((
                String::new(),
                String::new(),
                String::new(),
                current_atom.clone(),
            ));
        }

        // After a solvable slot conflict every parent atom targeting this `cat/pkg`
        // enforces together, so one version satisfies all and the conflict clears.
        let empty_constraints: Vec<String> = Vec::new();
        let extra_constraints = union_constraints.get(&key).unwrap_or(&empty_constraints);

        // Complete mode: the deep re-walk may only pick a package already in the
        // graph or installed, never anything new (installed match settles
        // `AlreadyInstalled`; nothing installed drops the node). Before
        // `resolve_pretend`, not after: resolving every installed package fully
        // is the work Portage's cheap check avoids. No-op unless the CLI's
        // complete pass populated `complete_locked_merges`; top-level atoms
        // always resolve (their miss is fatal). Portage: depgraph.py,
        // _complete_graph (_select_pkg_from_graph).
        let resolve_atom = |a: &str, a_running: bool| -> Result<PretendOutcome, Error> {
            let select_root: &Path = if a_running {
                ctx.cross_root()
                    .expect("running-root atom without a cross root")
            } else {
                ctx.root
            };
            resolve_pretend(
                &SelectSource {
                    repos: &ctx.repos,
                    root: select_root,
                    config,
                    local_binpkg: &ctx.local_binpkg,
                },
                a,
                &SelectOptions {
                    is_top_level: depth == 0,
                    extra_constraints,
                    ..ctx.select_options(bp)
                },
            )
        };
        let complete_locked = ctx.complete
            && !ctx.complete_locked_merges.is_empty()
            && owner.is_some()
            && !ctx.complete_locked_merges.contains(&key);
        // Portage `_minimize_children`: per cp with two or more distinct picks,
        // re-point an atom whose pick another selection also satisfies at that
        // one (installed first, ascending). The collapse point: the owner's
        // contiguous same-cp siblings resolve as a group and pin every member.
        // `||`-chosen atoms stay out, like the per-disjunction call.
        // Portage: depgraph.py, _minimize_children
        if let Some(o) = owner.as_ref()
            && !from_disjunction
            && !state
                .minimize_pins
                .contains_key(&(o.clone(), current_atom.clone()))
        {
            let mut group: Vec<(String, bool, bool)> = vec![(
                current_atom.clone(),
                buildtime_hard,
                item_targets_running_root,
            )];
            for it in state.queue.iter() {
                if it.owner.as_ref() != Some(o) || it.depth != depth || it.from_disjunction {
                    break;
                }
                if let Some(a) = portage_dep::parse_atom(&it.atom)
                    && a.blocker == portage_dep::Blocker::None
                    && a.category == key.0
                    && a.package == key.1
                    && !group.iter().any(|(g, _, _)| *g == it.atom)
                {
                    group.push((it.atom.clone(), it.buildtime_hard, it.targets_running_root));
                }
            }
            if group.len() >= 2 {
                let mut picks: Vec<(String, PretendOutcome)> = Vec::new();
                for (a, bh, running) in &group {
                    let mut out = if complete_locked {
                        match best_installed_for_atom(ctx.root, a, &key.0, &key.1) {
                            Some(v) => PretendOutcome::AlreadyInstalled { version: v },
                            None => continue,
                        }
                    } else {
                        resolve_atom(a, *running)?
                    };
                    // The installed fallback below, applied to the prediction too
                    // (each sibling resolves against its own root).
                    if matches!(out, PretendOutcome::NoVisibleCandidate)
                        && depth != 0
                        && !bh
                        && !ctx.empty
                        && portage_dep::parse_atom(a)
                            .is_some_and(|p| p.slot.is_some() || p.slot_operator.is_some())
                        && let Some(version) = best_installed_matching(
                            if *running {
                                ctx.cross_root()
                                    .expect("running-root atom without a cross root")
                            } else {
                                ctx.root
                            },
                            a,
                            config,
                        )
                    {
                        out = PretendOutcome::AlreadyInstalled { version };
                    }
                    picks.push((a.clone(), out));
                }
                // `minimize_children` reads installed slots out of the group's own
                // resolving root.
                for (a, out) in minimize_children(atom_root, &ctx.repos, config, &key, picks) {
                    state.minimize_pins.insert((o.clone(), a), out);
                }
            }
        }
        let pinned = owner.as_ref().and_then(|o| {
            state
                .minimize_pins
                .remove(&(o.clone(), current_atom.clone()))
        });
        let mut outcome = if let Some(pinned) = pinned {
            pinned
        } else if complete_locked {
            match best_installed_for_atom(ctx.root, &current_atom, &key.0, &key.1) {
                Some(v) => PretendOutcome::AlreadyInstalled { version: v },
                None => continue 'queue,
            }
        } else {
            resolve_atom(&current_atom, item_targets_running_root)?
        };

        // An argument-pinned installed instance satisfying this atom reuses it
        // instead of scheduling a merge (fresh verdicts only, never `||`-chosen).
        // See `arg_pinned_installed_reuse`.
        if !from_disjunction
            && let Some(fresh) = match &outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
                _ => None,
            }
            && let Some(kept) = arg_pinned_installed_reuse(ArgPinnedInstalledReuseArgs {
                ctx,
                config,
                bp,
                union_constraints: &union_constraints,
                cache: &mut state.top_pinned_installed,
                atom_root,
                key: &key,
                current_atom: &current_atom,
                atom: &atom,
                fresh_version: &fresh,
            })
        {
            outcome = PretendOutcome::AlreadyInstalled { version: kept };
        }

        // A slot-operator dependency no ebuild satisfies but an installed instance
        // does settles `AlreadyInstalled`, never a miss (Portage iterates the
        // installed db on every resolve). Scoped to genuine non-build-time
        // dependency edges; `||`-chosen edges keep their miss (the disjunction
        // layer classifies installed-ness itself); bare and version-range atoms
        // stay on the old path. Match requires the installed USE to satisfy the
        // atom's use-deps, and reads this atom's own resolving root.
        // Portage: depgraph.py, _wrapped_select_pkg_highest_available_imp.
        if matches!(outcome, PretendOutcome::NoVisibleCandidate)
            && owner.is_some()
            && depth != 0
            && !buildtime_hard
            && !from_disjunction
            && !ctx.empty
            && (atom.slot.is_some() || atom.slot_operator.is_some())
            && let Some(version) = best_installed_matching(atom_root, &current_atom, config)
        {
            outcome = PretendOutcome::AlreadyInstalled { version };
        }
        // A running-root dep no ebuild satisfies but the running root has
        // installed settles `AlreadyInstalled` (Portage resolves those groups
        // against the running root's installed db). Loosens both gates above
        // (`buildtime_hard`, slot-operator-only); `||` and `--emptytree` stay out.
        if matches!(outcome, PretendOutcome::NoVisibleCandidate)
            && owner.is_some()
            && depth != 0
            && !from_disjunction
            && !ctx.empty
            && item_targets_running_root
            && let Some(version) = best_installed_matching(atom_root, &current_atom, config)
        {
            outcome = PretendOutcome::AlreadyInstalled { version };
        }

        // `--reinstall-atoms` forces a matching installed package to re-merge
        // (Portage drops it from every `inst_pkgs` list): the `--emptytree`
        // rewrite, scoped to the matched atom.
        // The same flip point carries the auto slot-operator `force_reinstall`:
        // keyed on `cat/pkg` so whichever visit comes first flips (patching the
        // entry afterwards would leave its deps unenqueued).
        // Portage: depgraph.py, _gen_reinstall_sets.
        // An installed child the update probe forced is never kept: the first visit
        // selects the probe's candidate (like `force_reinstall` excluding the
        // installed instance).
        if let PretendOutcome::AlreadyInstalled { version } = &outcome
            && let Some((_, _, target)) = bp
                .slot_operator_forced_upgrade
                .iter()
                .find(|(c, p, _)| *c == key.0 && *p == key.1)
            && target != version
        {
            let forced = resolve_atom(
                &format!("={}/{}-{target}", key.0, key.1),
                item_targets_running_root,
            )?;
            if !matches!(forced, PretendOutcome::NoVisibleCandidate) {
                outcome = forced;
            }
        }
        if let PretendOutcome::AlreadyInstalled { version } = &outcome {
            let cpv = format!("{}/{}-{version}", key.0, key.1);
            if ctx
                .reinstall_atoms
                .iter()
                .any(|a| matches_config_entry(a, &cpv, &key.0, &key.1))
            {
                outcome = PretendOutcome::Reinstall {
                    version: version.clone(),
                    changed_flags: Vec::new(),
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: false,
                };
            } else if bp.slot_operator_replace_installed.contains(&key) {
                outcome = PretendOutcome::Reinstall {
                    version: version.clone(),
                    changed_flags: Vec::new(),
                    deps_changed: false,
                    slot_changed: false,
                    rebuilt_binary: false,
                    new_repo: false,
                    slot_operator_rebuild: true,
                };
            }
        }

        // Portage `_add_pkg`'s per-instance parent-atom record, taken after the
        // outcome flips and before the dedup below, so the child is what this
        // atom resolved to. A shadowed `AlreadyInstalled` keeps its own row.
        // Portage: depgraph.py, _add_parent_atom.
        if resolver_debug()
            && let Some((child_version, child_installed)) = match &outcome {
                PretendOutcome::AlreadyInstalled { version } => Some((version.clone(), true)),
                PretendOutcome::New { version } => Some((version.clone(), false)),
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some((to.clone(), false))
                }
                PretendOutcome::Reinstall { version, .. } => Some((version.clone(), false)),
                // A removal never resolves a dependency atom.
                PretendOutcome::NoVisibleCandidate | PretendOutcome::Uninstall { .. } => None,
            }
        {
            let row = resolver_trace::ParentAtom {
                child_category: key.0.clone(),
                child_package: key.1.clone(),
                child_version,
                child_installed,
                parent: owner.clone(),
                atom: current_atom.clone(),
                unevaluated: unevaluated_atom.clone(),
            };
            if !state.parent_atoms.contains(&row) {
                state.parent_atoms.push(row);
            }
        }

        // A dependency whose use-dep was conditional on the parent's USE and no
        // candidate satisfies the evaluated form: flip the parent's flag instead
        // (recorded, or folded for a backtrack re-walk). A probe re-resolution
        // gates both arms, so only a helping flip records. See `parent_use_flip`.
        // Portage: depgraph.py (_apply_parent_use_changes, _success_without_autounmask).
        let ControlFlow::Continue((current_atom, atom)) = parent_use_flip(ParentUseFlipArgs {
            outcome: &outcome,
            bp,
            depth,
            current_atom,
            atom,
            owner: &owner,
            unevaluated_atom: &unevaluated_atom,
            ctx,
            state: &mut state,
            config,
            key: &key,
        })?
        else {
            continue 'queue;
        };

        // `--changed-deps-report` stays silent with `--changed-deps` (Portage
        // discards its own dict unread), so skip computing `deps_changed` at all
        // in that case. Only installed-naming outcomes qualify; a `Reinstall`
        // here is always for another trigger, so it still fires independently.
        if ctx.changed_deps_report && !ctx.changed_deps {
            let installed_version = match &outcome {
                PretendOutcome::AlreadyInstalled { version }
                | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            if let Some(version) = installed_version {
                let dedup_key = (key.0.clone(), key.1.clone(), version.clone());
                if state.changed_deps_report_seen.insert(dedup_key)
                    && deps_changed(DepsChangedArgs {
                        root: ctx.root,
                        repos: &ctx.repos,
                        category: &key.0,
                        package: &key.1,
                        version: &version,
                        with_bdeps: ctx.with_bdeps,
                    })
                    && let Ok(repo_candidates) = list_candidates(&ctx.repos, &key.0, &key.1)
                    && let Some(c) = repo_candidates.iter().find(|c| c.version == version)
                {
                    state
                        .changed_deps_report_entries
                        .push(ChangedDepsReportEntry {
                            category: key.0.clone(),
                            package: key.1.clone(),
                            version,
                            repo_name: c.repo_name.clone(),
                        });
                }
            }
        }

        if step_top_level_unsatisfied(StepTopLevelUnsatisfiedArgs {
            ctx,
            current_atom: &current_atom,
            outcome: &mut outcome,
            state: &mut state,
            union_constraints: &union_constraints,
            key: &key,
            config,
            unevaluated_atom: &unevaluated_atom,
            bp,
            atom: &atom,
        })?
        .is_break()
        {
            continue 'queue;
        }

        let resolved_version = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
            _ => None,
        };

        let Some(version) = resolved_version else {
            record_unresolved_dep(RecordUnresolvedDepArgs {
                current_atom: &current_atom,
                bp,
                key,
                outcome,
                ctx,
                state: &mut state,
                config,
                owner: &owner,
                atom_root,
                item_targets_running_root,
                unevaluated_atom: &unevaluated_atom,
                depth,
                union_constraints: &union_constraints,
                atom: &atom,
            });
            continue;
        };

        let ControlFlow::Continue(repo_candidates) = candidate_pool(CandidatePoolArgs {
            ctx,
            key: &key,
            config,
            current_atom: &current_atom,
            atom: &atom,
            bp,
        }) else {
            continue 'queue;
        };
        let Some(resolved) = repo_candidates
            .iter()
            .filter(|c| c.version == *version)
            .max_by(|a, b| {
                // "Ebuild type is the last resort": a binary beats the ebuild at a
                // version tie. Portage: _wrapped_select_pkg_highest_available_imp
                let src_rank = |c: &Candidate| match c.source {
                    CandidateSource::Binary => 1u8,
                    CandidateSource::Ebuild => 0,
                };
                src_rank(a)
                    .cmp(&src_rank(b))
                    .then(a.repo_priority.cmp(&b.repo_priority))
            })
        else {
            continue;
        };
        let slot = resolved.slot.clone();
        let sub_slot = resolved.sub_slot.clone();
        let repo_location = resolved.repo_location.clone();
        let repo_name = resolved.repo_name.clone();
        let keywords = resolved.keywords.clone();

        // Node identity includes the resolving root: a running-root atom never
        // reuses a target-root slot (the cross-root in-graph boundary).
        let slot_key = (
            key.0.clone(),
            key.1.clone(),
            slot.clone(),
            item_targets_running_root,
        );
        if step_existing_slot(StepExistingSlotArgs {
            state: &mut state,
            slot_key: &slot_key,
            ctx,
            key: &key,
            slot: &slot,
            current_atom: &current_atom,
            atom: &atom,
            config,
            version: &version,
            bp,
            owner: &owner,
            union_constraints: &union_constraints,
        })
        .is_break()
        {
            continue 'queue;
        }
        // The mirror of the `AlreadyInstalled` check (opposite argv order): the
        // slot holds no merge but an installed node this atom rejects. Falls
        // through -- the merge still graphs, while the collision reports.
        if let Some(installed_version) = state.installed_slots.get(&slot_key).cloned() {
            let installed_sub = installed_refs(ctx.root, &key.0, &key.1)
                .into_iter()
                .find(|r| r.version == installed_version)
                .map(|r| r.sub_slot)
                .unwrap_or_default();
            let installed_str = if installed_sub.is_empty() {
                format!("{}/{}-{installed_version}:{slot}", key.0, key.1)
            } else {
                format!(
                    "{}/{}-{installed_version}:{slot}/{installed_sub}",
                    key.0, key.1
                )
            };
            if portage_dep::match_from_list(&current_atom, &[installed_str.as_str()])
                .is_none_or(|m| m.is_empty())
            {
                record_slot_conflict(
                    &mut state.slot_conflicts,
                    build_slot_conflict(BuildSlotConflictArgs {
                        repos: &ctx.repos,
                        config,
                        root: ctx.root,
                        category: &key.0,
                        package: &key.1,
                        slot: &slot,
                        existing_version: &installed_version,
                        current_atom: &current_atom,
                        current_version: &version,
                        slot_pullers: &state.slot_pullers,
                        installed_side: Some(InstalledSide::Existing),
                    }),
                );
            }
        }
        let PushGraphEntryOut {
            entry_idx,
            candidate_source,
            candidate_remote,
            candidate_build_id,
            already_installed_outcome,
            candidate_str,
        } = push_graph_entry(PushGraphEntryArgs {
            state: &mut state,
            slot_key: &slot_key,
            key: &key,
            slot: &slot,
            depth,
            resolved,
            config,
            version: &version,
            sub_slot: &sub_slot,
            repo_name: &repo_name,
            bp,
            ctx,
            outcome,
            item_targets_running_root,
        });

        let metadata = if candidate_source == CandidateSource::Binary {
            let Some(metadata) =
                read_binary_metadata_any_instance(ReadBinaryMetadataAnyInstanceArgs {
                    config,
                    root: ctx.root,
                    local: &ctx.local_binpkg,
                    category: &key.0,
                    package: &key.1,
                    version: &version,
                    build_id: candidate_build_id.as_deref(),
                })
            else {
                continue;
            };
            std::sync::Arc::new(metadata)
        } else {
            let pf = format!("{}-{version}", key.1);
            let Ok(metadata) = repo_aux_metadata(&repo_location, &key.0, &pf) else {
                continue;
            };
            metadata
        };
        let use_flags = entry_use_flags(EntryUseFlagsArgs {
            candidate_source,
            resolved,
            config,
            metadata: &metadata,
            keywords: &keywords,
            candidate_str: &candidate_str,
            key: &key,
            bp,
            atom: &atom,
            state: &mut state,
            repo_candidates: &repo_candidates,
            owner: &owner,
        });

        record_entry_fetch(RecordEntryFetchArgs {
            candidate_source,
            resolved,
            key: &key,
            candidate_str: &candidate_str,
            config,
            state: &mut state,
            entry_idx,
            metadata: &metadata,
            use_flags: &use_flags,
            repo_location: &repo_location,
            ctx,
            candidate_remote,
            version: &version,
        });

        if record_entry_use_display(RecordEntryUseDisplayArgs {
            already_installed_outcome,
            metadata: &metadata,
            config,
            use_flags: &use_flags,
            ctx,
            key: &key,
            version: &version,
            resolved,
            owner: &owner,
            state: &mut state,
            unevaluated_atom: &unevaluated_atom,
            current_atom: &current_atom,
            entry_idx,
            keywords: &keywords,
            candidate_str: &candidate_str,
        })?
        .is_break()
        {
            continue 'queue;
        }

        // `--nodeps` pops "recurse": no dep string is ever read, so no blockers
        // are ever collected either. Portage: create_depgraph_params.py; depgraph.py
        if ctx.nodeps {
            continue;
        }

        enqueue_entry_deps(EnqueueEntryDepsArgs {
            ctx,
            depth,
            metadata: &metadata,
            candidate_source,
            state: &mut state,
            entry_idx,
            use_flags: &use_flags,
            bp,
            config,
            union_constraints: &union_constraints,
            key: &key,
            version: &version,
            candidate_str: &candidate_str,
        });
    }

    // `.get()`, not `.remove()`: several entries can share one `(category,
    // package)` (one per slot), and every one needs the same `required_by`.
    // A `.remove()` left all but the first with an empty list, which `--tree`
    // dropped to its flush-left net. Unkeyed entries keep their built
    // `required_by` (a `--root-deps` build entry names its requester itself).
    for entry in &mut state.entries {
        if let Some(owners) = state
            .required_by_map
            .get(&(entry.category.clone(), entry.package.clone()))
        {
            let mut owners: Vec<(String, String)> = owners.iter().cloned().collect();
            owners.sort();
            entry.required_by = owners;
        }
    }

    // An installed package also merged in the same slot is one node, not two:
    // drop the redundant `AlreadyInstalled` entry (the merge already carries
    // the union `required_by`, filled just above).
    let mergebound_cp_slots: HashSet<(String, String, String)> = state
        .entries
        .iter()
        .filter_map(|e| {
            let ver = match &e.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
                _ => return None,
            };
            let slot = e
                .slot
                .clone()
                .unwrap_or_else(|| read_vdb_slot(ctx.root, &e.category, &e.package, ver).0);
            Some((e.category.clone(), e.package.clone(), slot))
        })
        .collect();
    state.entries.retain(|e| {
        let PretendOutcome::AlreadyInstalled { version } = &e.outcome else {
            return true;
        };
        let (slot, _) = read_vdb_slot(ctx.root, &e.category, &e.package, version);
        !mergebound_cp_slots.contains(&(e.category.clone(), e.package.clone(), slot))
    });

    // The direct solve runs on the walked graph before the blocker phases
    // (Portage solves first, then validates). One joint record per slot, so
    // the solver keeps the jointly-satisfying version; removals ride
    // `skipped_updates` to the renderer.
    state.slot_conflicts = merge_same_slot_conflicts(MergeSameSlotConflictsArgs {
        conflicts: &state.slot_conflicts,
        pullers: &state.slot_pullers,
        repos: &ctx.repos,
        config,
        root: ctx.root,
        // A lone record misses parents walked later; at `--backtrack=0` re-file
        // them from the live pullers, like the merged ones (Portage reads the
        // final parent atoms).
        refile_singletons: ctx.backtrack_max == 0,
    });
    solve_pass_slot_conflicts(&mut state, ctx, bp, config);

    // Rows for the shadowed second instances of surviving conflicts (after the
    // solve, before blocker collection, so the new nodes join it).
    synthesize_surviving_conflict_entries(SynthesizeSurvivingConflictEntriesArgs {
        entries: &mut state.entries,
        conflicts: &state.slot_conflicts,
        required_by_map: &state.required_by_map,
        repos: &ctx.repos,
        root: ctx.root,
        config,
    });

    // The all-installed-packages blocker collection runs after the graph is
    // complete and before classification; `--nodeps` is its only early exit.
    // Portage: depgraph.py.
    if !ctx.nodeps {
        collect_unwalked_installed_blockers(CollectUnwalkedInstalledBlockersArgs {
            repos: &ctx.repos,
            root: ctx.root,
            config,
            dynamic_deps: ctx.dynamic_deps,
            ignore_built_slot_operator_deps: ctx.ignore_built_slot_operator_deps,
            disj_constraints: &union_constraints,
            root_deps_running_root: ctx.root_deps_running_root,
            entries: &state.entries,
            pending_blockers: &mut state.pending_blockers,
            installed_meta_memo: &mut state.installed_meta_memo,
        });
    }

    let conflicts = resolve_blockers_with_seeds(ResolveBlockersWithSeedsArgs {
        root: ctx.root,
        pending: &state.pending_blockers,
        entries: &state.entries,
        blocker_retry_closure: &ctx.blocker_retry_closure,
        buildpkgonly: ctx.buildpkgonly,
        set_seed_atoms: &ctx.config.blocker_retry_seed_atoms,
    });
    // Unresolved rows whose owner has no display entry ride out on the result.
    let mut orphan_blockers: Vec<OrphanBlocker> = Vec::new();
    file_blocker_conflicts(
        &mut state.entries,
        ctx.root,
        conflicts,
        &mut orphan_blockers,
    );

    // Tree-mode solved verdicts: only Replacement rows can a scheduled uninstall
    // solve, and the simulation is gated on at least one existing, so the common
    // no-blocker resolve pays nothing. Flags ride into `GraphResult` for `--tree`.
    let needs_tree_sim = state.entries.iter().any(|e| {
        e.blockers.iter().any(|b| {
            !b.unsolvable && matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. }))
        })
    });
    if needs_tree_sim {
        for (owner, blocker) in
            merge_order::tree_solved_replacements(merge_order::TreeSolvedReplacementsArgs {
                entries: &state.entries,
                top_level_atoms: ctx.atoms,
                config: ctx.config,
                root: ctx.root,
                implicit_system_deps: ctx.implicit_system_deps,
                repos: &ctx.repos,
                dynamic_deps: ctx.dynamic_deps,
                circular: &bp.circular_dependency,
            })
        {
            if let Some(b) = state
                .entries
                .get_mut(owner)
                .and_then(|e| e.blockers.get_mut(blocker))
            {
                b.tree_scheduled_uninstall = true;
            }
        }
    }

    if !state.required_use_violations.is_empty() {
        // Each block is self-delimiting (leading + trailing newline).
        return Err(Error::Detail(state.required_use_violations.join("")));
    }

    let pass = PassResult {
        entries: state.entries,
        suppressed_nvc: state.suppressed_nvc,
        slot_conflicts: state.slot_conflicts,
        skipped_updates: state.skipped_updates,
        orphan_blockers,
        slot_want: state.slot_want,
        slot_pullers: state.slot_pullers,
        masked_deps: state.masked_deps,
        use_unsat_deps: state.use_unsat_deps,
        plain_miss_deps: state.plain_miss_deps,
        nvc_dep_atoms: state.nvc_dep_atoms,
        missing_dep_trigger: state.missing_dep_trigger,
        missing_args: state.missing_args,
        autounmask_grew: state.autounmask_grew,
        parent_flip_rescued: state.parent_flip_rescued,
        edge_kind_map: state.edge_kind_map,
        changed_deps_report_entries: state.changed_deps_report_entries,
        // The `WARNING: … package.provided:` block lists requested atoms in requested
        // order (not walk order); atoms absent from argv sort last, stably.
        pprovided_atoms: {
            let mut v = state.pprovided_atoms;
            let pos = |a: &String| ctx.atoms.iter().position(|x| x == a).unwrap_or(usize::MAX);
            v.sort_by_key(pos);
            v
        },
        autounmask_keyword_changes: state.autounmask_keyword_changes,
        autounmask_use_changes: state.autounmask_use_changes,
        autounmask_license_changes: state.autounmask_license_changes,
        autounmask_mask_changes: state.autounmask_mask_changes,
        use_overlay: state.use_overlay,
        use_change_overlay: state.use_change_overlay,
        use_broke: state.use_broke,
        abi_rebuilds: None,
        parent_atoms: state.parent_atoms,
    };
    Ok(pass)
}
