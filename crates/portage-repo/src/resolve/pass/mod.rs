//! One resolver pass over the graph.

use crate::*;
use std::ops::ControlFlow;

mod enqueue_deps;
mod entry_fetch;
mod entry_use_display;
mod entry_use_flags;
mod existing_slot;
mod graph_entry;
pub(crate) use enqueue_deps::*;
pub(crate) use entry_fetch::*;
pub(crate) use entry_use_display::*;
pub(crate) use entry_use_flags::*;
pub(crate) use existing_slot::*;
pub(crate) use graph_entry::*;

/// Phase A3 (023): one full BFS walk over the current `bp`.
/// Takes `&bp`, never `&mut bp` -- the walk records its
/// autounmask flips into the pass-local overlay (`PassState`:
/// `use_overlay` / `use_change_overlay` / `use_broke`, read back
/// overlay-then-`bp` within the pass) and `collect_feedback` + driver folds them
/// into `bp` before deciding. Returns the settled pass; the
/// `REQUIRED_USE` early-`Err` below is the only failure mode.
pub(crate) fn run_pass(
    ctx: &ResolveCtx,
    bp: &BacktrackParams,
    first_pass: bool,
) -> Result<PassResult, Error> {
    // The same effective-config view the loop feeds `collect_feedback` + driver /
    // `assemble_result` (cloned out of `bp`; only `&bp` is borrowed
    // in here, so no `config_owned` dance is needed).
    let config_owned: Option<portage_profile::Config> = bp.backtrack_config.clone();
    let config: &portage_profile::Config = config_owned.as_ref().unwrap_or(ctx.config);
    let mut state = PassState::default();
    // C1: readers see positives + rendered negatives as one bucket.
    // Pass-constant: `bp` is never mutated mid-walk (only the overlay).
    let union_constraints = bp.union_constraints();
    if resolver_debug() && first_pass {
        for a in ctx.atoms {
            resolver_trace::tr!("\n      Arg: {a}\n     Atom: {a}\n");
        }
    }
    for a in ctx.atoms.iter().rev() {
        // A top-level atom has no "unevaluated" form distinct from
        // itself (no parent to ever flip a flag on), matching real
        // portage, which never suggests a parent-flag fix for a
        // top-level atom either.
        // Backlog #90 (S1): reversed declaration order -- real feeds
        // same-level atoms through a stack (`_dep_stack`), so the last
        // declared resolves first. Selection is order-sensitive (an
        // already-graphed instance satisfying the dep is reused,
        // `_select_package:8370`): forward order graphs `2.0` for a
        // bare dep and then records a conflict real never sees (the
        // `slotconflictparent` bt0 silence). `top_level_cps` still
        // comes from `req.atoms` in declaration order, untouched.
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
    // #24 S3: real `_gen_reinstall_sets` (5457-5480) turns
    // `_slot_operator_replace_installed` into the pseudo `SetArg`
    // `@__auto_slot_operator_replace_installed__`, appended to the arg
    // list *after* the user's own args (`select_files` 5430) and walked
    // as a root node -- which is why the rebuilt consumer gets its deps
    // re-walked at all. Seeded here with the same ordering.
    //
    // `owner: None` mirrors the set arg having no package parent (real's
    // `_eliminate_rebuilds` rule 5 explicitly excludes the auto sets from
    // its "the user asked for it" test, 3859-4000). `depth: 1` is real's
    // `reset_depth=False`: the node must not interact with `--deep=N`
    // depth accounting, and -- portuale-side -- must not read as a
    // top-level argument, which `depth == 0` means everywhere in this
    // walk (`resolve_pretend`'s `toplevel`, the `(Argument)` slot-puller
    // line). `top_level`/`top_level_cps` come from `req.atoms` and are
    // untouched by construction.
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
    // #236 A: the probe's upgraded child rides the same auto set (real
    // `abi_reinstalls.add((child.root, child slot atom))`), seeded at the
    // exact candidate the probe chose.
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

    // #298: real adds every argument's package first, then pops its
    // `_dep_stack` LIFO, so the last argument's whole subtree is walked
    // before the previous argument's deps are even looked at
    // (`_create_graph`, `depgraph.py:3254-3271`). Portuale's FIFO queue
    // would walk every argument's direct deps first, interleaving the
    // subtrees (`slotconflictnewconsumer slotconflictunsolvable` selected
    // the bare dep's 2.0 before the pin pair's 1.0; real adds 1.0 first).
    // The children each seed item enqueues are set aside as one block
    // and the blocks drain one after the other, in seed order.
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
        // `package.provided` (real `dep_check.py:1052` for a dependency,
        // `depgraph.py:5497-5615` for a top-level target): an atom whose
        // `cat/pkg` is listed and whose constraint one of that cp's
        // provided CPVs satisfies is treated as already installed. A
        // dependency is silently dropped (no entry, no `required_by`
        // edge -- real portage removes it from the deplist); a top-level
        // atom is recorded for the `WARNING: … package.provided:` block
        // and otherwise skipped.
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
        // Track X Slice B (#242): this atom's resolving root -- real
        // `_dep_expand`'s `root_config` (`depgraph.py:4928`). The flag
        // implies `cross_root().is_some()` (tags are only ever set when
        // the roots differ); same-root runs always resolve here.
        let atom_root: &Path = if item_targets_running_root {
            ctx.cross_root()
                .expect("running-root atom without a cross root")
        } else {
            ctx.root
        };
        // Track X Slice B fix (#242): the running-root subgraph's own
        // cycle scope -- a soft running edge back into an already-visited
        // atom (a running `BDEPEND` bootstrap cycle like
        // `rdrcyca <-> rdrcycb`, whose edges are soft via the
        // running-`BDEPEND`-soft rule in `enqueue_flat_deps`) cuts here
        // with no edge and no entry, like the old sidecar
        // `root_deps_build_seen` guard did (`resolve_root_deps_build_entries`:
        // "the second edge back into an in-progress package just yields
        // no further entry", losing one `required_by` edge by design).
        // A hard running edge back (a running `DEPEND` cycle like the
        // `cyc0z-3`/`cyc0y` cross-root `DEPEND` cycle, the #207 flip)
        // still records its edge below so the cycle reports, and a
        // same-root soft revisit (a `RDEPEND` diamond's second owner)
        // still records too (its `--json` `required_by` lists both).
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
            // Track X Slice C (#242): the edge ends are root-qualified
            // (real's `(root, cp)` tracker keys). The child's root is the
            // atom's own resolving root -- the entry this resolution
            // creates or reuses always carries the queue item's flag
            // (slot/installed dedup is root-qualified since Slice B, and
            // top-level atoms are target-rooted like their flag).
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
            // Real `DepPriority.satisfied` (`_emerge/DepPriority.py`):
            // a build-time dep already provided by an installed
            // package gets a *satisfied* priority, which
            // `_serialize_tasks`' `DepPrioritySatisfiedRange` can
            // ignore when breaking a cycle -- so it is not an
            // unbreakable edge. Only a genuinely unsatisfied
            // build-time dep counts as `has_hard`, matching this
            // map's own "unsatisfied build-time dep" contract. Under
            // `--emptytree` both ends of the `app-arch/xz-utils` <->
            // `app-portage/elt-patches` build-time cycle are still
            // installed, so real orders them by their installed
            // versions rather than reporting a circular dependency.
            // #250: real's satisfaction test is `vardb.match_pkgs(atom)`,
            // which honours the atom's use-deps against the installed
            // instance's recorded USE -- an installed `foo` built `-bar`
            // does not satisfy `foo[bar]`, so the edge stays unbreakable
            // (the version/slot-only lookup let a USE-mismatched instance
            // hide a genuine build-time cycle). Track X Slice B (#242):
            // satisfaction is tested against this atom's own resolving
            // root (real `DepPriority.satisfied` reads that root's vdb).
            if buildtime_hard && best_installed_matching(atom_root, &current_atom, config).is_none()
            {
                kinds.0 = true;
            } else {
                kinds.1 = true;
            }
        }
        if !state.visited_atoms.insert(current_atom.clone()) {
            // #277: the atom text already resolved (the single-resolution
            // invariant above), but real's live `--debug` trace still
            // narrates the re-visit (`Child:`/`Parent Dep:` at every
            // `_add_pkg` call). Re-record the edge against the earlier
            // resolution -- same text, same child by the invariant --
            // so the dump's bounded re-narration can print the
            // innermost chain. `--debug` only: `parent_atoms` feeds
            // nothing but the trace dump, so production runs are
            // bit-identical with the flag off.
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
        // `emerge --pretend --debug` Stage 3: real
        // `_wrapped_select_pkg_highest_available_imp`'s own
        // `f"{type_name + ':':>10} {cpv}::{repo}\n"` candidate list
        // (`depgraph.py:8347`) -- every candidate that matched this
        // atom, in real's `dbs` order (ebuild, then binary, then
        // installed), to stderr. First pass only.
        if resolver_debug() && first_pass {
            resolver_trace::dump_atom_candidates(
                &ctx.repos,
                ctx.root,
                &current_atom,
                &key.0,
                &key.1,
            );
        }
        // Backtracking: record this atom as one of the constraints pulling
        // `cat/pkg` (real `_select_pkg_highest_available` sees the whole
        // atom set for a package, not just the first).
        state
            .slot_want
            .entry(key.clone())
            .or_default()
            .push(current_atom.clone());
        // Backtracking slice 4: a top-level atom targeting this
        // `cat/pkg` is an "Argument" puller for the slot-collision
        // block (real `slot_collision_handler`'s `(Argument)` line).
        // #24 S3: `depth == 0` excludes the auto-replace seeds above --
        // real's `__auto_slot_operator_replace_installed__` is an
        // internal set, never an `(Argument)` puller in the
        // slot-collision block.
        if owner.is_none() && depth == 0 {
            state.slot_pullers.entry(key.clone()).or_default().push((
                String::new(),
                String::new(),
                String::new(),
                current_atom.clone(),
            ));
        }

        // Backtracking: if an earlier attempt hit a *solvable* slot
        // conflict on this `cat/pkg`, every parent atom that targeted it is
        // now enforced together, so this attempt picks the one version
        // that satisfies all of them and the conflict disappears.
        let empty_constraints: Vec<String> = Vec::new();
        let extra_constraints = union_constraints.get(&key).unwrap_or(&empty_constraints);

        // Real `_complete_graph`'s `_select_pkg_from_graph`
        // (`depgraph.py:8495`): in complete mode the deep re-walk of
        // the required sets may only pick a package already in the
        // graph or installed -- never resolve/merge anything new. For
        // a dependency atom whose `cp` isn't a genuine phase-1 merge
        // target, that means: an installed version that satisfies it
        // -> `AlreadyInstalled` (deep-walk its deps, merge nothing);
        // nothing installed -> real's `_initially_unsatisfied_deps`,
        // no graph node -> drop. Done **before** `resolve_pretend`,
        // not after -- the full candidate/visibility/USE resolution
        // for every installed package in the world is exactly the
        // work real's cheap graph-or-installed check avoids (it made
        // `emerge -pt gnome-control-center` take minutes). A no-op
        // unless `complete_locked_merges` was populated (only the CLI
        // layer's complete pass does that); top-level atoms always go
        // through `resolve_pretend` (their `NoVisibleCandidate` is
        // fatal, handled below).
        // The selection call, shared with the #236 A forced-upgrade flip
        // below (same pass-constant inputs, another atom). Track X Slice
        // B (#242): selection runs against the atom's own resolving root
        // (real `_dep_expand`'s `root_config`) -- the group loop below
        // passes each sibling's own flag, the single resolve the popped
        // item's.
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
        // #236 B: real `_minimize_children` (`_emerge/depgraph.py:4751-4854`,
        // called on every parent's selected atom set at `:4521`/`:4660`).
        // Real selects a package for each of the parent's atoms first, then
        // -- per cp with two or more distinct selections -- eliminates the
        // redundant ones, installed first and ascending, so an atom whose
        // own pick is also satisfied by another selection is re-pointed at
        // that one. The dominant shape is an installed parent's dynamic
        // deps (`FakeVartree._apply_dynamic_deps`): the live `cat/pkg:=`
        // picks the tree's highest version while the appended built
        // `>=cat/pkg-1.0:0/1=` picks the installed one, and real binds
        // both to the installed instance (the #211 update probe decides
        // any upgrade afterwards). Portuale resolves each queued atom on
        // its own, so the collapse point is built here: the owner's plain
        // siblings on the same cp are contiguous at the queue front (one
        // `enqueue_*` call pushed them together), and the first one popped
        // resolves the whole group and pins every member's outcome.
        // `||`-chosen atoms stay out, like real's per-disjunction call.
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
                    // The #233 installed fallback below, applied to the
                    // prediction too (real's selection iterates vartree --
                    // Track X Slice B: the sibling's own resolving root).
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
                // Track X Slice B (#242): `minimize_children` reads
                // installed slots out of its `root` -- the group's own
                // resolving root (homogeneous in practice; siblings share
                // one owner and one cp).
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

        // Backlog #285: an argument-pinned installed instance satisfies
        // this atom -- reuse it instead of scheduling a new merge. See
        // [`arg_pinned_installed_reuse`] for the real grounding
        // (`find_existing_node`, `depgraph.py:7854/8180-8215/8370-8371`).
        // Fires only on a fresh new-merge verdict (`New`/`Upgrade`/
        // `Downgrade` -- a `Reinstall` verdict is never overridden) and
        // never for `||`-chosen atoms (the disjunction layer owns
        // installed classification there).
        if !from_disjunction
            && let Some(fresh) = match &outcome {
                PretendOutcome::New { version } => Some(version.clone()),
                PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
                PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
                _ => None,
            }
            && let Some(kept) = arg_pinned_installed_reuse(
                ctx,
                config,
                bp,
                &union_constraints,
                &mut state.top_pinned_installed,
                atom_root,
                &key,
                &current_atom,
                &atom,
                &fresh,
            )
        {
            outcome = PretendOutcome::AlreadyInstalled { version: kept };
        }

        // Backlog #233: a runtime-keyed dependency no ebuild satisfies
        // but an installed instance does settles `AlreadyInstalled`,
        // never `NoVisibleCandidate`. Real
        // `_wrapped_select_pkg_highest_available_imp`
        // (`depgraph.py:7799-7840`) iterates the installed db
        // (`_iter_match_pkgs(root_config, "installed", atom)`) for every
        // resolve, so a stale recorded built slot-operator atom on an
        // installed world member (`dev-libs/reinstslottarget:0/1=`, the
        // target having moved `0/1 -> 0/2` without a revbump) still
        // matches the installed instance in vartree: the edge is a
        // satisfied nomerge edge, never a second instance and never a
        // miss (real merges the unrelated `@world` updates and withholds
        // the reinstall silently, rc 0; S0 probe `probe.log`). Without
        // this the walk aborted (rc 1) before the #210
        // reverse-dependency scan could engage. Scoped to genuine
        // dependency edges (`owner.is_some()`, non-top-level depth) that
        // are not build-time-hard: a pure `DEPEND`/`BDEPEND` edge targets
        // the *running* root in real (EAPI-7+ `dep_root`), which this
        // walk cannot consult outside the `--root-deps` feed-in, so its
        // long-pinned "reported, not satisfied from the target vdb"
        // behaviour stands (`rootdepsprovider` unit + contract pins).
        // `--emptytree` is excluded the same way `resolve_pretend`'s own
        // fallback excludes it (real selects no installed candidates).
        // Version match alone is not enough: `best_installed_matching`
        // (not the USE-blind `best_installed_for_atom`) also requires
        // the installed instance's recorded vdb `USE` to satisfy the
        // atom's use-deps -- real `vardb.match` semantics, the same
        // filter `dependency_avoid_update_candidate` and the `||`
        // installed classification use. Without it an evaluated
        // `[-flip]` "matched" the installed `+flip` child and the dep
        // was wrongly dropped as satisfied instead of surfacing
        // unsatisfied for the abort path (`deep_walk_*_installed_parents`
        // arm A).
        // Two further exclusions. `||`-chosen edges (`from_disjunction`)
        // keep their miss: the disjunction layer classifies
        // installed-ness itself at the group level (`dep_check.py` soft
        // 715-724) and the chosen alternative's atoms each still need a
        // visible tree candidate of their own -- an installed-only atom
        // there stays reported (the `opartlya` pin). And only
        // slot-operator atoms qualify (`atom.slot` / `slot_operator`:
        // `:slot`, `:slot/sub=`, `:=`, `:*`): a stale built
        // slot-operator dep names the slot the instance was *built*
        // against, so once no ebuild still carries that slot the
        // installed instance is the only possible satisfaction -- real's
        // installed-db iteration is the whole rule, with no competing
        // ebuild candidate the pass masks could be protecting. Bare and
        // version-range atoms stay on the old path: their NVC can mean a
        // genuinely missing package, a masked candidate, or a
        // backtrack-masked instance (the needer/othermod triangle keeps
        // its restart count; `oldmovepkg` keeps reporting), all of which
        // need machinery this site deliberately does not replicate.
        // Backtrack `extra_constraints` are likewise not consulted: the
        // `!=cpv` negatives a pass can hold against a slot-bound
        // nomerge instance are portuale's own transient
        // conflict-before-scan artifacts (masking the instance the #210
        // assembly scan is about to withhold *to*), which real's
        // integrated scan never creates -- respecting them diverts the
        // walk into missing-dep feedback instead of terminating with
        // the both-instances shape the withhold needs. Track X Slice B
        // (#242): the installed fallback reads this atom's own resolving
        // root (real iterates that root's installed db).
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
        // Track X Slice B fix (#242): a running-root `BDEPEND`/`IDEPEND`
        // (and `DEPEND`) no ebuild satisfies but the running root has
        // installed settles `AlreadyInstalled`, never
        // `NoVisibleCandidate` -- real `_add_pkg_deps` resolves those
        // groups against the running root's installed db
        // (`depgraph.py:4218-4291`), so a cross-root build without
        // `--root-deps` (`rootdepspkg`'s own `BDEPEND`
        // `rootdepsprovider`, installed in the running root, no ebuild
        // anywhere) drops it silently instead of failing. Loosens both
        // gates the #233 fallback above keeps for target-rooted atoms:
        // `buildtime_hard` (a `BDEPEND` is always hard) and the
        // slot-operator requirement (a bare `BDEPEND` like
        // `rootdepsprovider` carries no slot). Scoped to running atoms
        // with an installed match in their own resolving root
        // (`atom_root`, already the running root here) -- the
        // `cyc0z-3`/`cyc0y` fixture-only CPs are never in the host vdb
        // (the #207 flip's own determinism basis), and neither are
        // `rdrtool*`/`rdrcyc*` in `/`, so that flip and the recursion
        // pins are untouched. `||`-chosen edges stay out (the
        // disjunction layer classifies installed-ness itself, like
        // above) and `--emptytree` stays out (real selects no
        // installed candidates there).
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

        // `--reinstall-atoms`: a matching already-installed package
        // is forced to re-merge (real `depgraph.py` drops it from
        // every `inst_pkgs` list). In portuale's model that is the
        // `--emptytree` rewrite, scoped to the matched atom.
        //
        // #24 S3: the same flip point carries real's
        // `__auto_slot_operator_replace_installed__` `force_reinstall=True`
        // (`_gen_reinstall_sets` 5457-5480). Keyed on the `cat/pkg`, not
        // on the seeded atom, so whichever visit of this package comes
        // first in the walk -- the seed, or an ordinary dependency edge
        // that reached it earlier -- is the one that flips; patching the
        // entry afterwards would leave its deps unenqueued.
        // #236 A: an installed child the update probe forced (see
        // `BacktrackParams::slot_operator_forced_upgrade`) is never kept:
        // whichever visit reaches it first selects the probe's candidate,
        // like real's `force_reinstall` excluding the installed instance.
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

        // #59 S1: real `_add_pkg`'s per-instance parent-atom record
        // (`_add_parent_atom`, depgraph.py:3583-3604), taken here --
        // after the reinstall/autounmask outcome flips, before the
        // `other_outcomes` dedup below -- so the child instance is the
        // one this atom actually resolved to. An `AlreadyInstalled`
        // instance shadowed later by a same-slot merge keeps its own
        // row (`paired-1.0` under othermod's `<paired-2.0`).
        if resolver_debug()
            && let Some((child_version, child_installed)) = match &outcome {
                PretendOutcome::AlreadyInstalled { version } => Some((version.clone(), true)),
                PretendOutcome::New { version } => Some((version.clone(), false)),
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    Some((to.clone(), false))
                }
                PretendOutcome::Reinstall { version, .. } => Some((version.clone(), false)),
                // #72 B3: a removal never resolves a dependency atom.
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

        // Real `--autounmask-use` PART B (`_apply_parent_use_changes`
        // -> `_show_unsatisfied_dep(collect_use_changes=True)`,
        // `depgraph.py:5820`/`6768`): a dependency's use-dep was originally
        // conditional on the *requesting parent's* own USE (`opt?`/`opt=`
        // forms), and no candidate satisfies the evaluated form. Real
        // portage flips the *parent's* conditional flag instead
        // (`suggested_parent_use_candidate`), folds it into
        // `_needed_use_config_changes`, and -- the graph having already
        // failed -- FAILS the resolve (`_success_without_autounmask`):
        // no merge rows, just the change block (and, with
        // `--autounmask-backtrack` off, the "terminated early" notice).
        // With `--autounmask-backtrack=y` (Slice 4) the flip is folded
        // into `autounmask_use_config` (keyed `(parent_cat, parent_pkg)` --
        // exactly a `package.use` entry) and the driver's
        // `autounmask_grew` restart re-walks everything instead. A probe
        // re-resolution of just the freed atom still gates both arms, so
        // only a flip that actually helps is recorded or folded.
        // `--autounmask-use=n` suppresses it via the shared
        // `autounmask_suggest_use` gate.
        let (current_atom, atom) = 'parent_flip: {
            if !(matches!(outcome, PretendOutcome::NoVisibleCandidate)
                && bp.autounmask_suggest_use
                && depth > 0)
            {
                break 'parent_flip (current_atom, atom);
            }
            let (Some(owner_key), Some(unevaluated)) =
                (owner.as_ref(), unevaluated_atom.as_deref())
            else {
                break 'parent_flip (current_atom, atom);
            };
            let Some((pc, pp, pv, target_use)) = suggested_parent_use_candidate(
                &ctx.repos,
                &state.entries,
                unevaluated,
                owner_key,
                config,
            ) else {
                break 'parent_flip (current_atom, atom);
            };
            let Some((parent_cand, _piuse, parent_use, _pru)) =
                parent_use_state(&ctx.repos, &state.entries, owner_key, config)
            else {
                break 'parent_flip (current_atom, atom);
            };
            let mut new_parent_use = parent_use.as_ref().clone();
            for (flag, want) in &target_use {
                if *want {
                    new_parent_use.insert(flag.clone());
                } else {
                    new_parent_use.remove(flag);
                }
            }
            let Some(re_atom) =
                portage_dep::evaluate_atom_conditionals(unevaluated, &new_parent_use)
            else {
                break 'parent_flip (current_atom, atom);
            };
            let Some(_re_parsed) = portage_dep::parse_atom(&re_atom) else {
                break 'parent_flip (current_atom, atom);
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
                break 'parent_flip (current_atom, atom);
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
                let pv_disp =
                    build_use_expand_display(&disp, config, None, &forced, true, &no_reinst);
                let p_disp =
                    build_use_expand_display(&disp, config, None, &forced, false, &no_reinst);
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
                (current_atom, atom)
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
                    break 'parent_flip (current_atom, atom);
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
                    break 'parent_flip (current_atom, atom);
                }
                let parent_all = list_candidates(&ctx.repos, &pc, &pp).unwrap_or_default();
                let token = target_use
                    .iter()
                    .map(|(f, e)| if *e { f.clone() } else { format!("-{f}") })
                    .collect::<Vec<_>>()
                    .join(" ");
                let atom_form =
                    autounmask_use_atom_form(&parent_cand, &parent_all, &pc, &pp, config);
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
                continue 'queue;
            }
        };

        // `--changed-deps-report`: real portage stays "completely
        // silent" whenever `--changed-deps` itself is also given (its
        // own collected `_changed_deps_pkgs` dict is discarded unread by
        // `_changed_deps_report`'s own early return in that case) -- so,
        // rather than collecting anything now and discarding it at print
        // time, this simply never bothers computing `deps_changed` at
        // all when `changed_deps` is true, an equivalent, simpler
        // no-op-preserving shortcut. Only AlreadyInstalled/Reinstall
        // outcomes name a version that's genuinely installed right now
        // (the only case `deps_changed` -- a vdb-vs-current-ebuild
        // comparison for one specific version -- is meaningful for); a
        // Reinstall here can only be for `newuse`/`changed_use`/
        // `changed_slot` (never for `changed_deps` itself, since that's
        // false in this branch), so this still fires independently of
        // those other reasons, matching real portage's own
        // freely-combinable reinstall triggers.
        if ctx.changed_deps_report && !ctx.changed_deps {
            let installed_version = match &outcome {
                PretendOutcome::AlreadyInstalled { version }
                | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                _ => None,
            };
            if let Some(version) = installed_version {
                let dedup_key = (key.0.clone(), key.1.clone(), version.clone());
                if state.changed_deps_report_seen.insert(dedup_key)
                    && deps_changed(
                        ctx.root,
                        &ctx.repos,
                        &key.0,
                        &key.1,
                        &version,
                        ctx.with_bdeps,
                    )
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

        // A top-level atom (as opposed to a dependency reached while
        // recursing) with no visible candidate aborts the whole call --
        // matching real portage's own depgraph.py behavior for an
        // unsatisfiable target, not the "report and keep going" treatment
        // a dependency's own NoVisibleCandidate gets a few lines down.
        // Top-level atoms are always dequeued (and so reach this point)
        // in argv order, before any dependency, so this also guarantees
        // the *first* bad top-level atom is the one that aborts.
        if ctx.top_level.contains(current_atom.as_str())
            && matches!(outcome, PretendOutcome::NoVisibleCandidate)
        {
            // Backlog #286: real `_resolve` (`depgraph.py:5577-5605`) drops
            // a `@world`/`@selected` set member that selects no package
            // into `_missing_args` and continues the walk, where an
            // explicit argument fails the run. The missing parent is an
            // installed one with no merge-bound replacement (the #276
            // shape: both instances masked by now), so the healed rest of
            // the graph settles and the display layer reports the skipped
            // update plus the world-file problem from `missing_args`.
            // Deduped: a repeat visit of the same atom text (e.g. the
            // best-run re-pass) records once.
            if ctx.set_args.contains(current_atom.as_str()) {
                if !state.missing_args.contains(&current_atom) {
                    state.missing_args.push(current_atom.clone());
                }
                continue 'queue;
            }
            // Backlog #210: complete-mode withhold of a phase-1 merge
            // (see `complete_mode_withheld_version`): a top-level
            // argument an enforced parent-atom pin leaves with no
            // mergeable candidate stays installed instead of aborting.
            // Without positive pins (a genuine absence) or on a pass
            // without required-set semantics (no seeds fed -- plain
            // phase 1) the abort below stands. `slot_op_reachable` is
            // exactly that gate: it is populated on the CLI's complete
            // pass and on its `--deep` re-run alike, and empty
            // otherwise.
            let withheld_version = (!ctx.slot_op_reachable.is_empty()
                && union_constraints
                    .get(&key)
                    .is_some_and(|v| v.iter().any(|c| !c.starts_with('!'))))
            .then(|| {
                let positives: Vec<String> = union_constraints.get(&key).map_or(Vec::new(), |v| {
                    v.iter().filter(|c| !c.starts_with('!')).cloned().collect()
                });
                complete_mode_withheld_version(
                    ctx.root,
                    &key.0,
                    &key.1,
                    current_atom.as_str(),
                    &positives,
                )
            })
            .flatten();
            if let Some(version) = withheld_version {
                outcome = PretendOutcome::AlreadyInstalled { version };
            } else {
                // Real `_show_unsatisfied_dep`: an atom that matches
                // ebuilds which all turned out masked reports the "All
                // ebuilds that could satisfy … have been masked" block
                // (+ each masked candidate and why), not the "there are
                // no ebuilds to satisfy" one.
                if let Some(report) = all_masked_report(
                    &ctx.repos,
                    current_atom.as_str(),
                    config,
                    &format!("{current_atom:?}"),
                ) {
                    return Err(Error::Detail(report));
                }
                // Backlog #265(b) (with #273): a top-level atom's own
                // USE miss aborts with real `_show_unsatisfied_dep`'s
                // "no ebuilds built with USE flags to satisfy" block
                // (`depgraph.py:6969-6994`), not the bare miss -- the
                // `(Change USE: …)` / `(Missing IUSE: …)` rows and, like
                // real for an `AtomArg` parent (`:7080-7090`), no
                // `(dependency required by …)` lines. Real's staging
                // `for <root>.` suffix stays absent (the long-standing
                // `fixture-miss-message-unsuffixed` class). Ordered
                // after the masked block like real's own
                // `show_missing_use`-before-`masked_packages` precedence
                // (the dependency arm renders the same way).
                if let Some(rows) = use_unsat_candidates_for_atom(
                    &ctx.repos,
                    ctx.root,
                    &state.entries,
                    unevaluated_atom.as_deref().unwrap_or(current_atom.as_str()),
                    current_atom.as_str(),
                    None,
                    config,
                ) {
                    let mut block = format!(
                        "there are no ebuilds built with USE flags to satisfy {current_atom:?}.\n\
                         !!! One of the following packages is required to complete your request:"
                    );
                    for (cpv, reasons) in rows {
                        block.push_str(&format!("\n- {cpv} ({})", reasons.join(", ")));
                    }
                    return Err(Error::Detail(block));
                }
                let mut message = format!("there are no ebuilds to satisfy {current_atom:?}.");
                // --autounmask's own keyword-suggestion sub-feature (see
                // this function's own doc comment for the full on/off
                // default-resolution logic): only even attempted when
                // enabled, and only ever finds something to suggest when a
                // real candidate exists that's masked by KEYWORDS alone
                // (see `keyword_masked_only`'s own doc comment) -- a
                // candidate masked by package.mask/license/etc. too gets no
                // suggestion here, matching real portage's own "only
                // suggest a change that would actually fix it" spirit,
                // even though portuale doesn't yet combine multiple
                // simultaneous suggestion kinds the way real portage can.
                if bp.autounmask_suggest_keywords
                    && let Some((version, keyword)) = suggested_keyword_candidate(
                        &ctx.repos,
                        &atom.category,
                        &atom.package,
                        config,
                    )
                {
                    message.push_str(&format!(
                        "\nnote: {}/{}-{version} exists but is masked by KEYWORDS; \
                     --autounmask-keep-keywords=n suggests adding \"{}/{} {keyword}\" \
                     to package.accept_keywords",
                        atom.category, atom.package, atom.category, atom.package,
                    ));
                }
                // `--autounmask-use`'s own suggestion sub-feature -- same
                // gating/"only suggest a fix that would actually work"
                // spirit as the keyword one just above. Message format
                // mirrors real `package.use` suggestion syntax
                // (`=category/package-version flag -flag`).
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
                // `--autounmask-license`'s own suggestion sub-feature -- same
                // "only suggest a fix that would actually work" gating.
                if bp.autounmask_suggest_license
                    && let Some((version, licenses)) = suggested_license_candidate(
                        &ctx.repos,
                        &atom.category,
                        &atom.package,
                        config,
                    )
                {
                    message.push_str(&format!(
                        "\nnote: {}/{}-{version} exists but its LICENSE is not accepted; \
                     --autounmask-license suggests adding \"={}/{}-{version} {licenses}\" \
                     to package.license",
                        atom.category, atom.package, atom.category, atom.package,
                    ));
                }
                // `--autounmask-keep-masks=n`'s own suggestion sub-feature.
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

        let resolved_version = match &outcome {
            PretendOutcome::New { version } => Some(version.clone()),
            PretendOutcome::Upgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
            PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
            _ => None,
        };

        let Some(version) = resolved_version else {
            // Real `_feedback_missing_dep`: this dependency atom has
            // no matching package. If backtracking is live and its
            // parent is a merge-bound entry not yet tried, remember to
            // mask `!=parent-cpv` and retry -- so a `||` group whose
            // chosen alternative has an unsatisfiable subtree yields to
            // the next alternative, and so a top-level-selected version
            // whose dependency is unmaskable yields to the next older
            // version (backlog #198: `akk0a-2` masked, `akk0a-1` picked,
            // real `depgraph.py:3493-3503` + `backtracking.py:197-207`
            // carry no top-level exemption -- the mask names one cpv,
            // never the requested package itself). Only the first
            // such trigger per pass is acted on (like real, one
            // backtrack node at a time); the NVC entry is still built
            // below so an un-fixable dep is reported unchanged.
            // Real (depgraph.py:3473-3483) does NOT missing-dep
            // backtrack when only a USE change would satisfy the dep
            // (`_select_package(dep.atom.without_use)` still finds a
            // package) -- that path is the autounmask machinery's, and
            // an unfixable `[flag]` dep is reported as a plain NVC.
            // C3: real's "no matching package" is mask-aware (a masked
            // candidate matches nothing), so the probe consults the
            // negatives, not the bare tree: a dependency unsatisfiable
            // only because backtracking masked its candidates masks its
            // parent and retries, reaching the same downgrade the old
            // puller pre-masking produced (e.g. `btconsumer-2.0` under a
            // masked `bttarget-2.0` falls back to `btconsumer-1.0` on
            // the retry). Positives stay out (real selection has no
            // enforcement counterpart); a USE-only failure still
            // satisfies the probe and stays out, exactly as before.
            let bare_atom = current_atom
                .split_once('[')
                .map_or(current_atom.as_str(), |(b, _)| b);
            let masked_probe = bp.masked_negatives_for(&key);
            if matches!(outcome, PretendOutcome::NoVisibleCandidate)
                && ctx.backtrack_max > 0
                && state.missing_dep_trigger.is_none()
                && !atom_currently_satisfiable(&ctx.repos, bare_atom, config, &masked_probe)
                && let Some((pc, pp)) = owner.clone()
            {
                let parent_cpv = state
                    .entries
                    .iter()
                    .find(|e| e.category == pc && e.package == pp)
                    .and_then(|e| match &e.outcome {
                        PretendOutcome::New { version }
                        | PretendOutcome::Reinstall { version, .. } => Some(version.clone()),
                        PretendOutcome::Upgrade { to, .. }
                        | PretendOutcome::Downgrade { to, .. } => Some(to.clone()),
                        _ => None,
                    });
                if let Some(pv) = parent_cpv {
                    let neg = format!("!={pc}/{pp}-{pv}");
                    if !bp.missing_dep_masked.contains(&neg) {
                        state.missing_dep_trigger = Some(((pc, pp), neg, current_atom.clone()));
                    }
                } else {
                    // Backlog #276: real masks an *installed* parent
                    // whose dep cannot be satisfied (`_add_dep`
                    // `depgraph.py:3456-3470` -> `runtime_pkg_mask`
                    // "missing dependency") and restarts; the
                    // merge-bound-only arming above never fires for it,
                    // so the run aborts on the installed parent's built
                    // atom where real steers. Fall back to the highest
                    // installed instance. The `[installed]` marker hides
                    // only that instance (real keys the mask by package
                    // object -- see [`is_installed_only_mask`]), so the
                    // same-version ebuild stays visible and the next
                    // pass walks it, exactly like real's second
                    // backtracking try masking the ebuild.
                    if let Some(iv) = installed_candidates(ctx.root, &pc, &pp)
                        .into_iter()
                        .map(|(v, _, _)| v)
                        .max_by(|a, b| vercmp_ordering(a, b))
                    {
                        let neg = format!("!={pc}/{pp}-{iv}[installed]");
                        if !bp.missing_dep_masked.contains(&neg) {
                            state.missing_dep_trigger = Some(((pc, pp), neg, current_atom.clone()));
                        }
                    }
                }
            }
            // #57, direction 1: this atom settled on an *installed*
            // instance, and the same `(cat, pkg, slot)` may already hold a
            // merge-bound one that this atom rejects -- real's
            // `_package_tracker` sees both nodes and calls that a slot
            // collision (`othermod`'s `<paired-2.0` landing on installed
            // 1.0 while `needer`'s `>=paired-2.0` already graphed 2.0).
            // Recorded *before* the `other_outcomes` dedup, since a
            // repeat visit of this `cat/pkg` under a different atom text
            // is exactly how the second, conflicting constraint arrives.
            //
            // Deliberately no `continue`: real keeps the installed node
            // in the graph and still merges the other instance, and the
            // solvable/unsolvable split (real
            // `_solve_non_slot_operator_slot_conflicts`) is the
            // backtracker's -- `collect_feedback`'s `slot_want`
            // solvability check reconciles a jointly-satisfiable slot on
            // the retry and this record disappears with it; only a slot
            // no single version can satisfy survives to be reported.
            // The vdb `SLOT` file only -- one read per installed node,
            // reused by the slot-conflict record below and the
            // `other_outcomes` dedup key. `installed_refs` would re-list
            // every version and read each one's `repository` too, and
            // this runs for every `AlreadyInstalled` outcome in the walk
            // (thousands on a `-puD @world`); the fuller lookup is
            // deferred to the two rare branches that actually build a
            // record.
            let installed_node_slot = match &outcome {
                PretendOutcome::AlreadyInstalled { version } => {
                    // Track X Slice B (#242): the instance lives in this
                    // atom's own resolving root.
                    Some(read_vdb_slot(atom_root, &key.0, &key.1, version).0)
                }
                _ => None,
            };
            if let PretendOutcome::AlreadyInstalled { version } = &outcome {
                let inst_slot = installed_node_slot.clone().unwrap_or_default();
                let slot_key = (
                    key.0.clone(),
                    key.1.clone(),
                    inst_slot.clone(),
                    item_targets_running_root,
                );
                if let Some(&existing_idx) = state.resolved_slots.get(&slot_key)
                    && let Some(existing_version) =
                        merge_bound_version(&state.entries[existing_idx].outcome).cloned()
                {
                    // Same sub-slot-carrying match string the
                    // merge-outcome check below builds, for the same
                    // reason (a built `cat/pkg:0/2=` puller matches no
                    // `:slot`-only string).
                    let existing_sub =
                        slot_conflict_meta(&ctx.repos, &key.0, &key.1, &existing_version).0;
                    let existing_str = if existing_sub.is_empty() {
                        format!("{}/{}-{existing_version}:{inst_slot}", key.0, key.1)
                    } else {
                        format!(
                            "{}/{}-{existing_version}:{inst_slot}/{existing_sub}",
                            key.0, key.1
                        )
                    };
                    if portage_dep::match_from_list(&current_atom, &[existing_str.as_str()])
                        .is_none_or(|m| m.is_empty())
                        // #270: the "this is a rebuild trigger, not a conflict"
                        // forgiveness presumes the slot-operator rebuild scan
                        // will schedule the consumer, and that scan only runs
                        // with a backtrack budget (`backtrack_max > 0`, like
                        // its own call site). At `--backtrack=0` nothing
                        // schedules the rebuild, so real's in-pass solver
                        // (`_solve_non_slot_operator_slot_conflicts`,
                        // `depgraph.py:1774-2115`) sees the conflict and
                        // withholds the provider update instead of merging it
                        // alone (real 3.0.82.2: `[sct, oldc, newc]` only).
                        && !(ctx.backtrack_max > 0
                            && built_slot_operator_rebuild_trigger(&current_atom, &existing_str))
                    {
                        record_slot_conflict(
                            &mut state.slot_conflicts,
                            build_slot_conflict(
                                &ctx.repos,
                                config,
                                ctx.root,
                                &key.0,
                                &key.1,
                                &inst_slot,
                                &existing_version,
                                &current_atom,
                                version,
                                &state.slot_pullers,
                                Some(InstalledSide::Current),
                            ),
                        );
                    }
                }
                state
                    .installed_slots
                    .entry(slot_key)
                    .or_insert_with(|| version.clone());
            }
            // AlreadyInstalled repeats are keyed by the installed
            // instance's own slot (real's `pkg.slot_atom` node identity):
            // a *second* installed instance of the same cp in another
            // slot is a distinct node whose deps must still be walked
            // (#74 S2a -- the old cp key let the first slot processed
            // shadow the rest, dropping `llvm-core/clang:23`'s own build
            // deps). NoVisibleCandidate has no instance to key by: `None`
            // keeps the old cp-scoped repeat dedup.
            let other_key = (
                key.0.clone(),
                key.1.clone(),
                installed_node_slot,
                item_targets_running_root,
            );
            if !state.other_outcomes.insert(other_key) {
                // #57: a deduped `NoVisibleCandidate` never gets its own
                // entry, so under an active `runtime_pkg_mask` the
                // `has_nvc` scan must still treat the pass as a dead end
                // (real's `_create_graph` returns 0 on an unsatisfiable
                // dep and the attempt is abandoned in favour of
                // `get_best_run`; the argv order `othermod needer` used
                // to settle on the masked try otherwise). Gated on an
                // active mask so no unmasked pass changes behaviour.
                if matches!(outcome, PretendOutcome::NoVisibleCandidate)
                    && !bp.runtime_pkg_mask.is_empty()
                {
                    state.suppressed_nvc = true;
                }
                // Backlog #286: a deduped miss still refreshes the
                // disclosure when its atom is the live one real reports
                // (see `live_miss_atom_supersedes`): the repeat edge
                // carries the atom the first edge lacked, and real's
                // fail-fast walk would have hit it first. Rows refresh
                // alongside (both row builders read the new atom only).
                // Cheap gate first: the row builders run only when the
                // recorded atom is a supersedeable built one.
                if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
                    let display_atom = unevaluated_atom.as_deref().unwrap_or(&current_atom);
                    if state
                        .nvc_dep_atoms
                        .get(&key)
                        .is_some_and(|old| live_miss_atom_supersedes(old, display_atom))
                    {
                        let masked_rows =
                            masked_candidates_for_atom(&ctx.repos, display_atom, config);
                        let use_rows = use_unsat_candidates_for_atom(
                            &ctx.repos,
                            ctx.root,
                            &state.entries,
                            display_atom,
                            &current_atom,
                            owner.as_ref(),
                            config,
                        );
                        state.nvc_dep_atoms.entry(key.clone()).and_modify(|old| {
                            if live_miss_atom_supersedes(old, display_atom) {
                                *old = display_atom.to_string();
                            }
                        });
                        for rep in state
                            .masked_deps
                            .iter_mut()
                            .filter(|r| r.category == key.0 && r.package == key.1)
                        {
                            if live_miss_atom_supersedes(&rep.atom, display_atom) {
                                rep.atom = display_atom.to_string();
                                if let Some(masked) = masked_rows.clone() {
                                    rep.masked = masked;
                                }
                            }
                        }
                        for rep in state
                            .use_unsat_deps
                            .iter_mut()
                            .filter(|r| r.category == key.0 && r.package == key.1)
                        {
                            if live_miss_atom_supersedes(&rep.atom, display_atom) {
                                rep.atom = display_atom.to_string();
                                if let Some(rows) = use_rows.clone() {
                                    rep.rows = rows;
                                }
                            }
                        }
                        for rep in state
                            .plain_miss_deps
                            .iter_mut()
                            .filter(|r| r.category == key.0 && r.package == key.1)
                        {
                            if live_miss_atom_supersedes(&rep.atom, display_atom) {
                                rep.atom = display_atom.to_string();
                            }
                        }
                    }
                }
                continue;
            }
            // `--deep`: an AlreadyInstalled package's own further
            // dependencies are walked too, once `deep` allows recursion
            // at this package's own depth (see `Deep::recurses_at`'s own
            // doc comment) -- never for NoVisibleCandidate (no version to
            // look anything up by), and never when `nodeps` disables the
            // dependency walk entirely, matching every other outcome's
            // own `nodeps` handling further below. Real's own `_add_pkg`
            // gate is the same: an installed package whose deps aren't
            // recursed into gets pushed onto `_ignored_deps`, never
            // `.order`, so `deps` (below) is likewise only ever
            // computed under this identical condition.
            let mut already_installed_deps: Vec<DepEdge> = Vec::new();
            if let PretendOutcome::AlreadyInstalled { version } = &outcome
                && !ctx.nodeps
                && ctx.deep.recurses_at(depth)
            {
                // `GraphEntry::deps` for an AlreadyInstalled
                // entry: A2 (#26) builds it from the *same* installed
                // metadata view the recursion below walks
                // (`installed_dep_string`): the live ebuild's deps plus
                // the vdb's own built `:=` atoms by default (backlog #25
                // S1b), the vdb snapshot under `--dynamic-deps=n`.
                // So `--debug`'s `Depstring` line and `--tree`'s edges can no
                // longer contradict the child the walk actually queued.
                // Backlog #86: the display list reads the same
                // recorded-repo view the recursion walks (never a
                // priority search), so `--debug`/`--tree` cannot
                // contradict the queued child.
                // Track X Slice B (#242): this installed instance lives
                // in the atom's own resolving root (real `pkg.root`).
                if let Some(metadata) =
                    live_metadata_for_installed(&ctx.repos, atom_root, &key.0, &key.1, version)
                {
                    // Installed recorded USE, not effective profile
                    // USE -- see `enqueue_dependencies`'s own note
                    // just below; a `flag?`-gated dep this display
                    // list shows must match what the recursion
                    // actually queued.
                    let use_flags = read_vdb_flag_set(atom_root, &key.0, &key.1, version, "USE");
                    let portage_order_keys: &[&str] = if ctx.with_bdeps {
                        &["RDEPEND", "IDEPEND", "PDEPEND", "DEPEND", "BDEPEND"]
                    } else {
                        &["RDEPEND", "IDEPEND", "PDEPEND"]
                    };
                    let layer = if ctx.dynamic_deps {
                        InstalledMetaLayer::Effective
                    } else {
                        InstalledMetaLayer::Raw
                    };
                    let mut effective: HashMap<String, String> = HashMap::new();
                    for k in portage_order_keys {
                        let s = installed_dep_string(
                            atom_root,
                            ctx.dynamic_deps,
                            ctx.ignore_built_slot_operator_deps,
                            &key.0,
                            &key.1,
                            version,
                            Some(&metadata),
                            k,
                            layer,
                            &mut state.installed_meta_memo,
                        );
                        if !s.trim().is_empty() {
                            effective.insert((*k).to_string(), s);
                        }
                    }
                    // An installed package is `pkg.built`, so
                    // real marks its build-time deps `optional`.
                    already_installed_deps = merge_order::dep_edges_from_metadata(
                        &effective,
                        &use_flags,
                        portage_order_keys,
                        true,
                    );
                }
                enqueue_dependencies(
                    &SelectSource {
                        repos: &ctx.repos,
                        root: atom_root,
                        config,
                        local_binpkg: &ctx.local_binpkg,
                    },
                    &DepOwner {
                        target_root: ctx.root,
                        category: &key.0,
                        package: &key.1,
                        version,
                        child_depth: depth + 1,
                    },
                    &ctx.enqueue_options(),
                    EnqueueSinks {
                        queue: &mut state.queue,
                        pending_blockers: &mut state.pending_blockers,
                        entries: &mut state.entries,
                        root_deps_build_seen: &mut state.root_deps_build_seen,
                        installed_meta_memo: &mut state.installed_meta_memo,
                        slot_pullers: &mut state.slot_pullers,
                    },
                    &union_constraints,
                );
            }
            // `--autounmask`'s own keyword-suggestion sub-feature,
            // extended here to a *dependency's* own `NoVisibleCandidate`
            // -- see `GraphEntry::keyword_suggestion`'s own doc comment.
            let keyword_suggestion = if matches!(outcome, PretendOutcome::NoVisibleCandidate)
                && bp.autounmask_suggest_keywords
            {
                suggested_keyword_candidate(&ctx.repos, &key.0, &key.1, config)
            } else {
                None
            };
            // `--autounmask-use`'s own suggestion sub-feature -- see
            // `GraphEntry::use_suggestion`'s own doc comment. `atom.
            // use_deps` is the dependency atom's own use-dep spec
            // (already conditional-evaluated by `enqueue_flat_deps`
            // before this atom was ever queued, so only plain `flag`/
            // `-flag` forms survive to be checked here).
            let use_suggestion = if matches!(outcome, PretendOutcome::NoVisibleCandidate)
                && bp.autounmask_suggest_use
            {
                suggested_use_candidate(
                    &ctx.repos,
                    &key.0,
                    &key.1,
                    atom.use_deps.as_deref(),
                    config,
                )
            } else {
                None
            };
            // `--autounmask-use`'s own second, architecturally distinct
            // suggestion sub-feature -- see `suggested_parent_use_
            // candidate`'s own doc comment. Only ever attempted when
            // this atom actually had a conditional use-dep evaluated
            // away (`unevaluated_atom.is_some()`) and has a real parent
            // to flip a flag on (`owner.is_some()`, always true here: a
            // top-level atom's own `NoVisibleCandidate` already aborted
            // the whole call via the fatal check above, so any
            // `NoVisibleCandidate` reaching this point is necessarily a
            // dependency's own, which always has an `owner`).
            let parent_use_suggestion = if matches!(outcome, PretendOutcome::NoVisibleCandidate)
                && bp.autounmask_suggest_use
            {
                owner
                    .as_ref()
                    .zip(unevaluated_atom.as_deref())
                    .and_then(|(owner, unevaluated)| {
                        suggested_parent_use_candidate(
                            &ctx.repos,
                            &state.entries,
                            unevaluated,
                            owner,
                            config,
                        )
                    })
            } else {
                None
            };
            // Masked-dependency disclosure (real
            // `_show_unsatisfied_dep`'s "All ebuilds that could satisfy
            // … have been masked" block for a *dependency* -- the Tier
            // 2.20 half of finding S's top-level report): a dependency
            // `NoVisibleCandidate` whose atom still matches ebuilds by
            // version, every one of which is masked, is recorded here
            // (one per `cat/pkg`, like the entry itself) instead of
            // rendering the bare `!!! no visible ebuild` line. The
            // display atom is the unevaluated form as queued, so a
            // USE-conditional dep shows as written. Computed every
            // pass; only the final pass's reports are rendered.
            if matches!(outcome, PretendOutcome::NoVisibleCandidate) {
                let display_atom = unevaluated_atom.as_deref().unwrap_or(&current_atom);
                state
                    .nvc_dep_atoms
                    .entry(key.clone())
                    .and_modify(|old| {
                        // Backlog #286: a live slot-operator miss
                        // supersedes a built one (see
                        // `live_miss_atom_supersedes`).
                        if live_miss_atom_supersedes(old, display_atom) {
                            *old = display_atom.to_string();
                        }
                    })
                    .or_insert_with(|| display_atom.to_string());
                // A dep the `'parent_flip` off-arm collected into a
                // parent USE flip records no disclosure at all: real
                // drops the collected item from
                // `_unsatisfied_deps_for_display` (`depgraph.py:5820`
                // `remaining_items`), so neither the masked block, nor
                // the use-unsat block, nor the plain-miss block prints
                // for it -- the change block is its whole output.
                // (`nvc_dep_atoms` above is kept: `abort_outcome` still
                // needs the atom for the abort reason.)
                if !state.parent_flip_recorded.contains(&key) {
                    if let Some(masked) =
                        masked_candidates_for_atom(&ctx.repos, display_atom, config)
                    {
                        // Backlog #286: live supersedes built (see
                        // `live_miss_atom_supersedes`); the rows were just
                        // computed for the new atom, so a supersede
                        // refreshes them too.
                        match state
                            .masked_deps
                            .iter_mut()
                            .find(|r| r.category == key.0 && r.package == key.1)
                        {
                            Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                                r.atom = display_atom.to_string();
                                r.masked = masked;
                            }
                            Some(_) => {}
                            None => state.masked_deps.push(MaskedDepReport {
                                category: key.0.clone(),
                                package: key.1.clone(),
                                atom: display_atom.to_string(),
                                masked,
                                // Walked post-loop out of the final entries
                                // (see below).
                                chain: Vec::new(),
                            }),
                        }
                    }
                    // USE-unsatisfied-dependency disclosure (backlog #20,
                    // real `_show_unsatisfied_dep`'s separate "no ebuilds
                    // built with USE flags to satisfy" path): a dependency
                    // `NoVisibleCandidate` whose atom names version-matching
                    // ebuilds that are *visible* but cannot satisfy the
                    // atom's own `[use]` deps. Real's precedence puts this
                    // block before the masked one (`show_missing_use` wins),
                    // and the caller renders it first. Computed every pass;
                    // only the final pass's reports are rendered.
                    if let Some(rows) = use_unsat_candidates_for_atom(
                        &ctx.repos,
                        ctx.root,
                        &state.entries,
                        display_atom,
                        &current_atom,
                        owner.as_ref(),
                        config,
                    ) {
                        // Backlog #286: live supersedes built (see
                        // `live_miss_atom_supersedes`); the rows were just
                        // computed for the new atom, so a supersede
                        // refreshes them too.
                        match state
                            .use_unsat_deps
                            .iter_mut()
                            .find(|r| r.category == key.0 && r.package == key.1)
                        {
                            Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                                r.atom = display_atom.to_string();
                                r.rows = rows;
                            }
                            Some(_) => {}
                            None => state.use_unsat_deps.push(UseUnsatDepReport {
                                category: key.0.clone(),
                                package: key.1.clone(),
                                atom: display_atom.to_string(),
                                rows,
                                chain: Vec::new(),
                            }),
                        }
                    } else if !state
                        .masked_deps
                        .iter()
                        .any(|r: &MaskedDepReport| r.category == key.0 && r.package == key.1)
                        && !state
                            .use_unsat_deps
                            .iter()
                            .any(|r: &UseUnsatDepReport| r.category == key.0 && r.package == key.1)
                        && !state
                            .plain_miss_deps
                            .iter()
                            .any(|r: &PlainMissDepReport| r.category == key.0 && r.package == key.1)
                    {
                        // Neither sibling block claimed this miss: nothing
                        // matches by version at all, or no candidate's
                        // failure is a `Change USE:`/`Missing IUSE:` one
                        // (e.g. a profile-forced flag). Real's
                        // `emerge: there are no ebuilds to satisfy "<atom>"`
                        // block (#135 (d)). Backlog #286: live supersedes
                        // built (see `live_miss_atom_supersedes`).
                        match state
                            .plain_miss_deps
                            .iter_mut()
                            .find(|r| r.category == key.0 && r.package == key.1)
                        {
                            Some(r) if live_miss_atom_supersedes(&r.atom, display_atom) => {
                                r.atom = display_atom.to_string();
                            }
                            Some(_) => {}
                            None => state.plain_miss_deps.push(PlainMissDepReport {
                                category: key.0.clone(),
                                package: key.1.clone(),
                                atom: display_atom.to_string(),
                                chain: Vec::new(),
                            }),
                        }
                    }
                }
            }
            state.entries.push(GraphEntry {
                discovery: 0,
                category: key.0,
                package: key.1,
                outcome,
                blockers: Vec::new(),
                slot: None,
                sub_slot: None,
                repo_name: None,
                oldbest: Vec::new(),
                use_flags_display: Vec::new(),
                use_expand_display: Vec::new(),
                use_expand_display_p: Vec::new(),
                keyword_mask: None,
                new_slot: false,
                interactive: false,
                fetch_restrict: false,
                fetch_restrict_satisfied: false,
                download_files: Vec::new(),
                required_by: Vec::new(),
                source: CandidateSource::Ebuild,
                provenance: VisibilityProvenance::default(),
                keyword_suggestion,
                use_suggestion,
                parent_use_suggestion,
                // Track X Slice B (#242): the node carries its resolving
                // root -- real `Package.root_config` / `pkg.root`.
                targets_running_root: item_targets_running_root,
                remote_binary: false,
                build_id: None,
                deps: already_installed_deps,
            });
            continue;
        };

        // The resolved version may have come from any of `repos`, or
        // from PKGDIR (`--usepkg`/`--usepkgonly`) or a remote binhost
        // (`--getbinpkg`), so re-derive which one it actually lives in
        // -- reusing `list_candidates`/`list_binary_candidates` rather
        // than threading a repo location back out of `PretendOutcome`,
        // since more than one source could carry the identical
        // version. This step MUST agree with what `resolve_pretend`
        // itself chose: real `_wrapped_select_pkg_highest_available_
        // imp` returns `matched_packages[-1]` with `ebuild` built
        // first in `dbs` order -- "ebuild type is the last resort"
        // (`depgraph.py:8492`) -- so at a version tie a
        // `--usepkg`/`--getbinpkg` binary that passed
        // `binpkg-respect-use` beats the ebuild. Mirrors
        // `resolve_pretend`'s own pool construction (`--usepkgonly`
        // excludes ebuilds entirely).
        let mut repo_candidates = if ctx.usepkgonly {
            Vec::new()
        } else {
            let Ok(c) = list_candidates(&ctx.repos, &key.0, &key.1) else {
                continue;
            };
            c.to_vec()
        };
        if ctx.usepkg || ctx.usepkgonly {
            let mut binary_candidates = list_binary_candidates(&ctx.local_binpkg, &key.0, &key.1);
            if ctx.getbinpkg {
                binary_candidates.extend(list_remote_binary_candidates(
                    &config.binrepos,
                    ctx.root,
                    &ctx.local_binpkg,
                    &key.0,
                    &key.1,
                ));
            }
            // Same `--binpkg-respect-use` / `--binpkg-changed-deps`
            // gates `resolve_pretend` applied when it chose this
            // version -- a binary that was never eligible then can't
            // be re-derived as the source here either.
            if ctx.binpkg_respect_use
                || ctx.newuse
                || ctx.changed_use
                || !ctx.reinstall_atoms.is_empty()
            {
                let ebuild_at_version = |version: &str| {
                    repo_candidates
                        .iter()
                        .find(|e| e.source == CandidateSource::Ebuild && e.version == version)
                };
                binary_candidates.retain(|c| {
                    binpkg_respect_use_ok(
                        c,
                        ebuild_at_version(&c.version),
                        &key.0,
                        &key.1,
                        config,
                        ctx.newuse,
                        ctx.changed_use,
                        ctx.binpkg_respect_use,
                    )
                });
            }
            if binpkg_changed_deps_active(ctx.usepkgonly) {
                binary_candidates.retain(|c| {
                    !binary_deps_changed(ctx.root, &ctx.repos, c, &key.0, &key.1, ctx.with_bdeps)
                });
            }
            // `_equiv_ebuild_visible` (see `resolve_pretend`): a
            // binary re-derived here needs a visible ebuild at its
            // own exact version, when any visible ebuild satisfies
            // the atom. `--use-ebuild-visibility` enforces it even
            // under `--usepkgonly` / a `--useoldpkg-atoms` match.
            let uev = use_ebuild_visibility();
            if (!ctx.usepkgonly || uev) && !binary_candidates.is_empty() {
                let some_ebuild_matches = repo_candidates.iter().any(|c| {
                    c.source == CandidateSource::Ebuild
                        && is_visible(c, &key.0, &key.1, config)
                        && {
                            let s = format!(
                                "{}/{}-{}:{}/{}::{}",
                                key.0, key.1, c.version, c.slot, c.sub_slot, c.repo_name
                            );
                            portage_dep::match_from_list(&current_atom, &[s.as_str()])
                                .is_some_and(|r| !r.is_empty())
                        }
                });
                if some_ebuild_matches || uev {
                    binary_candidates.retain(|c| {
                        (!uev && useoldpkg_atom_matches(&key.0, &key.1, &c.version))
                            || repo_candidates.iter().any(|e| {
                                e.source == CandidateSource::Ebuild
                                    && e.version == c.version
                                    && is_visible(e, &key.0, &key.1, config)
                            })
                    });
                }
            }
            let binary_candidates = dedup_binary_instances(binary_candidates, &atom, config);
            // #212 (v2 `#24c`): mirror of `resolve_pretend`'s own
            // `slot_operator_mask_built` pool filter -- a masked binary
            // must not be re-derived as the entry source here either
            // (this step MUST agree with what `resolve_pretend`
            // chose). Plain (`!=cpv`) negatives read as `None` and are
            // ignored; only the `[binary]`-marked form filters.
            let masked_here = bp.masked_negatives_for(&key);
            let binary_candidates: Vec<Candidate> = binary_candidates
                .into_iter()
                .filter(|c| {
                    !masked_here.iter().any(|neg| {
                        slot_operator_binary_masked_version(neg, &key.0, &key.1)
                            .is_some_and(|ver| ver == c.version)
                    })
                })
                .collect();
            repo_candidates.extend(filter_usepkg_exclude_include(
                binary_candidates,
                &key.0,
                &key.1,
                ctx.usepkg_exclude,
                ctx.usepkg_include,
            ));
        }
        let Some(resolved) = repo_candidates
            .iter()
            .filter(|c| c.version == *version)
            .max_by(|a, b| {
                // "ebuild type is the last resort" -- see
                // `resolve_pretend`'s own `best` pick.
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

        // Track X Slice B (#242): node identity includes the resolving
        // root -- a running-root atom never reuses a target-root slot
        // (this is the cross-root in-graph boundary: real's per-root
        // graph db). Same-root runs key uniformly `false`: identical.
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
        // #57, direction 2 (the mirror of the `AlreadyInstalled` check
        // above, for the opposite argv order): this slot is free of
        // merge-bound instances but an `AlreadyInstalled` node already
        // holds it, and this atom rejects that node's version. Real's
        // tracker holds both and reports the collision either way round
        // (S0 cells a/b are the same block); portuale's slot check only
        // ever looked at `resolved_slots`, which indexes merge outcomes
        // only. Falls through -- the merge is still graphed, exactly as
        // real merges `paired-2.0` while reporting the block.
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
                    build_slot_conflict(
                        &ctx.repos,
                        config,
                        ctx.root,
                        &key.0,
                        &key.1,
                        &slot,
                        &installed_version,
                        &current_atom,
                        &version,
                        &state.slot_pullers,
                        Some(InstalledSide::Existing),
                    ),
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
            let Some(metadata) = read_binary_metadata_any_instance(
                config,
                ctx.root,
                &ctx.local_binpkg,
                &key.0,
                &key.1,
                &version,
                candidate_build_id.as_deref(),
            ) else {
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

        // `--nodeps`: real create_depgraph_params.py pops "recurse" from
        // myparams, and depgraph.py's own dependency-walk returns early
        // when "recurse" isn't in myparams -- ported here as skipping
        // this package's own DEPEND/RDEPEND/etc entirely, so nothing of
        // its is ever parsed, flattened, or queued. This also means no
        // blockers are ever collected from a `--nodeps` run (blockers
        // only ever come from a dependency string, which is never read),
        // matching real portage exactly.
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

    // `.get()`, not `.remove()`: `entries` can hold more than one entry
    // for the same `(category, package)` -- one per resolved slot (see
    // this function's own doc comment on multi-slot support) -- and every
    // one of them was pulled in by the same owner(s), so every one needs
    // the same `required_by`. A destructive `.remove()` here handed the
    // owners to whichever slot's entry the loop reached first and left
    // the rest with an empty `required_by`, which `--tree` then dropped
    // to its flush-left safety net instead of nesting under the parent
    // (and `--json` reported as `"required_by": []`). Entries with no key
    // in the map keep whatever `required_by` they were built with -- a
    // `--root-deps` running-root build entry sets its own immediate
    // requester at construction and must not be wiped to empty here.
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

    // An installed package that is *also* being merged in the same
    // slot is one node in real's graph, not two. The BFS can build an
    // `AlreadyInstalled` entry for a `cat/pkg` before some later atom
    // forces a same-slot merge of it -- `net-libs/nghttp2`'s
    // `>=sys-apps/systemd-209` resolves to the installed systemd, then
    // `sys-auth/polkit`'s `sys-apps/systemd:0=[policykit]` forces the
    // reinstall. Drop the redundant `AlreadyInstalled` entry; the
    // merge-bound one supersedes it (and already carries the union of
    // both owners' `required_by`, filled just above).
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

    // Backlog #90 (S2): the direct solve runs on the walked graph
    // before the blocker phases -- real `_process_slot_conflicts`
    // solves first, then `_validate_blockers` (`depgraph.py:2116-2125`).
    // #142 S2 F1: solve one joint record per (category, package,
    // slot) — `merge_same_slot_conflicts` folds the per-atom records
    // first, so the solver keeps the jointly-satisfying version.
    // Gated on recorded conflicts (the common conflict-free pass pays
    // nothing). Removal drops merge entries and dissolves
    // single-survivor conflicts; the skip rows ride `skipped_updates`
    // to the renderer (rc stays 0: no `SlotConflict` survives a
    // removal, and untouched conflicts keep their #62 rc 1).
    state.slot_conflicts = merge_same_slot_conflicts(
        &state.slot_conflicts,
        &state.slot_pullers,
        &ctx.repos,
        config,
        ctx.root,
        // #270: a record is built when its first mismatching atom is
        // seen, so a parent walked later (a sibling that pulls the same
        // cp after the detection) is missing from a lone record's
        // parents, and the direct solve would remove an instance real's
        // solver keeps forced (`mlocaml`/`mllabltk`: real reports the
        // unsolvable conflict, `depgraph.py:1858-1870`). Real's solver
        // reads the final `_parent_atoms`; at `--backtrack=0`, where the
        // exemption for built slot-operator triggers no longer hides
        // such records (see the walk's conflict arm), re-file the lone
        // records from the live pullers like the merged ones.
        ctx.backtrack_max == 0,
    );
    if !state.slot_conflicts.is_empty() {
        let solved = direct_solve_slot_conflicts(DirectSolveInput {
            conflicts: &state.slot_conflicts,
            entries: &state.entries,
            top_level: &ctx.top_level_cps,
            excluded: ctx.excluded,
            selective: ctx.selective,
            replace_cps: &bp.slot_operator_replace_installed,
            root: ctx.root,
            repos: &ctx.repos,
            config,
        });
        if !solved.removed.is_empty() {
            // The kept instance takes over the removed instance's
            // merge row (real merges it; portuale holds one row per
            // cp). A removal with no keeper deletes the row outright
            // (degenerate all-unforced shape -- no notice was recorded
            // for it either). Repointing keeps `required_by`, merge
            // order and USE wiring stable; the removed version's own
            // walked deps stay attached (all S2 oracle removals are
            // leaves -- a non-leaf removal keeps stale deps, filed in
            // the fn docs).
            for (c, p, r, k) in &solved.removed {
                let at = state.entries.iter().position(|e| {
                    e.category == *c
                        && e.package == *p
                        && merge_bound_version(&e.outcome) == Some(r)
                });
                let Some(idx) = at else {
                    continue;
                };
                if k.is_empty() {
                    state.entries.remove(idx);
                    continue;
                }
                // Repoint the row at the kept version. An installed
                // keeper becomes AlreadyInstalled (no row, like real's
                // kept installed node); a merge keeper stays New. Slot
                // data comes from the vdb for an installed keeper,
                // from the tree metadata otherwise.
                let vdb_hit = installed_candidates(ctx.root, c, p)
                    .into_iter()
                    .find(|(v, _, _)| v == k);
                let (slot, sub_slot, repo) = match &vdb_hit {
                    Some((_, s, ss)) => {
                        let repo = installed_pkg_repo(ctx.root, c, p, k);
                        (s.clone(), ss.clone(), repo)
                    }
                    None => {
                        let (sub, repo, slot) = slot_conflict_meta(&ctx.repos, c, p, k);
                        (slot, sub, repo)
                    }
                };
                let entry = &mut state.entries[idx];
                entry.slot = Some(slot);
                entry.sub_slot = Some(sub_slot);
                entry.repo_name = Some(repo);
                // Per-flag `(flag, enabled)` pairs (the entry shape),
                // not the `(VAR, body)` display pairs: vdb record for
                // an installed keeper, tree candidate + effective USE
                // for a merge keeper.
                if vdb_hit.is_some() {
                    let (iuse, use_flags) = installed_pkg_iuse_and_use(ctx.root, c, p, k);
                    entry.use_flags_display = iuse
                        .into_iter()
                        .map(|f| {
                            let on = use_flags.contains(&f);
                            (f, on)
                        })
                        .collect();
                    entry.outcome = PretendOutcome::AlreadyInstalled { version: k.clone() };
                } else {
                    let (declared, enabled) = slot_conflict_flag_sets(&ctx.repos, config, c, p, k);
                    entry.use_flags_display = declared
                        .into_iter()
                        .map(|f| {
                            let on = enabled.contains(&f);
                            (f, on)
                        })
                        .collect();
                    entry.outcome = PretendOutcome::New { version: k.clone() };
                }
            }
            state.slot_conflicts = solved.surviving;
            state.skipped_updates.extend(solved.skipped);
        }
        // #142 S2 F3: a repointed row's keeper version was never walked
        // (losing currents get no entry and no dep flatten), so its own
        // blockers are not pending — but real's kept graph node carries
        // them into `_validate_blockers`. Flatten the keeper's ebuild
        // dep strings for blocker tokens here, before `resolve_blockers`
        // runs below; the F2 owner-version gate then files exactly the
        // keeper's blockers and drops the losers'. Only blockers are
        // collected (a keeper merge dep would need a second walk — the
        // documented non-leaf staleness in `direct_solve_slot_conflicts`'
        // repoint arm stands). Installed keepers are exempt: their
        // blockers come from the vdb scan when unwalked.
        for (c, p, _removed, k) in &solved.removed {
            if installed_candidates(ctx.root, c, p)
                .into_iter()
                .any(|(v, _, _)| v == *k)
            {
                continue;
            }
            let Some(keeper_cand) = list_candidates(&ctx.repos, c, p).ok().and_then(|cs| {
                cs.iter()
                    .filter(|cand| cand.version == *k && cand.source == CandidateSource::Ebuild)
                    .max_by_key(|cand| cand.repo_priority)
                    .cloned()
            }) else {
                continue;
            };
            let pf = format!("{p}-{k}");
            let Ok(meta) = repo_aux_metadata(&keeper_cand.repo_location, c, &pf) else {
                continue;
            };
            let (_declared, enabled) = slot_conflict_flag_sets(&ctx.repos, config, c, p, k);
            let owner_key = (c.clone(), p.clone());
            for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
                let Some(dep_str) = meta.get(dep_key) else {
                    continue;
                };
                let toks: Vec<String> = dep_str.split_whitespace().map(String::from).collect();
                let Ok(flat) = portage_use_reduce::use_reduce_flat(
                    &toks,
                    &enabled,
                    portage_use_reduce::MatchMode::Normal,
                ) else {
                    continue;
                };
                for tok in flat {
                    if tok == "||" {
                        continue;
                    }
                    if let Some(dep_atom) = portage_dep::parse_atom(&tok)
                        && dep_atom.blocker != portage_dep::Blocker::None
                    {
                        let duplicate = state.pending_blockers.iter().any(|pb| {
                            pb.owner_key == owner_key
                                && pb.owner_version == *k
                                && pb.atom_str == tok
                        });
                        if !duplicate {
                            state.pending_blockers.push(PendingBlocker {
                                atom_str: tok,
                                strong: dep_atom.blocker == portage_dep::Blocker::Strong,
                                buildtime: dep_key == "DEPEND" || dep_key == "BDEPEND",
                                target_category: dep_atom.category,
                                target_package: dep_atom.package,
                                owner_key: owner_key.clone(),
                                owner_version: k.clone(),
                                owner_merging: true,
                                owner_installed: false,
                            });
                        }
                    }
                }
            }
        }
    }

    // Backlog #148: rows for the shadowed second instances of
    // surviving slot conflicts (see the fn docs -- display-complete,
    // walk-light). After the solve/removal above, so solved cells are
    // untouched; before the installed-blocker collection below, so the
    // new merge nodes participate in it as merge-side nodes.
    synthesize_surviving_conflict_entries(
        &mut state.entries,
        &state.slot_conflicts,
        &state.required_by_map,
        &ctx.repos,
        ctx.root,
        config,
    );

    // #77 A1: real's all-installed-packages blocker collection runs after
    // the graph is complete and before `_validate_blockers` classifies
    // (`depgraph.py:8919-9078`), and `--nodeps` is its only early exit
    // (`:8908-8910`).
    if !ctx.nodeps {
        collect_unwalked_installed_blockers(
            &ctx.repos,
            ctx.root,
            config,
            ctx.dynamic_deps,
            ctx.ignore_built_slot_operator_deps,
            &union_constraints,
            ctx.root_deps_running_root,
            &state.entries,
            &mut state.pending_blockers,
            &mut state.installed_meta_memo,
        );
    }

    let conflicts = resolve_blockers_with_seeds(
        ctx.root,
        &state.pending_blockers,
        &state.entries,
        &ctx.blocker_retry_closure,
        ctx.buildpkgonly,
        &ctx.config.blocker_retry_seed_atoms,
    );
    // Backlog #80: unresolved rows whose owner has no display entry
    // ride out on the result instead of being dropped.
    let mut orphan_blockers: Vec<OrphanBlocker> = Vec::new();
    file_blocker_conflicts(
        &mut state.entries,
        ctx.root,
        conflicts,
        &mut orphan_blockers,
    );

    // Backlog #81: tree-mode solved verdicts. Only Replacement rows can
    // be solved by a scheduled uninstall, and the simulation is gated on
    // at least one existing (it returns immediately otherwise), so the
    // common no-blocker resolve pays nothing for the second scheduling
    // pass. Flags ride the conflicts into `GraphResult`; `--tree`
    // display reads them, everything else ignores them.
    let needs_tree_sim = state.entries.iter().any(|e| {
        e.blockers.iter().any(|b| {
            !b.unsolvable && matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. }))
        })
    });
    if needs_tree_sim {
        for (owner, blocker) in merge_order::tree_solved_replacements(
            &state.entries,
            ctx.atoms,
            ctx.config,
            ctx.root,
            ctx.implicit_system_deps,
            &ctx.repos,
            ctx.dynamic_deps,
            &bp.circular_dependency,
        ) {
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
        // Backlog #90 (S1): the walk resolves last-declared-first
        // (real's stack), but real's `WARNING: … package.provided:`
        // block lists the requested atoms in requested order (staged
        // oracle: providedpkg before providedpkg2) -- restore argv
        // order here; atoms absent from argv (shouldn't happen:
        // only depth-0 pushes record) sort last, stably.
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
