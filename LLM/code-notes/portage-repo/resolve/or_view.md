# crates/portage-repo/src/resolve/or_view.rs
The package set a `||` choice is judged against (#580 10b).

## struct PostMergeSet
- Portage `PackageTracker.match` (`package_tracker.py:230-252`) as `_graph_trees` hands it to `dep_zapdeps` (`depgraph.py:764-771`): the graph nodes plus the installed packages that no merge-bound node replaces. A node replaces an installed package of its slot or of its version (PT:137-143). An installed package that is itself a node (`AlreadyInstalled`) stays available. `Uninstall` and `NoVisibleCandidate` entries are not nodes.
- Built from a pass's `entries` and the vdb root. Survivors are read from `installed_refs` on demand. The set has no tree: an arm that only an ebuild could satisfy is not available, which is why `~p589acore-1.0` is unavailable in T1 without `--deep` and available when the walk merged it.
- Candidate strings are `cat/pkg-ver:slot/sub::repo`, matched with `match_from_list` on the USE-stripped atom.

## enum OrView
- Portage `_filtered_trees` (the normal walk, D:775-800) against `_graph_trees` (complete mode, D:764-771). The rule that picks the view per consumer is in `t9/views.md`; `rev_dep_or.rs` applies it. `Normal` delegates to today's functions. `PostMerge`: availability is `PostMergeSet::matches`; `all_installed` / `all_installed_slots` are the set (a `virtual/` cp counts as installed, DC:603-616); `all_in_graph` is the nodes only; `want_update` stays false (D:7359).
- `OrProbe` bundles the tree gate, the vdb root and the config that the `Normal` arms read.

## fn PostMergeSet::use_matches / OrView::use_satisfiable (backlog #605)
- In `PostMerge` a `||` arm's `[use]` is judged against the set members that match it: a merge node with its own USE (`GraphEntry.use_flags_display`, enabled flags and IUSE), a kept installed package with its vdb `USE`/`IUSE` plus the profile's `IUSE_EFFECTIVE`. Portage's `dep_zapdeps` reads `avail_pkg_use` from the `_graph_trees` dbapi, which carries the graph nodes' USE (`dep_check.py:492-495`).
- Before, the tree and the vdb stood in for the node, so `|| ( w605x[w605flag] w605y )` kept the `w605x` arm when the installed `w605x-1` had the flag but the merge node `w605x-2` did not. That only mattered once #671 made such an arm a conflict parent: without this, variant A became a false slot conflict.
- `use_deps_violated` gets an empty parent USE: `use_satisfiable` does not receive the consumer's USE, so an atom-level conditional (`[flag?]`, `[flag=]`) in a `||` arm is judged as if the consumer had every flag off. An approximation, not seen in any fixture or bed shape; group conditionals (`flag? ( … )`) are already reduced before this point.
