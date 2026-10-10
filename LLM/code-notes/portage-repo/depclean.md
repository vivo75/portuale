# crates/portage-repo/src/depclean.rs
depclean, prune and clean selection.

## struct DepcleanResult
- Portage `emerge --depclean`'s own removal list (`_calc_depclean` + `create_cleanlist`, no `args_set`).
- The graph edges: a package that is *only* a build-time dependency of a kept package is itself kept -- `emerge --depclean` will not remove something the tree still needs to rebuild what stays installed. Build-time keys are followed because Portage's "remove" mode sets `bdeps="auto"` (`create_depgraph_params(myopts, "remove")`, `create_depgraph_params.py:100-103`; `depgraph.py:4208-4213` discards them only with `--with-bdeps=n`). Removal walks resolve build deps against the root being cleaned (`depend_root = myroot`, `depgraph.py:4218-4219`).
- No-`args` roots: the installed packages `world_atoms` ∪ `system_atoms` match. `args`-mode roots: `system_atoms` ∪ `{=cpv | installed, unmatched by args}` (Portage `_calc_depclean` plus `_complete_graph` in "remove" mode drop the world "selected" plain atoms entirely -- the named packages get removed *and* deselected).
- `lib_protected_providers` path: Portage computes the tentative cleanlist, then (`actions.py:1356-1573`) runs a soname-linkage scan over `NEEDED.ELF.2` and, for any cleanlist package solely providing a library still needed by a *surviving* consumer, re-adds that provider (and its dependencies, via `_complete_graph`). The caller owns the `NEEDED.ELF.2` linkage map (see `portuale`'s `needed_elf`): first call with `lib_protected_providers = &[]`, scan, then call again passing the packages found. Cleanlist, `required_count`, `kept_parents` and removal order are all recomputed consistently.
- Narrowings: a lib-protected provider is kept but, unlike Portage (which labels its reverse-dep entry with the link-level consumer), contributes no `--verbose` `kept_parents` line unless an ordinary dependency also reaches it.
- Result fields: already in Portage `_calc_depclean`'s unmerge order; `required_count` is Portage `req_pkg_count` (the `Required packages:` stat); `ordered` mirrors Portage `_unmerge_display`'s `if not ordered:` regroup branch (`unmerge.py:459-474`); the caller is `run_depclean_pretend` / `run_prune_pretend`.
- `kept_parents`: Portage `create_cleanlist`'s `elif "--verbose": show_parents(pkg)` (`actions.py:1324`/`1331`). No-args covers all kept packages; args covers only the `args`-matched ones. Atoms within a line sort by atom package-name descending (`sorted(atoms, reverse=True, key=attrgetter("package"))`); a kept package whose only parent is the internal protected-set (Portage `protected_set_name` filter) contributes no entry.

## fn topological_removal_order
- With no edges at all (`len(graph.order) == len(graph.root_nodes())` in Portage), `ordered` is `false` and the input `cat`/`pn`/version sort is kept; Portage `_unmerge_display` then does its own `cat/pn` grouping. With edges the order is Portage's `nodes.sort(reverse=True)` repeated pop. Rationale quoted from Portage: "to avoid breaking things that may need to run during pkg_prerm or pkg_postrm".
- Priority table (Portage `actions.py:1608-1622` + `UnmergeDepPriority.__int__`): `IDEPEND` 0, `RDEPEND` -2 (bumped to -1 for `slot_operator_built` -- `foo/bar:2/2=`, Portage `runtime_slot_op`), `PDEPEND` -3, `DEPEND`/`BDEPEND` -4 (a `DEPEND`/`BDEPEND` slot-op edge stays -4; `__int__` never bumps `buildtime_slot_op`). When one `(i, j)` edge comes from several keys, the highest wins (Portage `digraph`'s sorted `priorities[-1]`).
- The pop mirrors Portage `actions.py:1713-1727`: while any package still has an un-removed depender, emit all current true roots at once, cpv-descending; only with none (a genuine cycle) use the `ignore_priority_range` scan (`[-4, -3, -2, -1, 0]`), popping just the cpv-max node so the fewest ordering edges break. `ignore_priority == 0` always yields a node, so the loop terminates. Portage's `node_refcounts` pre-sort has no observable effect (every batch is re-sorted by cpv anyway), so it isn't reproduced.

## fn render_show_parents
- Dedupes identical `(parent, atom)` edges before rendering.

## fn pprovided_matches
- E.g. `depgraph.py:5497-5503` for a top-level target, `dep_check.py:1052` area for a dependency.

## fn unresolved_runtime_deps
- Portage `DEPEND`/`BDEPEND` are `buildtime` = SOFT and never trip the halt (only `runtime`/`runtime_post` do). The any-of resolution for a whole `||` group is out of scope. Portage relies on libc genuinely being installed (`find_libc_deps` -- what an installed `virtual/libc` pulls in); `strip_libc_deps`' premise is that implicit libc deps are noise, so a config/fixture without an installed libc must not spuriously halt.

## struct DepcleanCleanlistArgs
- `world_seeds` labels: `@selected` for a `world` file line, `@<name>` for a `world_sets` nested set.
- `deselect`: Portage `action_depclean`'s `deselect = myopts.get("--deselect") != "n"`; `--depclean <atoms> --deselect=n` keeps `world` as a protection root so a world member named as an arg is kept.

## fn depclean_cleanlist
- `matches_atom`/`matched_by_args` shape: candidate strings per installed package, `parse_atom` + `match_from_list` filtered to the same cp.
- Kept/`kept_parents`/`unresolved` coverage: `show_parents` over kept packages cpv-sorted; `unresolved_deps()` over *every* kept package (Portage `_complete_graph`'s full required set); `required_count = reachable.len()`.

## fn prune_cleanlist
- Portage seeds its `protected_set` with every installed cp as a bare `cp` atom (resolving to just the *highest* installed version), then additionally protects the highest version of every cp and every non-highest version an argument atom does not match. With no `args`, `args_set` auto-fills with every multi-version cp (`actions.py:1071-1075`); `emerge --prune <atom>` uses those atoms as `args_set` instead.
- `lib_protected_providers` and `package.provided` params mirror `depclean_cleanlist`'s.

## fn prune_nodeps_selection
- Portage `actions.py:2684` routes `--nodeps` to the `_unmerge_display` prune branch *instead of* `_calc_depclean`. The `COUNTER` tiebreak is a documented narrowing, same category as the other `-pC` slot refinements. `--prune --nodeps '<cat/pkg-3'` keeps `pkg-2` even if `pkg-3` is installed (split over the matched set). A cp matched by several atoms appears once (Portage `_unmerge_display`'s `if not ordered:` regroup).

## fn clean_selection
- `args` given: the best/rest split is over the union of the matched sets, grouped by `(cp, slot)`; empty: every installed `(cp, slot)` (Portage `vardb.cp_all()` -> `cp_list`). Portage's single-instance-per-slot fixtures never hit the `COUNTER` collision.

## fn strip_slot_operator_one
- Guards: a plain `:=` (no slot), a `:2` / `:2/3` (no operator), or a `:2*` is left untouched, matching Portage `strip_slots`'s `x.slot_operator == "=" and x.slot is not None` guard. `::repo` uses `::` and use-deps live inside `[...]`, so a single leftmost replacement is safe.
