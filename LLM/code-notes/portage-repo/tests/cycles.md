# crates/portage-repo/src/tests/cycles.rs
Cycle detector and suggestion tests: restart/hard/virtual cycles, per-edge labels.

## fn find_restart_cycles_reports_a_hard_two_cycle
- Backlog #221: Portage `_serialize_tasks`' no-leaf restart trigger (`_emerge/depgraph.py:10264-10296`). Order convention matches `find_hard_cycles` (each element depends on the next, last wraps to first).

## fn find_restart_cycles_reports_a_runtime_closing_cycle
- The bug-705986/icedtea shape: the closing edge is runtime-only (an RDEPEND `||` branch). Only the buildtime half reaches the walk's hard arm; the runtime half is a soft map entry at most.

## fn find_restart_cycles_ignores_runtime_only_and_acyclic_graphs
- A pure-runtime two-cycle is group-merged, never restarted (Portage's `find_smallest_cycle` absorbs it): no hard edge, no trigger.

## fn softened_buildtime_edge_hides_from_hard_cycles_but_not_restart_cycles
- Backlog #221 (I3): `sbrA` BDEPENDs+RDEPENDs on `sbrB` while `sbrB` BDEPENDs back. The `(true, true)` map entry keeps `find_hard_cycles` empty but still strands the walk; `assemble_result`'s persisting-ring leg (not the hard leg) renders the failure.

## fn find_hard_cycles_reports_a_build_time_cycle_and_ignores_a_run_time_one
- Pure hard cycle reported starting where Portage's `shortest_cycle[0]` starts (backlog #278): hca earliest-inserted, so the cycle closes through its child hcb (`[hcb, hca]`). Fake root: the #228 slot-op arm never fires and no vdb is consulted.

## fn find_hard_cycles_starts_where_portages_get_cycles_starts
- Backlog #278, expected value from Portage: Portage `circular_dependency_handler` prints `shortest_cycle[0]` (`resolver/circular_dependency.py:76-99`), and `_find_cycles` (`:48-56`) keeps the first strictly-shortest cycle of Portage `digraph.get_cycles` order (earliest-inserted node first), not a lowest-index rotation.
- The #249 variant shape (entries in discovery order, the requested build tool first): hard edges both ways (`u249make -BDEPEND(dev-libs/u249json:0=)-> u249json`, `u249json -BDEPEND(dev-util/u249make)-> u249make`), so Portage 3.0.82.2 prints from `u249json` (`u249json -> u249make -> u249json`; O1 S0 probe). The old lowest-index rotation started at `u249make` instead.

## fn find_hard_cycles_reports_a_true_self_loop_but_not_a_cross_slot_edge
- Backlog #216: the kept `||` branch's atom matching the owner's own merge-bound cpv reports the one-member ring (Portage deadlocks on `go → go` in `_serialize_tasks`), while `=hcs-2.0` against merged `hcs-1.0` is ordinary in Portage's package-keyed digraph. The `oi == di` gate and the atom-match `&&` widening fail here.
- Backlog #216 scope: a self-loop touching an installed instance stays invisible; reporting it would flip an outcome-correct resolve into a spurious circular abort, and the retry that would re-resolve past it is #221's scope.

## fn cycle_restartable_gates_the_retry_to_fresh_branch_self_loops
- Backlog #216: a two-node ring, an inline self-dep, and a self-loop with an installed instance of the owner keep today's report-no-retry path (multi-node rings are #221's scope; the installed cases entangle merge-vs-installed bookkeeping this slice does not re-model). Each gate `&&` fails here.

## fn find_hard_cycles_ignores_a_suppressed_disjunctive_slot_op_branch
- Backlog #228 fix round 1 (review Important-1): Portage's digraph holds the `||` branch the walk selected (Portage `dep_check.py::dep_zapdeps`' `choice_bins`: the first branch, in written order, all of whose atoms resolve in the graph wins). Owner `dev-libs/own` carries RDEPEND `|| ( dev-libs/keb dev-libs/kea:= )`; the walk kept the first (plain run-time) branch while `dev-libs/kea` is merge-bound through its own hard DEPEND back-edge. Pre-fix the unrecorded-edge arm added the suppressed edge and reported a phantom `own -> kea -> own` cycle.

## fn find_hard_cycles_slot_op_gate_requires_the_child_slot_like_portage
- Backlog #228 fix round 1 (review Important-2), expected value from Portage: Portage filters `inst_pkgs` to the child's slot and sub-slot for `slot_operator == "="` (Portage `depgraph.py::_add_pkg_dep_string`).
- Rotation starts at `sdep` since backlog #278 (Portage's `shortest_cycle[0]`).

## fn find_virtual_cycles_reports_self_loop_ring_and_lollipop
- Backlog #193, expected value from Portage `_virt_deps_visible` (`depgraph.py:6208`): upstream `test_virtual_cycle.py` (bug 965570).
- Three-ring members report sorted (Portage sorts the probe-stack packages, `Package.py:830`). Lollipop: Portage reports the whole recursion set (`list(_virt_deps_visible_recursion)`), not just the loop.

## fn find_virtual_cycles_ignores_non_rdepend_and_non_virtual_shapes
- Portage's stack holds virtuals only and the imp expands `RDEPEND` alone (`depgraph.py:6224-6256`): a `virtual/ → non-virtual → virtual/` ring probes clean (an ordinary merge), a `DEPEND` (`key: 3`) loop is never expanded, and an `AlreadyInstalled` virtual is never a start node (not merge-bound). A ring with no virtual at all is ordinary circular handling's shape.

## fn find_virtual_cycles_ignores_a_suppressed_disjunctive_branch
- `O` (virtual, merge-bound): RDEPEND `|| ( virtual/plain virtual/D )`, both merge-bound; the walk keeps the first written branch (Portage `dep_zapdeps` choice_bins, the same `kept_alt_branches` set `build_digraph` collapses to). Without the kept check this walk would follow `O → D → O` and report both. Portage's `dep_check` may probe the suppressed branch during choice evaluation and fail here instead (a documented cut in `find_virtual_cycles`).

## fn circular_dep_solutions_suggests_disabling_the_gating_flag
- Fixture `dev-libs/usecyclea` (IUSE +x) build-depends on `dev-libs/usecycleb` only under `x`; `usecycleb` build-depends back unconditionally.

## fn circular_dep_solutions_applies_the_autounmask_use_overlay
- Backlog #208(b): `=dev-libs/cyc0w-3` needs `dev-libs/cyc0z[bar]`, so the resolve autounmasks `bar` on `cyc0z-3` (IUSE `+foo bar`, full DEPEND in the test). Portage `_pkg_use_enabled` (`_emerge/depgraph.py:7669`) evaluates the suggestion on the autounmasked `USE="bar foo"` while `bar` stays untouchable (`resolver/circular_dependency.py:104-114`), so the only assignment dropping `dev-libs/cyc0y` is `foo` off: `cyc0z-3 (Change USE: -foo)`. Without the overlay `bar` reads off and no assignment drops the atom (the generic "temporarily disabling USE flags" advisory).

## fn circular_dep_solutions_grandparent_conflict_disqualifies_the_suggestion
- Same USE-gated cycle shape (here gpcyclea/gpcycleb), but top-level `dev-libs/gpcyclec` is a "grandparent" with its own hard `dev-libs/gpcyclea[x]` use-dep: `grandparent_use_conflict`'s `ignore` branch fires. `docs/history/find-suggestions-plan.md`'s "no fixture yet" note.

## fn mixed_priority_cycle_reports_per_edge_labels
- Backlog #228, expected value from Portage: `dev-libs/slopcyca -RDEPEND:=-> dev-libs/slopcycb -DEPEND-> dev-libs/slopcyca`. The pre-#228 detector saw only a soft run-time edge and merged silently. Grounded in the n228 probe (Portage 3.0.81.3 rc 1 with `(buildtime)` on the back edge and `(runtime_slot_op)` on the slot-op edge) and a fresh 3.0.82.2 probe printing `slopcycb depends on / slopcyca (buildtime) / slopcycb (runtime_slot_op)`.

## fn circular_dep_solutions_is_empty_for_a_cycle_with_no_conditional_edge
- `extract_affecting_use` finds nothing on unconditional DEPENDs, so there is no suggestion (Portage hits the generic-advisory else).
