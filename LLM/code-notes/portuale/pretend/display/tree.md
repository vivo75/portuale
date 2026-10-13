# crates/portuale/src/pretend/display/tree.rs
The `--tree` display. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## struct PrintTreeArgs
- `force_sizes` (backlog #206 S2): forwarded to `print_entry_line`.
- `circular` (backlog #221): the resolve's recorded cycle edges -- the `--tree` walk keeps the cycle-breaking branch the walk took.

## fn print_tree
- Ports Portage's tree display (`output.py:585-586` routes tree mode through `output_helpers.py::_tree_display`, `:341-407`, then `_ordered_tree_display` `:434-495` / `_unordered_tree_display` `:410-431`, pruned by `_prune_tree_display` `:497-535`).
- Display-only port; C2 owns the tree-mode serializer that can schedule extra uninstalls (`TEST/findings/l0.md` "#75 C0", p2b).
- Graph rewrite shapes: `Package -> Blocker -> Uninstall` (`:377-390`); `upgrade_node -> blocker` (`:392-398`).
- Absent-owner scan arm (#77 A1): `[blocks b]`, then `[uninstall]`, then the merge.
- Dependency edges: `build_digraph`'s own `_select_pkg_highest_available` narrowing (#82) -- a suppressed `||` alternative contributes no tree edge.
- `--onlydeps`: Portage's retlist excludes the suppressed root but the graph keeps it; portuale excludes it from the ordered node list for the same effect.
- Return value: Portage's `_ordered_tree_display` walk product, before printing; the only consumer is backlog #206 S2's `Total:` recount (Portage counts displayed merge *rows*, repeats included, `nomerge` excluded).
- #131 S1: the walk consumes the tree-mode serialization order (merge direction; the ordered walk below iterates it reversed), exactly like Portage's `_ordered_tree_display` consumes the reversed tree-mode retlist (`depgraph.py:10618-10619`) -- not the flat merge order. `display_order` is `merge_order::tree_display_order` (a permutation of the entry indices, same leftover weave-back as the flat path, so every entry keeps its node even when the scheduler never saw it).
- Backlog #155 S1: Portage's digraph parent lists follow edge-insertion order (resolution order), and the walk picks the first untraversed parent -- enumerating over merge order lets a later-discovered dependent precede the earlier puller in every parent list they share (`mgfb` before `mgfa` for `mgfc-1`, hiding the Portage `[nomerge] mgfa` parent).
- `required_by` fallback: Portage's digraph still nests a deps-less entry under its owners. #82 (`TEST/findings/l0.md` "#82"): `required_by` is cp-keyed, so every scheduled slot of a cp lists every owner that pulled any of them; an owner whose own atom already resolved to one instance (`_select_pkg_highest_available` returned exactly one package) has no edge to the others in Portage's digraph -- with `treeslotparent` pulling only `dev-libs/treeslotpkg:1`, Portage nests `1.10` under it and `1.9` elsewhere.
- Replaced a private tie-break copy of the narrowing that had drifted to a string version compare with `portage_repo::resolved_dep_targets` verbatim.
- Blocker insertion: Portage appends a solved blocker to the retlist right after the blocked package that solved it, and the display walk consumes the reversed retlist. The prune drops a Blocker that exceeds the merge depth set by shallower later merges (C0 p2b).
- Tree-stuck arm (backlog #81, C0 p2b): flat display never reaches it (disposition is Hidden there); it only changes which rows become tree Blocker nodes.
- `_ordered_tree_display`'s ancestor walk passes `ordered=False` (`:453`).
- Portage `_create_graph` resolves each dep atom to one package. #82: that narrowing is `build_digraph`'s own, reused verbatim through `portage_repo::resolved_dep_targets` (merge-bound targets only -- portuale renders no line for an installed entry, so an edge to one would open a hole in the tree) instead of the private copy this function used to carry, whose tie-break had drifted to a string version compare. Suppressed `|| ( … )` alternatives are already `None` there, so no separate `kept_alt_branches` pass is needed.
- Blocker nodes: a satisfied row hangs its removal/replacement under the blocker, and the blocker under the row's owner (unless the row already lives on that same removal entry -- the #77 absent-owner arm, whose owner is Portage's non-node vardb `Package`). #84: one kept-branch derivation for the whole node walk.
- `_ordered_tree_display` (`output_helpers.py:434-495`): walk the reversed list; a node that is a child of the current chain continues it, otherwise `add_parents` fills the ancestor chain in first. The `ordered` flag is `False` for every ancestor-walk occurrence (`:453`), which is what renders a merge node as `nomerge`. /// Arguments of [`add_parents`].
- `_prune_tree_display` (`output_helpers.py:497-535`).

## fn unordered_display_order
- `_unordered_tree_display` (`output_helpers.py:410-431`): DFS from every root (no parents) in graph insertion order, each node once.

## fn render_tree_rows
- `ordered` is Portage's `set_pkg_info` flag (`output.py:612-614`). B0b's p2c-t capture has depth 2 at three spaces.
- Indent: `depth + 1` spaces for depth >= 1, none at depth 0.
- `self.indent` is exactly `depth` (`_ordered_tree_display`'s depth passes straight to `_display_tree`).
- Unreached nodes: Portage's own "never silently lose information".
