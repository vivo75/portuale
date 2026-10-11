# crates/portage-repo/src/tests/depclean.rs
Depclean and prune selection tests: cleanlist, removal order, nodeps split.

## fn depclean_cleanlist_keeps_the_required_closure_and_lists_the_rest
- Fixture graph: dcworld (world) -> dcdep -> dcsub, dcworld -[bar?, USE=bar]-> dccond; dcorphan -> dcorphandep orphaned; systempkg required as a root. dcbuilddep rides DEPEND, dcbdep rides BDEPEND; seven required in total.
- `args` mode (`-c dev-libs/dcorphan`): just that one; its private dep dcorphandep is protected as a non-arg. `-c dev-libs/dcsub` lists nothing (dcdep still needs it).
- `--depclean-lib-check` feedback: forcing dcorphan as a protected provider keeps it and its RDEPEND, recomputing an empty cleanlist and required_count 9.

## fn depclean_cleanlist_package_provided_intercepts_both_a_world_root_and_a_dependency_edge
- Expected value from Portage: the `-pc` advisory ("package.provided ... will be removed by depclean, even if they are part of the world set"). Portage `_resolve`'s per-arg loop intercepts `pwprov` before it ever becomes a graph root (`depgraph.py:5497-5503`, before `_add_pkg`/`_select_package` run), so world membership does not protect it; `dep_check.py:1052` intercepts the `pw -> pdprov` edge too (no graph node for the literal installed provider), so `pdprov` stays unreachable and joins the cleanlist. `pw` (genuine, non-provided) stays required.

## fn depclean_unresolved_halt_does_not_fire_for_a_dep_satisfied_only_by_package_provided
- `uk` (kept, reachable from world) RDEPENDs `dev-libs/uprovided`, which has no literal vdb entry, only a `package_provided` listing. Without the fix this spuriously halted with "not being installed" (Portage behaviour: it doesn't).
- Portage dependency resolution treats a `package.provided`-matched atom as already satisfied (`dep_check.py:1052`), so `unresolved_deps()` must not flag it.

## fn depclean_cleanlist_is_returned_in_topological_removal_order
- Portage `_calc_depclean` removes a package before the ones it depends on; each level's ready set emits cpv-descending (Portage `nodes.sort(reverse=True)`). Fixture: zztop RDEPENDs aabase, mmid RDEPENDs zztop, plus edgeless loner; order is mmid, loner, zztop, aabase.
- Flat root (no edges): `ordered = false`, cat/pn sort kept.

## fn topological_removal_order_breaks_a_cycle_by_priority_and_pops_one
- `hi` DEPENDs `lo` (buildtime, priority -4), `lo` RDEPENDs `hi` (runtime, -2). Portage's `ignore_priority_range` scan tries -4 first: only `lo` has all remaining incoming edges <= -4 (the single hi->lo edge), so `lo` pops first.
- Mutual RDEPEND (both -2): -4/-3 yield nothing, -2 makes both eligible.

## fn prune_cleanlist_removes_superseded_versions_only
- Fixture: aa, zz, mm multi-version; single is not. `zz-1.0` pins `aa-1.0` (both themselves prunable).
- Topological order: level 0 ready {zz-1.0, mm-1.0} cpv-desc, then aa-1.0. mm-3.0/aa-2.0/zz-2.0 are highest, single is single-version, keeper is protected.
- `--prune --verbose`: the highest versions are pulled in only by the internal protected-set seed, which `show_parents` filters out.
- `--prune --nodeps` keeps nothing for `keeper` (unlike `--prune`); `<dev-libs/mm-3.0` keeps mm-2.0 and prunes mm-1.0.
- `--prune --nodeps`: NO dependency check -- every non-highest version of every multi-version cp is selected, mm-2.0 included (unlike `--prune`, which keeps it for `keeper`). `--prune --nodeps <atom>`: best/rest split over the atom's own match set. `--prune dev-libs/mm`: only mm's old versions are candidates.

## fn depclean_cleanlist_deselect_false_keeps_a_world_member_named_as_an_arg
- `-c dev-libs/dw` (deselect default true): the world seed is dropped in args mode -> dw is unreachable -> removed. `-c dev-libs/dw --deselect=n`: the world seed is kept -> @selected still reaches dw -> nothing to remove.

## fn depclean_cleanlist_a_use_conditional_dep_that_is_off_does_not_keep_its_target
- dcw's `off? ( ... )` group is inactive -> dchidden is orphan.
