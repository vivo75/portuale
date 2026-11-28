# crates/portage-repo/src/tests/tests_unwalked_consumer.rs
Complete mode and the installed consumers the walk never recursed into (#603).

## module
- Oracle: host Portage 3.0.83 with the same argv on a scratch copy of the fixtures, captures in `LLM/evidence/2026-11-06-sweep/603/` (`plain`, `matching`).
- `resolve_with_world` feeds the world file as complete-graph seeds and turns `selective` off, as the CLI does for a plain `emerge -p <atom>`; with `selective` on the installed `w603lib-3` would stay `AlreadyInstalled`.
- `unwalked_consumer_atom_matching_nothing_installed_does_not_pin` fails on the pre-#603 tree (checked by stashing the `rev_dep.rs` change).

## fn unwalked_consumer_atom_matching_nothing_installed_does_not_pin
- Backlog numbers removed from the comment (original text): #603. Installed `w603lib-3` and world-only `w603cons-1` (`<dev-libs/w603lib-2`). Portage 3.0.83 `emerge -p dev-libs/w603lib` (`603/plain.portage.txt`): complete mode walks the consumer, its atom matches no installed package, so it is filed as initially unsatisfied and never a parent atom (D:8797-8806): `U w603lib-3.1 [3]`, 0/20, no skip block.

## fn unwalked_consumer_atom_matching_an_installed_package_still_pins
- Backlog numbers removed from the comment (original text): #603 counterpart. Installed `w603lib-1` matches `<dev-libs/w603lib-2`, so `_complete_graph` adds it and the conflict forms (D:8807-8834). Portage 3.0.83 (`603/matching.portage.txt`): `R w603lib-1`, 1/20, skip row from `w603cons-1`.

## fn w602_root
- Backlog numbers removed from the comment (original text): The #602 shape: installed `w602prov-1` (`0/1`) and `w602cons-1` (`dev-libs/w602prov:0/1=`), in world.

## fn slot_conflict_while_backtracking_is_reported_for_complete_mode
- Backlog numbers removed from the comment (original text): #602. `w602s` picks 2, `w602a` needs `<w602s-2`: a slot conflict. Portage turns complete mode on when one appears while backtracking is allowed (D:9473-9480); the CLI's second walk reads this flag (`602/` captures: `r U w602prov-2`, `rR w602cons-1` with the complete-graph switches off).

## fn w605_root
- Backlog numbers removed from the comment (original text): The #605 shapes: installed `w605x-1` built with `w605flag` and one world consumer per variant (`a`: `|| ( x[w605flag] y )` with `w605y-1` installed, `c`: plain `x[w605flag]`). `w605x-2` merges with the flag off (profile default).

## fn post_merge_use_reads_the_merge_node_use
- Backlog numbers removed from the comment (original text): #605: in `_graph_trees` the `[use]` of an `||` arm is matched against the merge node's own USE (DC:492-495), not the tree or the vdb.

## fn upgrade_breaking_an_unwalked_consumer_use_dep_conflicts
- Backlog numbers removed from the comment (original text): #671. Portage 3.0.83 `emerge -p =dev-libs/w605x-2` (`605/C-p`): complete mode walks `w605conc`, `w605x[w605flag]` still matches the installed `w605x-1`, so it is pulled back in: a slot conflict, rc 1, "Change USE: +w605flag". Variant A (`605/A-p`): the post-merge `||` takes `w605y` (#605), no conflict.

## fn update_held_back_by_an_installed_consumers_use_dep_is_reported
- Backlog numbers removed from the comment (original text): #699. Installed `w605x-1` (built `w605flag`), world consumer `w605conc-1` (`dev-libs/w605x[w605flag]`), visible `w605x-2` with the flag off. `-uD` keeps `w605x-1`; the plan is healthy but the user must be told that `w605x-2` exists and `+w605flag` lets it in (charter property 5). Portage 3.0.83 fails with the same hint (comparison only).

## fn an_or_arm_the_installed_consumer_does_not_need_holds_nothing_back
- Backlog numbers removed from the comment (original text): #699 negative: `|| ( w605x[w605flag] w605y )` with `w605y` installed is met without `w605x`, so `w605x-2` is simply not required by the consumer.

## fn p700_empty_world_consumer_reached_through_a_merge_bound_package
- Backlog numbers removed from the comment (original text): #700, charter property 1. The #676 shape with an empty world: `p590vpull-2` names the installed `virtual/p590vvb-1` (`=p590vprov-5.42*` or `=5.38*`), which keeps `p590vprov-5.44` out of the plan even though no world atom reaches it.
