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
- `--resume` still rebuilds the entries from bare `cat/pkg-ver`
  (`emerge_build/resume.rs`), so a resumed boot row loses its marker; the
  checker's unmarked-duplicate rule fails that as `slot` (a safe failure).
  Persisting the marker was investigated under #759 and left open: the
  `mtimedb["resume"]` mergelist items are Portage's `[type, root, cpv,
  "merge"]` lists, and #585 pins that shape.
- A boot row is never a second identical row (that is what #749 removed):
  the `portage_use_reduce::BootstrapUse` marker, not the outcome, distinguishes it, and the
  final row of the slot carries the declared USE.
- Candidate eligibility (#759, owner rulings 2026-11-15), applied through
  the `eligible` predicate of `bootstrap_reduction`: the flag must be in
  the declared USE and not forced (`forced_or_masked_flags`, the set
  `boot_display_inputs` already shows); a flag whose removal switches on a
  `!flag?` arm naming a build-time `cp` the declared USE does not
  (`flag_adds_build_dep`, two `use_reduce_flat` evaluations) is refused,
  because the boot row could not carry the new dependency in order. The
  first pass also requires the entry's `REQUIRED_USE` to hold without the
  flag; if that finds nothing a second pass drops that requirement (prefer,
  never refuse: the boot build is replaced at once). Other packages'
  `[flag]` USE deps on the boot package are ignored for the same reason.
- `guard_map`/`edge_drops_without_flag` stay syntactic
  (`conditional_guards`): replacing them with a double `use_reduce`
  evaluation is not equivalent, because the evaluation treats an arm that
  is inactive under the declared USE as absent (cyc0w-3 in
  `circular_dep_solutions_applies_the_autounmask_use_overlay` then
  bootstraps instead of reporting its cycle). That replacement is open.
- Interrupted run (#759): `interrupted_bootstrap_notes` (portuale
  `emerge_build/failure.rs`) reads the installed `USE` of each boot row's
  cpv; one lacking the removed flag means the run stopped before the
  definitive row. `entries_not_merged` judges by `CONTENTS`, so after a
  merged boot row the definitive row of the same cpv counts as merged and
  drops out of the saved resume list.

## fn maybe_bootstrap

- Rewrite rule on the sorted vec: the boot row takes the earliest of
  the boot slot's own position and its transitive dependents (it must
  precede everything needing it), and the definitive entry moves to
  immediately after the last entry that transitively build-depends on
  the boot slot (reduced build graph).
  Every entry that transitively build-depends on the boot slot: the
  boot row precedes the earliest of them, the definitive entry follows
  the latest.

## fn boot_row

- The boot row for the definitive entry with `flag` forced off: the
  entry cloned variant-for-variant (the marker, not the outcome,
  distinguishes the rows), USE displays recomputed with the flag off,
  `deps` the reduced closure (the build-time `DepEdge`s to cycle members
  whose arm the flag guards are gone), `dep_input` re-pointed at the
  reduced evaluation. `blockers` ride along as filed under the declared
  USE (no fixture conditions a blocker on the reduced flag; re-evaluating
  them under boot USE is open work, not a known divergence).

## fn edge_drops_without_flag

- Whether removing `flag` drops every build-time arm of `metadata` naming
  `dep_cp`: every raw atom with that `cp` sits under the flag
  (universally quantified, so an unguarded `||` alternative keeps the edge
  and the STOP-2 re-check below refuses instead of dropping a live need).
  A `[flag?]` USE-conditional atom counts as unguarded: the single-flag
  boot shape cannot express it, so such an edge is never reduced. One
  shared predicate for the guard map and the `deps` filter, so the two
  cannot disagree.
