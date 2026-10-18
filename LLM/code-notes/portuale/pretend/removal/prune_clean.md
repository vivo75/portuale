# crates/portuale/src/pretend/removal/prune_clean.rs
`--prune` and `--clean`.

## struct RunPrunePretendArgs / RunPruneNodepsPretendArgs / RunCleanPretendArgs / RunPruneNodepsOrCleanArgs
- `repos`: threaded into the post-removal GLEP 42 count notice (backlog #196 fix round 1).
- `quiet` / `noinfo`: threaded into the post-removal info-regen block (Portage's `--quiet` suppression + resolved-config `noinfo`).

## fn run_prune_pretend
- Ports Portage's `emerge --prune` / `-P` (`action_depclean` with `action="prune"`). Unlike `--depclean`, Portage's `action_depclean` returns right after `unmerge()` (`actions.py:888`), so there is **no** `* ` advisory block (only `action == "depclean"` prints it, `:840`) and **no** `Packages installed:` / `Required packages:` / `Number ...` stats block. The empty-cleanlist message gains a `>>> To ignore dependencies, use --nodeps` line (`create_cleanlist`, `:1348`). Without `--pretend` the shared `run_unmerge_pretend` really removes the cleanlist (`execute_unmerge`). See `portage_repo::prune_cleanlist`'s doc comment for the removal-set semantics.
- The `unresolved_deps()` safety halt serves `action in ("depclean", "prune")`, so it applies here too (with the prune-only `use --nodeps` trailer). `--depclean-lib-check` applies to `--prune` too.
- Portage's `create_cleanlist` prune branch prints `show_parents(pkg)` inline while it builds the removal list.
- Removal: Portage's `action_depclean` (`action == "prune"`): `unmerge(..., "unmerge", cleanlist, ordered=ordered)`.

## fn run_prune_nodeps_pretend
- Ports Portage's `emerge --pretend --prune --nodeps` (`actions.py:2684-2697`): `--nodeps` routes prune to `unmerge()`'s own `_unmerge_display` prune branch (`unmerge.py:245-272`) *instead of* `_calc_depclean` -- so there is NO dependency/reachability check at all, no `>>> Calculating removal order...`, and no `show_parents` (`--verbose` is inert here). For every cp with more than one version installed the best (highest) version is `protected` and every other version is `selected` for removal (see `portage_repo::prune_nodeps_selection`). `_unmerge_display` renders it unordered: the header, per-cp `selected:`/`protected:`/`omitted:` blocks (`omitted` is always `none` here -- every version is either selected or protected), the `sys-apps/portage` self-skip, the "still listed in package sets" warning, and the footer. Empty result: `>>> No outdated packages were found on your system.` with no args (Portage's `global_unmerge`), else `>>> No packages selected for removal by prune` -- both exit 1 (Portage's `_unmerge_display` returns `(1, {})`, unlike plain `--prune`'s exit 0). Without `--pretend`, `execute_unmerge` removes the selected versions after the display -- Portage's `actions.py:2684` routes `prune --nodeps` through the very same `unmerge()` `-C` uses.

## fn run_clean_pretend
- Ports Portage's `emerge --clean` (`action_uninstall` -> `unmerge` with `unmerge_action == "clean"`). Individual ebuild-path args are rejected (Portage's `unmerge.py:131`); `resolve_cleanup_args` only produces `cat/pkg` atoms here. No `sys-apps/portage` self-skip (Portage's `unmerge.py:368`).

## fn run_prune_nodeps_or_clean
- Header: Portage's `_unmerge_display` (`unmerge.py:195`) prints it only under `--pretend`/`--ask`.
- Self-skip: Portage's `sys-apps/portage` self-skip (`unmerge.py:368-391`); Portage's `unmerge.py:368` `if unmerge_action != "clean"`.
- Warning: Portage's `_unmerge_display` "still listed in package sets" (`unmerge.py:393-447`).

## fn run_prune_nodeps_or_clean (autoclean flag)
- Backlog #404 (02.346 S11): `autoclean: bool` threads the post-merge auto-clean through the shared `--clean` selection (`clean_selection` system-wide with empty targets, same table, same `execute_unmerge` removal). With it set, the `CLEAN_DELAY` countdown is skipped (Portage `_emerge/unmerge.py:638`, `not autoclean`); the two header lines live in `run_autoclean_after_merge` instead, so manual `--clean`/`--prune`/`-C` output is unchanged. S404 oracle: no `Waiting` line in any auto-clean cell with the 5-second default in force.
