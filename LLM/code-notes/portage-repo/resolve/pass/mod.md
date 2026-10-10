# crates/portage-repo/src/resolve/pass/mod.rs
One resolver pass (BFS walk) over the dependency graph.

## fn run_pass
- Phase A3 (backlog #023): `PassState` holds `use_overlay` / `use_change_overlay` / `use_broke`, read back overlay-then-`bp` within the pass.
- The same effective-config view the loop feeds `collect_feedback` + driver / `assemble_result` (cloned out of `bp`; only `&bp` is borrowed in here, so no `config_owned` dance is needed).
- `union_constraints` is pass-constant (`bp` is never mutated mid-walk, only the overlay): C1 readers see positives + rendered negatives as one bucket.
- First-pass `--debug` prints each argument as `Arg:` / `Atom:` lines via `resolver_trace::tr!`.

## fn run_pass::seed order
- Backlog #90 (S1): Portage feeds same-level atoms through its `_dep_stack`, so the last declared resolves first; selection is order-sensitive since an already-graphed satisfier is reused (`_select_package:8370`) -- forward order graphed `2.0` for a bare dep then recorded a conflict Portage never sees (the `slotconflictparent` bt0 silence). `top_level_cps` still comes from `req.atoms` in declaration order, untouched.

## fn run_pass::auto-replace seeds
- Backlog #24 (S3): Portage `_gen_reinstall_sets` (5457-5480) turns `_slot_operator_replace_installed` into the pseudo `SetArg` `@__auto_slot_operator_replace_installed__`, appended after the user's own args (`select_files` 5430). `owner: None` mirrors the set arg having no package parent. `depth: 1` is Portage's `reset_depth=False`: the node must not interact with `--deep=N` depth accounting, and -- portuale-side `depth == 0` means top-level argument everywhere (`resolve_pretend`'s `toplevel`, the `(Argument)` slot-puller line). `top_level`/`top_level_cps` come from `req.atoms`, untouched by construction. `_eliminate_rebuilds` rule 5 excludes the auto sets from its "the user asked for it" test (3859-4000). The set is walked as a root node, which is why the rebuilt consumer gets its deps re-walked at all.
- Backlog #236 (A): the probe's upgraded child rides the same auto set (Portage `abi_reinstalls.add((child.root, child slot atom))`), seeded at the exact candidate the probe chose.

## fn run_pass::seed blocks
- Backlog #298: Portage `_create_graph` (`depgraph.py:3254-3271`) adds every argument's package first, then pops LIFO. The interleaving showed as `slotconflictnewconsumer slotconflictunsolvable` selecting the bare dep's 2.0 before the pin pair's 1.0; Portage adds 1.0 first.

## fn run_pass::package.provided
- Portage `dep_check.py:1052` (dependency) and `depgraph.py:5497-5615` (top-level target); Portage removes the dep from the deplist.

## fn run_pass::resolving root
- Track X Slice B (backlog #242): this atom's resolving root -- Portage `_dep_expand`'s `root_config` (`depgraph.py:4928`). The flag implies `cross_root().is_some()` (tags are only ever set when the roots differ); same-root runs always resolve here.

## fn run_pass::running cycle cut
- The soft running edges are soft via the running-`BDEPEND`-soft rule in `enqueue_flat_deps` (e.g. the `rdrcyca <-> rdrcycb` bootstrap cycle); cut mirrors the old sidecar `root_deps_build_seen` guard (`resolve_root_deps_build_entries`: "the second edge back into an in-progress package just yields no further entry", losing one `required_by` edge by design). The hard running edge back is the `cyc0z-3`/`cyc0y` cross-root `DEPEND` cycle (the #207 flip) and still records its edge so the cycle reports; a same-root soft revisit (a `RDEPEND` diamond's second owner) still records too (its `--json` `required_by` lists both).

## fn run_pass::edge kinds
- Track X Slice C (#242): slot/installed dedup is root-qualified since Slice B; top-level atoms are target-rooted like their flag.
- Portage `DepPriority.satisfied` (`_emerge/DepPriority.py`): a build-time dep already provided by an installed package gets a *satisfied* priority, which `_serialize_tasks`' `DepPrioritySatisfiedRange` can ignore when breaking a cycle, so it is not an unbreakable edge. Only a genuinely unsatisfied build-time dep counts as `has_hard`, matching this map's own "unsatisfied build-time dep" contract. Under `--emptytree` both ends of the `app-arch/xz-utils` <-> `app-portage/elt-patches` build-time cycle are still installed, so Portage orders them by installed versions rather than reporting a circular dependency.
- Backlog #250: Portage's satisfaction test is `vardb.match_pkgs(atom)`, which honours the atom's use-deps against the installed instance's recorded USE -- an installed `foo` built `-bar` does not satisfy `foo[bar]`, so the edge stays unbreakable; the version/slot-only lookup used to let a USE-mismatched instance hide a genuine build-time cycle. Track X Slice B (#242): satisfaction is tested against this atom's own resolving root (Portage `DepPriority.satisfied` reads that root's vdb).

## fn run_pass::revisit record
- Backlog #277: Portage narrates `Child:`/`Parent Dep:` at every `_add_pkg` call; the single-resolution invariant means same text, same child. `parent_atoms` feeds only the trace dump.

## fn run_pass::debug candidates
- `emerge --pretend --debug` Stage 3: Portage `_wrapped_select_pkg_highest_available_imp`'s candidate list, every candidate that matched this atom, first pass only. Format is Portage's own `f"{type_name + ':':>10} {cpv}::{repo}\n"` (`depgraph.py:8347`), in `dbs` order (ebuild, then binary, then installed), to stderr.

## fn run_pass::pullers and constraints
- Backtracking: this atom is recorded as one of the constraints pulling `cat/pkg` (Portage `_select_pkg_highest_available` sees the whole atom set for a package, not just the first); slice 4 renders the `(Argument)` line via `slot_collision_handler`.

## fn run_pass::complete mode
- Portage `_complete_graph`'s `_select_pkg_from_graph` (`depgraph.py:8495`): in complete mode the deep re-walk of the required sets may only pick a package already in the graph or installed -- never resolve/merge anything new. For a dependency atom whose `cp` isn't a genuine phase-1 merge target, that means an installed version that satisfies it -> `AlreadyInstalled` (deep-walk its deps, merge nothing); nothing installed -> Portage's `_initially_unsatisfied_deps`, no graph node -> drop. Done before `resolve_pretend`, not after; top-level atoms always go through `resolve_pretend` (their `NoVisibleCandidate` is fatal, handled below).
- Portage `_initially_unsatisfied_deps` for the nothing-installed drop. Resolving every installed package fully made `emerge -pt gnome-control-center` take minutes; only the CLI layer's complete pass populates `complete_locked_merges`.
- `resolve_atom` is shared with the #236 (A) forced-upgrade flip (same pass-constant inputs, another atom); the minimize group loop passes each sibling's own flag while the single resolve uses the popped item's.

## fn run_pass::minimize group
- Backlog #236 (B): Portage `_minimize_children` (`_emerge/depgraph.py:4751-4854`) is called on every parent's selected atom set (`:4521`/`:4660`). Portage selects a package for each of the parent's atoms first, then -- per cp with two or more distinct selections -- eliminates the redundant ones, installed first and ascending, so an atom whose own pick is also satisfied by another selection is re-pointed at that one. Portuale resolves each queued atom on its own, so the collapse point is built here: the first sibling popped resolves the whole group and pins every member's outcome. `||`-chosen atoms stay out, like Portage's per-disjunction call. Dominant shape is an installed parent's dynamic deps (`FakeVartree._apply_dynamic_deps`): the live `cat/pkg:=` picks the tree's highest while the appended built `>=cat/pkg-1.0:0/1=` picks the installed one, and Portage binds both to the installed instance (the #211 update probe decides upgrades afterwards). The owner's plain siblings are contiguous at the queue front because one `enqueue_*` call pushed them together.

## fn run_pass::arg-pinned reuse
- Backlog #285: Portage `find_existing_node` (`depgraph.py:7854/8180-8215/8370-8371`).

## fn run_pass::installed fallback (#233)
- Portage `_iter_match_pkgs(root_config, "installed", atom)` (`depgraph.py:7799-7840`). The stale built slot-operator atom on an installed world member (`dev-libs/reinstslottarget:0/1=`, target moved `0/1 -> 0/2` without a revbump) still matches in vartree: satisfied nomerge edge, Portage merges the unrelated `@world` updates and withholds the reinstall silently (rc 0; S0 probe `probe.log`). Without the fallback the walk aborted (rc 1) before the #210 reverse-dependency scan engaged.
- Scoping: a pure `DEPEND`/`BDEPEND` edge targets the running root in Portage (EAPI-7+ `dep_root`), unconsultable outside the `--root-deps` feed-in, so its long-pinned "reported, not satisfied from the target vdb" behaviour stands (`rootdepsprovider` unit + contract pins). `--emptytree` excluded like `resolve_pretend`'s own fallback (Portage selects no installed candidates). The USE filter is Portage `vardb.match` semantics, shared with `dependency_avoid_update_candidate` and the `||` installed classification; an evaluated `[-flip]` "matching" the installed `+flip` child wrongly dropped the dep as satisfied instead of surfacing unsatisfied (`deep_walk_*_installed_parents` arm A).
- `||` arm: Portage `dep_check.py` soft 715-724; the `opartlya` pin. Slot-operator atoms (`:slot`, `:slot/sub=`, `:=`, `:*`) name the built slot, so with no ebuild carrying it the installed instance is the only satisfaction and no competing ebuild candidate needs pass-mask protection. Bare/version-range NVCs keep the old path (the needer/othermod triangle keeps its restart count; `oldmovepkg` keeps reporting).
- Backtrack `extra_constraints` not consulted: the `!=cpv` negatives are portuale's own transient conflict-before-scan artifacts (masking the instance the #210 assembly scan is about to withhold to), which Portage's integrated scan never creates.

## fn run_pass::running fallback
- Track X Slice B fix (#242): a running-root `BDEPEND`/`IDEPEND` (and `DEPEND`) no ebuild satisfies but the running root has installed settles `AlreadyInstalled`, never `NoVisibleCandidate` -- Portage `_add_pkg_deps` resolves those groups against the running root's installed db (`depgraph.py:4218-4291`). This loosens both gates the #233 fallback keeps for target-rooted atoms: `buildtime_hard` (a `BDEPEND` is always hard) and the slot-operator requirement (a bare `BDEPEND` like `rootdepsprovider` carries no slot). Scoped to running atoms with an installed match in their own resolving root.
- Cross-root build without `--root-deps`: `rootdepspkg`'s `BDEPEND` on `rootdepsprovider` (installed in the running root, no ebuild anywhere) drops silently. The `cyc0z-3`/`cyc0y` fixture-only CPs are never in the host vdb (the #207 flip's determinism basis), nor are `rdrtool*`/`rdrcyc*` in `/`, so that flip and the recursion pins are untouched.

## fn run_pass::reinstall flips
- Keyed on `cat/pkg`, not the seeded atom: whichever visit comes first -- seed or ordinary edge -- flips; patching the entry afterwards would leave its deps unenqueued.
- `--reinstall-atoms`: a matching already-installed package is forced to re-merge (Portage `depgraph.py` drops it from every `inst_pkgs` list); in portuale's model that is the `--emptytree` rewrite, scoped to the matched atom.
- Backlog #24 (S3): the same flip point carries Portage's `__auto_slot_operator_replace_installed__` `force_reinstall=True` (`_gen_reinstall_sets` 5457-5480).
- Backlog #236 (A): an installed child the update probe forced (see `BacktrackParams::slot_operator_forced_upgrade`) is never kept: whichever visit reaches it first selects the probe's candidate, like Portage's `force_reinstall` excluding the installed instance.

## fn run_pass::parent record
- Backlog #59 (S1): Portage `_add_pkg`'s per-instance parent-atom record (`_add_parent_atom`, `depgraph.py:3583-3604`), taken here -- after the reinstall/autounmask outcome flips, before the `other_outcomes` dedup below -- so the child instance is the one this atom actually resolved to. An `AlreadyInstalled` instance shadowed later by a same-slot merge keeps its own row: `paired-1.0` keeps its own row under othermod's `<paired-2.0`. A removal never resolves a dependency atom (backlog #72 B3, kept in code).

## fn run_pass::parent-use flip call
- Portage `--autounmask-use` PART B (`_apply_parent_use_changes` -> `_show_unsatisfied_dep(collect_use_changes=True)`, `depgraph.py:5820`/`6768`): a dependency's use-dep was originally conditional on the *requesting parent's* own USE, and no candidate satisfies the evaluated form. Details: `opt?`/`opt=` forms; `suggested_parent_use_candidate` into `_needed_use_config_changes`; the graph already failed so `_success_without_autounmask` fails the resolve with no merge rows (plus the "terminated early" notice with `--autounmask-backtrack` off). With `--autounmask-backtrack=y` (Slice 4) the flip is folded into `autounmask_use_config` (keyed `(parent_cat, parent_pkg)` -- exactly a `package.use` entry) and the driver's `autounmask_grew` restart re-walks everything instead. A probe re-resolution of just the freed atom still gates both arms, so only a flip that actually helps is recorded or folded. `--autounmask-use=n` suppresses it via the shared `autounmask_suggest_use` gate.

## fn run_pass::changed-deps report
- Portage's own `_changed_deps_pkgs` dict is discarded unread by `_changed_deps_report`'s early return. `deps_changed` is a vdb-vs-current-ebuild comparison for one installed version; Portage's `newuse`/`changed_use`/`changed_slot` triggers stay freely combinable.

## fn run_pass::version tie
- Portage `matched_packages[-1]` with ebuild built first (`depgraph.py:8492`).

## fn run_pass::installed-side conflict
- Backlog #57, direction 2: Portage's tracker reports the collision either way round (S0 cells a/b are the same block); portuale's slot check only looked at `resolved_slots` (merge outcomes only). Portage merges `paired-2.0` while reporting the block.

## fn run_pass::post-loop required_by
- Multi-slot support: several entries per `(category, package)`; `--json` reported `"required_by": []` for the starved ones. A `--root-deps` running-root build entry sets its own immediate requester at construction.

## fn run_pass::merge-merge dedup
- The BFS can build `AlreadyInstalled` before a later atom forces a same-slot merge (`net-libs/nghttp2`'s `>=sys-apps/systemd-209` resolving to installed systemd, then `sys-auth/polkit`'s `sys-apps/systemd:0=[policykit]` forcing the reinstall).

## fn run_pass::direct solve
- Backlog #90 (S2): Portage `_process_slot_conflicts` solves first, then `_validate_blockers` (`depgraph.py:2116-2125`). The common conflict-free pass pays nothing; rc stays 0 when no `SlotConflict` survives a removal, untouched conflicts keep their #62 rc 1.
- Backlog #270: a lone record misses parents walked later (a sibling pulling the same cp after detection), and the direct solve would remove an instance Portage keeps forced (`mlocaml`/`mllabltk`: Portage reports the unsolvable conflict, `depgraph.py:1858-1870`). Portage's solver reads the final `_parent_atoms`; at `--backtrack=0`, where the built slot-operator-trigger exemption no longer hides such records (see the walk's conflict arm), the lone records are re-filed from the live pullers like the merged ones.

## fn run_pass::surviving entries
- Backlog #80: unresolved rows whose owner has no display entry ride out on the result (`orphan_blockers`).
- Backlogs #90 (S2), #142 (S2 F1): one joint record per slot, so the solver keeps the jointly-satisfying version; removals ride `skipped_updates` to the renderer.
- Backlog #148: display-complete, walk-light rows.

## fn run_pass::blockers tail
- Backlog #77 (A1): Portage `depgraph.py:8919-9078`, early exit at `:8908-8910`.
- Backlog #81: everything but `--tree` display ignores the flags.

## fn run_pass::pprovided order
- Backlog #90 (S1): the `WARNING: … package.provided:` block lists requested atoms in requested order (not walk order); atoms absent from argv sort last, stably. Staged oracle: `providedpkg` before `providedpkg2`. Only depth-0 pushes record, so absent-from-argv shouldn't happen.
