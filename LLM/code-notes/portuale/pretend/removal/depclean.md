# crates/portuale/src/pretend/removal/depclean.rs
`--depclean`.

## struct LibConsumerProtection
- One cleanlist package `--depclean-lib-check` keeps installed: it solely provides a library still needed at link level (`NEEDED.ELF.2` soname) by a package that is *not* itself being removed. Surviving consumer cpv -> the sonames it needs, sorted (Portage's `sorted(libs)` / `sorted(consumer.mycpv ...)`).

## fn lib_consumer_scan
- Ports Portage's `_calc_depclean` `--depclean-lib-check` scan (`actions.py:1381-1546`), narrowed to its pure computation. For every cleanlist package, takes the `NEEDED.ELF.2`-indexed objects it owns that carry a `DT_SONAME`, and asks `needed_elf::find_consumers` (non-greedy, so a consumer already satisfied by *another* provider of the same soname is excluded -- Portage's `actions.py:1454-1504` multi-provider filter) which surviving objects still link against them. A consumer whose owning package is itself in the cleanlist is dropped (Portage's `lib_consumer in clean_set`).
- **Documented narrowing**: Portage's multi-provider filter only lets an *alternative* provider satisfy a consumer when that alternative's own package is not also being removed; `find_consumers` is not clean-set aware, so portuale can under-report in the (rare) case where the only surviving provider of a soname is itself another cleanlist member. Also Portage's intermediate `>>> Assigning files to packages...` progress line can appear with no WARNING following (all consumers satisfied elsewhere); portuale prints it only alongside the WARNING.
- The objects this package owns that provide a soname: Portage's `pkg_dblink.getcontents()` intersected with the linkmap, non-empty soname. Final sort: Portage's `sorted(consumer_map, key=cmp_sort_key(cmp_pkg_cpv))`.

## fn apply_depclean_lib_check
- Ports Portage's `_calc_depclean` `--depclean-lib-check` phase (`actions.py:1356-1590`): prints the `>>> Checking for lib consumers...` progress and, when any provider must be kept, the `* ...one or more packages will not be removed` WARNING (Portage's `bad(" * ")` prefix, `logging.WARNING` -> stderr) plus the per-provider detail, then hands the protected providers to `recompute` (a second `depclean_cleanlist` / `prune_cleanlist` pass that seeds them as roots so their own deps also leave the cleanlist). A no-op when `lib_check` is off (`--depclean-lib-check=n`) or the cleanlist is already empty.
- Warning shape: Portage's `"".join(bad(" * ") + f"{line}\n" for line in textwrap.wrap(msg, 70))` -- the wrap is pinned here since it never changes. Second `msg` list: a blank ` * ` line, then `  <cpv> pulled in by:` and `    <consumer> needs <sonames>` per provider, then a trailing blank ` * ` line.
- Portage's `_calc_depclean` `--depclean-lib-check` phase (`actions.py:1356-1590`): run `lib_consumer_scan` on the tentative `result`, print the `>>> Checking for lib consumers...` progress and, when any provider must be kept, the `* ...one or more packages will not be removed` WARNING (Portage's `bad(" * ")` prefix, `logging.WARNING` -> stderr) plus the per-provider `pulled in by:` / `needs <soname>` detail, then hand the protected providers to `recompute` (a second `depclean_cleanlist` / `prune_cleanlist` pass that seeds them as roots so their own deps also leave the cleanlist). A no-op when `lib_check` is off (`--depclean-lib-check=n`) or the cleanlist is already empty.
- Advisory-line encoding: `None` = Portage's leading bare `writemsg_stdout("\n")`; `Some("")` = Portage's `msg.append("\n")` (still `WARN(" * ")`-prefixed); `Some(text)` = a ` * `-prefixed advisory line. The first paragraph (`Depclean may break link level dependencies...`) is printed only when `--depclean-lib-check=n`.

## fn depclean_unresolved_halt
- Ports Portage's `_calc_depclean` `unresolved_deps()` halt (`actions.py:1177-1248`): depclean/prune print the `bad(" * ")`-prefixed `Dependencies could not be completely resolved ...` block (`logging.ERROR` -> stderr) and exit 1 without removing anything. `is_prune` adds the prune-only `use --nodeps` trailer. Wrap: Portage's `textwrap.wrap(..., 65)` -- pinned, it never changes.

## struct RunDepcleanPretendArgs
- `repos`: threaded into the post-removal GLEP 42 count notice (backlog #196 fix round 1).
- `deselect`: Portage's `action_depclean` `deselect = myopts.get("--deselect") != "n"`. See `depclean_cleanlist`'s `deselect` param.
- `pretend`: Portage's `action_depclean:908-911` (`Number removed:` vs `Number to remove:`).
- `quiet` / `noinfo`: threaded into the post-removal info-regen block (Portage's `--quiet` suppression + resolved-config `noinfo`).

## fn run_depclean_pretend
- Ports Portage's `emerge --pretend --depclean` / `-pc` (`action_depclean` + `_calc_depclean`, no package arguments). Portage's `action_depclean` literally feeds its cleanlist to `unmerge(..., "unmerge", cleanlist, ordered=ordered)` (and without `--pretend` that same `run_unmerge_pretend` really removes them via `execute_unmerge` + per-package `deselect_from_world`), after the safety halt and `--depclean-lib-check` have already run. See `portage_repo::depclean_cleanlist`'s doc comment for the graph and its documented narrowings.
- `emerge -pc <atoms>` (the `--depclean <atoms>` narrowing): the world "selected" plain atoms are dropped (the named packages get deselected *and* removed), every other installed package becomes a protected root, and the cleanlist is just the `args`-matched packages nothing else needs -- see `depclean_cleanlist`'s doc comment.
- Bare-name targets get their category from the vdb, then each atom is checked against the vdb (Portage's `action_depclean`, `:848-863`).
- Advisory block: Portage's `action_depclean` shows it only with no package arguments (`not myfiles`), non-quiet. The "Depclean may break link level dependencies" first paragraph is Portage's own `if "preserve-libs" not in features and --depclean-lib-check == "n"`. Each line is `colorize("WARN", " * ")` (yellow) + text, and each backtick-wrapped command inside the text is `good("...")` (green). `None` = Portage's leading bare `writemsg_stdout("\n")`; `Some("")` = Portage's `msg.append("\n")` (still `WARN(" * ")`-prefixed).
- `@world` = the `world` file (parent `@selected`, Portage's `show_parents`) plus each `world_sets` nested set's own atoms (parent `@<name>`). `depclean_cleanlist` drops these seeds in `args` mode.
- Safety halt: Portage's `_calc_depclean` `unresolved_deps()` (`actions.py:1247`).
- Verbose reverse-deps: Portage's `create_cleanlist` `elif "--verbose": show_parents(pkg)`.
- Removal-order line: Portage's `_calc_depclean` prints it only when there's something to remove.
- Stats block: Portage's `action_depclean` prints it after `unmerge()` returns. A mid-removal failure Portage-`sys.exit`s before this.
- Removal-order line: Portage's `_calc_depclean` prints `>>> Calculating removal order...` only when there's something to remove; then `unmerge(..., "unmerge", cleanlist)` renders the per-package block.
- Portage's `action_depclean` calls `unmerge(root_config, myopts, "unmerge", cleanlist, ordered=ordered)` -- the same `unmerge()` the `-C` path uses. Without `--pretend`, `run_unmerge_pretend` removes them.
