# crates/portage-repo/src/resolve/bootstrap.rs
Charter 2a bootstrap (backlog #753): the resolver-side synthesis.

## Charter 2a bootstrap (backlog #753)

- The selection rule lives in the `portage-use-reduce` leaf
  (`bootstrap_reduction`, shared with the fast plan); this module only feeds
  it the resolver's cycles + guard map, synthesizes the boot row after the
  sort, and rewrites the sorted vec. One writer, no duplicated rule, no new
  dependency edge.
- Node namespace is merge-bound `cat/pkg-ver` on both ends (the
  `find_hard_cycles` output needs no conversion). Deps with no merge-bound
  instance end the DFS there: an already-installed package is not rebuilt
  by the boot, so it cannot carry a build-time need for a member.
- `guard_map` is keyed `(owner_cpv, dep_cpv)` via the same cp lookup as the
  edges (a dep-`cp`-keyed map silently missed every edge — caught by the
  sequence test). `edge_drops_without_flag` is one shared predicate for the
  map and the `deps` filter (universal over occurrences, so an unguarded
  `||` alternative keeps the edge and STOP-2 refuses instead of dropping a
  live need). A `[flag?]` USE-conditional atom counts as unguarded: the
  single-flag boot shape cannot express it, so such an edge is never
  reduced (conservative refuse, not a wrong drop).
- `boot_row` clones the definitive entry variant-for-variant (the version
  lives in the `PretendOutcome`; slot-operator targets are read off the
  candidate, so there is no version/bindings field to remap), flips the
  reduced flag in `use_flags_display`, rebuilds both expand displays with
  the same installed/forced inputs the walk's display pass uses, drops the
  flag-guarded build-time `DepEdge`s to members, and re-points `dep_input`
  at the reduced evaluation (the health checker reads the boot row's build
  graph from it). `blockers` ride along as filed under the declared USE
  (review m2: no fixture conditions a blocker on the reduced flag;
  re-evaluating them under boot USE is open work, not a known divergence).
  `required_by` is copied (finalised per `(category,
  package)` before synthesis, so `--tree` keeps the node) and `discovery`
  is stamped with the final index (collisions with another row's pre-sort
  position are benign: the sort already ran).
- Rewrite rule on the sorted vec: the boot row takes the earliest of the
  boot slot's own position and its transitive dependents (it must precede
  everything needing it — same-index replace on the owner's shape,
  strictly safer otherwise), and the definitive entry moves to immediately
  after the last entry that transitively build-depends on the boot slot
  (reduced build graph). Dependents are computed over the reduced edges
  (`graph_view_reaches`), not the leaf DFS.
- `maybe_bootstrap` refuses (keeps today's shape) on: `--buildpkgonly`
  (a bootstrap *is* merges), no reduction (`None`), a boot closure that
  still reaches a member, more than 3 rows for the affected slots, a boot
  slot with no recorded USE to reduce from, or any remaining build-time
  `DepEdge` from the boot slot to a member on the synthesized row itself.
- The hard-cycle report reads the reduced edge set: the dropped walk edges
  are softened by endpoint match (owner root view + boot cp + member cp,
  hard-only keys only — a runtime arm would have set soft, and STOP-2
  already refused any surviving build-time arm, so every softened key is
  the dropped arm), then `build_cycle_display` re-runs and every field is
  recomputed. Any cycle the reduction does not break still fails exactly as
  today; cross-root same-cp keys are untouched.
- `--resume` needs no new machinery: a persisted boot row loses its marker
  when rebuilt from bare `cat/pkg-ver`, so an interrupted bootstrap resumed
  this way replays unmarked rows and the checker's unmarked-duplicate rule
  fails it as `slot` — a safe failure, never a silent wrong merge.
