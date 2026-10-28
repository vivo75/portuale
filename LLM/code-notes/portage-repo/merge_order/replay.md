# crates/portage-repo/src/merge_order/replay.rs
Replaying `_create_graph` for fail-fast passes.

## struct ReplayUse
- The USE state `replay_create_graph` needs for one entry: its declared (profile-valid) `IUSE`, the flags its configuration enables before any autounmask change, and the flags no `package.use` change can touch (profile-forced or -masked). `None` for an entry whose USE cannot change (installed, binary, missing) -- its use-deps are not checked.

## struct ReplayFailure
- The first dependency Portage's `_create_graph` fails on, as `(parent entry, index into its deps)`. `contradiction` marks the autounmask shape: the atom needs a USE flag the other way from a change already needed on the same package (Portage rejects the flip, `depgraph.py:7714`, and the dependency is unsatisfied).

## struct WalkReplay
- What `replay_create_graph` saw: Portage's admission order, the edge that first admitted each entry (`None` for an argument or an unreached entry), the first failing dependency, and the USE changes the walk needed, per `cat/pkg` (Portage's `_needed_use_config_changes`).
- For an argument entry: the index into `top_level_atoms` of the atom that added it.
- Every resolved `(parent, dep index, target)` edge, in walk order (Portage's digraph parent-insertion order).
- Per `cat/pkg`, the `(parent, dep index)` whose atom first needed each USE change the walk made (Portage's dep chain for the change).

## fn replay_create_graph
- Backlog #244: Portage's `_create_graph`'s walk replayed over a settled graph, stopping at the first dependency that fails -- the fail-fast half portuale's BFS pass (which walks everything and classifies afterwards, see `abort_outcome`) does not have.
- Walk order is `build_digraph`'s own `.order` replay (arguments added in argv order and pushed, `_dep_stack` popped LIFO, disjunctive bundles deferred until the stack drains). Per popped node, each dep key is one `_add_pkg_dep_string` call: `_minimize_children` (`depgraph.py:4751-4775`) yields unresolvable atoms first, so a failing atom anywhere in the key fails the node before any of the key's children is admitted; earlier keys' children stay admitted.
- USE: an atom's use-deps are checked against the target's configured USE with the needed changes so far applied (Portage's `_pkg_use_enabled`). With `allow_flips` a mismatch the target can take becomes a needed change -- unless the change contradicts one already needed, which is the failure (`depgraph.py:7714`); without it (Portage's `myparams["autounmask"] = False` clean pass) every mismatch fails. `needed_init` seeds the changes from earlier passes (Portage's `_feedback_config`), keyed by `cat/pkg`.
- Deliberately conservative: an edge counts as failing only when its `cat/pkg` has a `NoVisibleCandidate` entry naming this parent, or when the USE rule above fails it; an edge with no entry to land on at all (deps portuale never walks) is skipped.

## fn replay_key
- One `_add_pkg_dep_string` call of `replay_create_graph`: resolve every atom of `entries[i]`'s key `key`, fail the key on the first unresolvable one (unresolvable atoms are yielded first, so nothing of the key is admitted then), else admit the targets in atom order. Returns `true` once a failure is recorded.
- Pass 1: pick every atom's target and settle its USE; a failure stops the key before anything is admitted.
- #594: the loop runs target-less edges first (`replay_edge_has_target`, stable sort over the stored order), so a missing dep fails the key before a later atom's USE change is recorded -- the order above is now the implementation, not just the intent.
- Only the use-deps written unconditionally (`flag` / `-flag`) are checked: a `flag?` / `flag=` one depends on the parent's own USE, which Portage's walk may itself change (`_apply_parent_use_changes`) -- evaluating it against the settled parent would invent contradictions Portage never meets.
- Portage's `_select_pkg_highest_available` over the graph: an instance whose IUSE can carry every use-dep flag, then the usual merge-bound-then-highest ranking. Only the unconditional `flag` / `-flag` forms constrain the target (`use_deps_satisfied`'s own rule); a flag with a `(+)`/`(-)` default need not be declared.
- Undeclared: satisfied or not by its `(+)`/`(-)` default alone -- no change can help.

## fn replay_edge_has_target (#594)
- Whether a replayed edge can land on a merge-bound or installed target: a `NoVisibleCandidate`/`Uninstall` outcome never lands, and neither does an edge with no entry at all. Mirrors the target computation at the top of the old `replay_key` loop (same atom parse with the same `drop-ok` fallback, same blocker skip, same `edge_matches` filter); blockers sort target-less and skip either way.
