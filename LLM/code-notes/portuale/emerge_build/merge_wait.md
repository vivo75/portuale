# crates/portuale/src/emerge_build/merge_wait.rs
Merge-wait barriers and cycle-aware build readiness for the `-j` scheduler (backlog #512).

## const CORE_TOOLCHAIN
- Portage `_emerge/_find_deep_system_runtime_deps.py`, `CORE_TOOLCHAIN`: the 12 fixed cps whose ebuild environment runs under essentially every build. Copied verbatim (order kept); host `Scheduler.py` and the tag file are byte-identical.

## fn features_hold_all_merges
- Portage `_emerge/Scheduler.py`, `_build_exit`: a merge barriers when `"merge-wait" in build.settings.features` **or** the package is a system package. The scheduler checks per entry (`entry_resolved_features`, which folds `package.env` over the run-wide list and the calling env), falling back to the run-wide resolved list when no `package.env` entry matches — review follow-up I1, since the first version read the run-wide list only. Whole-token match, so `-merge-wait` disables and `merge-wait-foo` does not. The oracle control shape `05-default-features` (default `FEATURES`, `merge-wait` on) shows every merge serialized after the builds drain, including `2 merge wait` in `>>> Jobs:`.

## struct RuntimeIndex
- Built once per `merge_wait_set` (`deep`): same-cp membership over borrowed
  keys, per-entry `cpv:slot/sub-slot` match strings (the `entry_matches_any`
  shape), and every entry's resolved runtime targets. The deep walk then does
  no per-lookup indexing or key cloning.
- Forward edges resolve by cp membership narrowed with the dep atom against
  the target's match string (undecidable keeps the edge, like the merge-order
  digraph); an unqualified atom over two slots of one cp keeps both members
  (safe direction: over-barriers). `required_by` fallback links stay cp-level,
  so a fallback claim is suppressed when a same-cp sibling reaches the child
  through its own forward edge — that link is the sibling's node, not this
  entry's (Portage walks each package node's own children).

## fn merge_wait_set
- Portage `_emerge/_find_deep_system_runtime_deps.py` with `scope`: `none` selects nothing; `toolchain` the fixed list above; `system` the `@system` members in the graph; `deep` those plus everything reachable by runtime edges only (`RDEPEND`/`IDEPEND`/`PDEPEND`, i.e. `priority.runtime || priority.runtime_post`). The upstream white-box test `test_merge_wait_scope.py` (`testMergeWaitScope`, run green against `3rdparty/portage` at the `portage-3.0.83` tag) is the selection oracle: deep `{glibc, sysextra, runtime-dep, deeper-runtime-dep}`, system `{glibc, sysextra}`, toolchain `{glibc}`, none `{}`; default `deep`; bogus raises.
- System seeds match with `entry_matches_any` (the `findAtomForPackage` equivalent), so `*`-prefixed profile atoms work; slot-qualified seeds select one slot of a multi-slot cp. Without a resolved config (`resolved_config: None`, all scheduler unit tests before #512) every scope but `toolchain` selects nothing, which keeps the `deep` default byte-identical there. The deep walk follows `deps` forward edges plus fallback edges: `i` depends on `d` exactly when `i`'s cp sits in `d`'s `required_by` with no `deps` record on `i`'s side (the dispatch loop's own inversion) — the first version walked `i`'s dependents instead, caught by `deep_scope_follows_required_by_fallback_edges` before it shipped. Two slots of one cp used to collapse onto one member (cp-keyed map plus cp-level fallback); `deep_scope_keeps_two_slots_of_one_cp_apart` pins the per-node walk.

## fn cyclic_nodes
- Portage `_emerge/Scheduler.py`, `_find_cyclic_nodes` (commit `3d677b7b2`, bug 452172): Tarjan over the blocking edges, members are components of more than one node (a lone self-edge is not a cycle). Reuses the `depclean` Tarjan, now `pub` for exactly this.

## fn post_edge_blocks, fn blocking_deps
- Portage `_emerge/DepPrioritySatisfiedRange.py`, `ignore_medium_post` (`_ignore_runtime_post`): inside a cycle a pure post-merge (`PDEPEND`) edge no longer gates the build; anything with a buildtime or runtime side still does, as do `required_by`-only fallback edges (runtime). Acyclic graphs are untouched, so dispatch there is byte-identical. Before this, a merge cycle dispatched nothing at all (every build waited on an unstarted merge): `emerge cycA cycB` printed the list and merged nothing with rc 0; now `schedcycb` merges before `schedcyca`, like the oracle shape `09-cycle`.

## review follow-ups (read-only review of #512)
- M2: the `required_by` fallback counts any edge as runtime, where Portage's `ignore_priority` would exclude a buildtime-only one. Safe direction (over-barriers), and a fallback with a real buildtime-only reverse edge needs a `deps` record to go missing first; fixtures always carry records.
- M5 (fixed): `merge_wait_set` keyed by `(category, package)`, so two slots of one cp resolved the deep walk to one member. Fixed by entry-keyed resolution (see `RuntimeIndex` above); the scheduler's own `cp_to_idx` keeps the same aliasing, as does dispatch.
- Uninstall-outcome entries are not filtered from the deep walk (Portage skips `operation == "uninstall"` children): no fixture covers it, left as is.
