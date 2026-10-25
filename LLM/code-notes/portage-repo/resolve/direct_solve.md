# crates/portage-repo/src/resolve/direct_solve.rs
Solves slot conflicts directly, without backtracking.

## struct DirectSolveInput
- Backlog #90 (S2): Portage `_solve_non_slot_operator_slot_conflicts` (`depgraph.py:1774-2106`), run unconditionally by `_process_slot_conflicts` (`:2108-2116`) -- no backtracking gate (the bt0 capture says `backtrack: 0/0`, zero retries, yet reconciled). Portuale used to reconcile solvable shapes only through its backtrack retry loop, so at `--backtrack=0` it recorded the conflict (notice + rc 1 since #62) where Portage is silent.
- A conflict package with no non-conflict parent edge is unforced and removed (`_remove_pkg`), its parents re-walked (`_dep_stack` + `_create_graph`), and the removed instance reported via `_conflict_missed_update["slot conflict"]`. Portuale mirrors the verdict computation (forced/non-forced sets, `or_tuple` first-pulled, `non_matching_forced` protection, the `is_arg_parent` installed exclusion, the `_slot_operator_replace_installed` skip) over its own recorded `SlotConflict`s; removal drops the merge entries and prunes single-instance survivors, and the re-walk is folded into the settle (the walk already resolved every remaining entry -- S2 cells carry no blockers on removed instances, so no `_validate_blockers` rerun is owed).
- Deliberate narrowings, all documented at the use sites: indirect conflict candidates (children reachable only through conflict packages, `:1808-1829`) are not modelled -- every S2 oracle cell holds leaf instances, so the set is empty there. No orphan cascade (`_remove_pkg`'s parentless removal): removed S2 instances are leaves; lingering orphans, if any, stay listed. Removal runs after the blocker filing (Portage validates blockers after solving): blocker rows anchored to a removed entry are out of scope -- no S2 cell has them.

## struct DirectSolveOutput
- Merge instances to drop, each with the surviving same-slot version that takes over its merge row (`(category, package, removed_version, kept_version)`). The row is repointed rather than deleted: Portage merges the kept instance (its row stays), while the removed one vanishes -- and portuale holds one row per cp anyway. Conflicts with >= 2 surviving instances (the rest dissolve with the removal -- their story is the skip notice, not a block).

## fn direct_solve_atom_matches
- Portage `(parent, atom)` matching (`atom.match(pkg.with_use(...))`, `:1921-1930`) with portuale's display-grade metadata: the structural probe carries the tree slot/sub-slot (the same `slot_conflict_meta` convention the record path uses), and `[use]` deps are checked against the instance's own resolved flags (merge entry display, else tree-effective, else vdb -- see `direct_solve_instance_use`). `for_skipped` selects Portage's plain-match form used for the missed-update recording (`:2083-2094`, no `is_arg_parent` gate there).
- A bare `cat/pkg:=` matches its own conflict's every instance: Portage matches the raw atom structurally (`atom.match(...)` has no built-binding step -- the `:=` only narrows by slot linkage), while portuale's list matcher does not implement the built operator at all (it would match nothing and wrongly dissolve the need_rebuild-trailer conflict). Slot and cp always agree here (edges are per-conflict); version/USE never discriminate a same-slot `:=` pull.
- #270: a *versioned* bare `:=` atom (`>=cat/pkg-4.02:=`) still discriminates by version in Portage (`atom.match` applies the version; the `:=` only narrows by slot linkage, which this per-conflict edge already shares), so drop the operator and match the rest. Without this `mllabltk`'s `>=mlocaml-4.02:=` accepted the installed `4.01.0` and never forced the `4.02.1` instance.

## fn direct_solve_instance_use
- Portage `pkg.with_use(self._pkg_use_enabled(pkg))`: a merge instance with an entry reads its resolved display flags, a merge instance without one (shadowed second instance) reads the tree candidate effectively, an installed instance reads its vdb record.

## fn direct_solve_arg_mode
- Portage `_want_installed_pkg` (`depgraph.py:7280-7302`) folded to what the direct solve needs: the installed instance is "argued" (excluded from every parent's match list) exactly when the user explicitly requested that cp (a top-level atom targets it) without `--exclude` and outside selective mode. Excluded/selective runs want the installed package (`True`), as does a cp nobody asked for.

## fn direct_solve_slot_conflicts
- Insertion order for `or_tuple` first-pulled (`:1998-2006`): merge instances in walk (entries) order, installed ones after (Portage's tracker order for installed nodes is walk-dependent; every S2 oracle tuple is all-merge, so this is untested beyond that -- documented here, not silently assumed).
- All conflict members, for the non-conflict test (`:1897-1901`): a parent whose (cp, version, installedness) names a conflict instance draws a package-to-package edge instead of the `non_conflict_node` substitution.
- `parent_identity`: full `cat/pkg-ver:slot/sub::repo` form (Portage's own): strip the slot/repo suffix `split_cpv` cannot read.

## fn solve_one_conflict
- Slot-operator rebuild conflicts never enter the direct solve (`:1784-1788`): their key is `(root, slot_atom)`; portuale's replace set is cp-keyed, which is the same gate at this granularity (#24/#78 territory, not #90). Neither do conflicts that feed the need_rebuild trailer: an installed parent whose built slot-operator atom (`:=` with a sub-slot, Portage `Atom.slot_operator_built`) cannot rebuild (`--exclude` / `--useoldpkg-atoms`) is Portage's rebuild-path residue, not remove-path material -- removing the reinstall would silence the trailer the F-A2 gate exists to print. (The masked-or-unavailable-ebuild reason stays the filed follow-up: this gate only knows the two pinned reasons.)
- Edges: (parent node, matched instance indices). A top-level `(Argument)` pull (empty cpv): no package parent, so the shared sink. Its atom still matches normally.
- Autounmask-broken USE or multislot (`:1926-1931`, bug 220341): protect the whole conflict from removal instead of solving it wrong.

## fn protect_forced_instances
- `or_tuple`s resolve only once every plain package had its chance (`:1994-2006`): an already-forced member satisfies the dependency, otherwise the first-pulled member wins ("the package that was pulled first, as this should be the most desirable choice").
- Dependencies of forced packages join the set (`:2012-2020`; portuale models no indirect candidates -- see the fn docs).

## fn pair_instance_with_others
- The take-over version: the first surviving same-cp instance (installed, or a forced merge) in conflict order -- never the removed instance itself. Empty when every instance drops (degenerate: no edge reached anything); then the entry is deleted outright with no notice owed (there is no keeper to report against).
- Missed-update recording (`:2063-2106`): for the removed instance, every parent of every *other* instance whose atom does not accept it. No `is_arg_parent` gate here, exactly like Portage.
- Backlog #230: the slot instances' own `use_display` is the slot-collision notice's IUSE-only rendering; the skipped block renders Portage's full `pkg_use_display` instead (recomputed here -- the notice keeps its own).
- #270: a parent no merge entry replaces, whose installed instance carries the atom, is Portage's installed (nomerge) node -- `installed in '<root>'`, vdb slot/repo, like the #205 rows of `backtrack_missed_updates` -- not a merge-scheduled one. A merge-scheduled parent keeps the tree form.
- Backlog #242 Slice D: per-node roots (the missed instance rides the rejecting parent's edge; a merge parent rides its entry; installed parents ride the walk root — always the target walk here).
- #270: blocks in the order their missed package was first recorded (Portage renders its `_conflict_missed_update` dict in insertion order, `depgraph.py:1529-1565`); only the parents inside one block are sorted -- Portage iterates a set there, so any fixed order is as good, and a stable one keeps the output deterministic.

## gate threading (#571)
- `DirectSolveInput` carries the gate into the skipped-block displays; `direct_solve_instance_use` stays raw (installed-anchored addressed read of the instance's own version).
