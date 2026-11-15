# crates/portage-world/src/order.rs
`--sort depend` order of the plan rows: each slot after the slots it needs.

## fn depend_order (#743)

- `ordering_edges` drops `PDEPEND`-only edges (PMS: a post dependency imposes
  no order); `depend_order` serializes the rest depth-first in name order
  (deterministic run to run). A cycle is cut at the first runtime-only edge
  in cycle order from the ancestor the back edge points at; only when the
  cycle has no runtime-only edge at all (or the back edge is its only one) is
  the back edge cut, as before. A cycle with no cuttable edge is genuine:
  no cut is chosen, the walk orders it implicitly, and `build_time_cycles`
  reports it for the checker to fail (backlog #749: reported, never a
  second build). Cutting a runtime edge keeps every
  build-time edge, so a build dependency is still merged before the package
  needing it (PMS `dependencies.tex`, `sec:dependency-classes`); a runtime
  dependency only needs its target merged by the end (charter property 2, and
  the health checker's own rule that a runtime cycle counts as usable).
- Why the cut matters: in the L0 health container the fast `@world` plan
  merged `dev-lang/perl-5.44.0` at #45 after `virtual/perl-ExtUtils-MakeMaker`
  (BDEPEND of `dev-perl/PerlIO-utf8_strict-0.10.0`) at #18, whose
  `RDEPEND || ( =dev-lang/perl-5.44* ~perl-core/ExtUtils-MakeMaker-7.780.0 )`
  is met only by the new perl. The `||` resolves to the perl branch
  (`branch_preference`: installed `cat/pkg` beats available, ties go to the
  first), so the `virtual -> perl` edge exists (hypothesis (a) excluded on a
  fixture: the edges print `virt -> prov` with the `RDEPEND` bit); the order
  came from cutting the cycle at the back edge instead of at a runtime edge
  (hypothesis (b)). The checker's verdict is `build-order` (not
  `cycle-noted`) because its graph has no cycle: the portage cycle runs
  through a package the plan does not merge (a kept one), which the checker
  never orders. Fixture model `w743prov -> w743zeb (kept) -> w743virt
  -RDEPEND-> w743prov` with `w743user -BDEPEND-> w743virt`: pre-fix order
  `virt, prov, user`, post-fix `prov, virt, user`, 0 findings either way on
  the shape (the consumer lands after the provider in the miniature).
- Fixture caveat recorded: with the cycle closed directly through the
  consumer (`prov -RDEPEND-> user`, plan's literal fixture (b)) the three form
  a runtime-closed cycle, not an unserializable one: the literal checker
  passes it with zero findings (backlog #749 — the strict usability model
  that failed it is gone). The fix still produces the best static order
  (provider first, every build edge kept).
- `World::edge_kinds` already holds the `*DEPEND` bits behind each edge, so
  the cut needs no second resolution pass. `build_mask` covers
  `DEPEND`/`BDEPEND`/`IDEPEND`; an edge without kind information (unit tests
  only) counts as build-time, preserving the old behaviour there. Cutting
  opens further cycles, so `depend_order` clones the edge set and rescans
  until no runtime cut remains; each rescan removes one edge, so it
  terminates, and every scan is in name order, so the result is deterministic.
- Host effect (live-verified): 25 runtime cuts on the host `--plan-world
  --sort depend` plan, same 249-row set, 0 checker findings before and after;
  21 head rows re-serialized (provider-first in runtime cycles through kept
  `systemd`/`perl`/`python`, e.g. `zlib` after `libarchive`).

## fn runtime_cut (#745, #746)

- A runtime-only edge is cuttable unless its source is used at build time
  (`is_cuttable_runtime_edge`, next to `is_runtime_edge`): PMS 8.1 makes a
  build tool usable, so its `RDEPEND` closure constrains the order. The
  "used at build time" set is computed once per `depend_order` call, as the
  targets of a build-kind edge, and deliberately does not propagate through
  `RDEPEND`: propagating it would leave the #746-shaped graph with no
  cuttable edge and force a spurious second build, contradicting the
  `a_cycle_with_a_cuttable_runtime_edge_needs_no_second_build` shape.
- A `virtual/*` target does not confer build-time status. That exclusion is
  a heuristic that keeps the perl/libcrypt rebuild cluster ordered, not PMS
  soundness: `BDEPEND virtual/foo` makes the provider a build input in fact
  (PMS 8.1 usability), so a kept virtual's provider edge stays cuttable even
  when the installed provider is broken. Without the exclusion, every
  provider edge of the bed's perl/libcrypt region was uncuttable and dragged
  the whole perl rebuild cluster past the `perl-5.44` upgrade the rebuilds
  are for (10 new `build-order` findings on a 176-row bed plan). The known
  hole (and the real fix) is filed as #751.
- Pinned 2026-11-14 (plan `02.751-kept-virtual-cut` §6-A,
  evidence `docs/evidence/02.751/`): on the amended R/B/R probe (kept
  `virtual/*` middle, `base BDEPEND prov` so `prov` is build-time-used,
  stale-provider rebuild) the cut lands on the virtual's provider edge
  (`BackEdge(virt, prov)`) — the exclusion routing the cut — with rows
  `[user-1 N, prov-2 U, base-2 U]` and a checker verdict of zero
  non-`pre_existing` failures and zero notes on both the `virtual/` and
  the `dev-libs/`-middle variants. The consumer builds while the
  installed provider copy is still usable, so the broken-at-build-time
  shape is unexhibited: theoretical, pinned, not proven safe.
- Three outcomes per cycle (`CycleCut`): cut the first cuttable runtime edge;
  cut the closing back edge itself when only it is cuttable, and keep
  scanning (stopping there left every later cycle to the walk below, whose
  implicit cuts follow no rule — same #746 perl-cluster symptom); when
  nothing is cuttable the cycle is genuine: no cut is chosen (backlog #749),
  the walk orders it implicitly, and `build_time_cycles` reports it for
  the checker to fail. Deleted 2026-11-14 (plan `02.749-resolver-2a`):
  the `CycleCut::Rebuild` third outcome and the breaker loop that built
  the back-edge consumer twice — the owner ruled the second build illegal
  (charter property 2 literal), so the duplicate and the strict usability
  closure went with it; the cut rule above is unchanged.

## fn close_usability_gaps (#745, phase 2) — deleted 2026-11-14

- Plan `02.749-resolver-2a`: the owner ruled the positional duplicate
  illegal (charter property 2 literal — a build dep must be installed,
  its runtime deps only present by the end), so the whole closure walk
  went with it: `close_usability_gaps`, `struct Usability`, `chain_unmet`,
  `usable_unmet`, `build_deps`, `is_available` (former `order.rs:391–547`),
  the `CycleCut::Rebuild` outcome, the `second`/`members_of` insertion
  loop, and `struct Row` + `enum Pass` with the `built_twice` machinery
  (`plan_world.rs` entry/JSON/summary: `Row { slot, pass }`, `Pass::First`
  vs `Pass::Rebuild`, `Pass::Rebuild` carrying `"built_twice": 2` and no
  `from_version`). What it did (history): after the
  cut order existed it walked it in merge order and duplicated the first
  package whose build-time need chain stayed unmet, at the first later
  position where the closure held (mirroring the checker's `_Moment.gap`);
  availability was slot-node name equality, a name-level approximation of
  atom satisfaction, with the known `:=` hole (a same-`cp:slot` provider
  merged earlier satisfied the walk while the checker's atom stayed
  unsatisfied). Disjunctions still reach the walk resolved — `World::edges`
  records the slots the first pass followed — so only the chosen arm is an
  edge. The checker side went the same day: `_unusable`,
  `_second_build`, `_Moment`/`gap`, `_components`/Tarjan, the
  same-component excuse, the identical-cpv collapse. The genuine cycle
  still fails — the same-component excuse went with the deletion, so a
  runtime back-edge never turns a merge-it-first case into a "cycle"
  (`reaches` runs on the build-only `_build_graph`).

## fn depend_order (#745, #746)

- Returns the node names, each once: the walk itself (`unordered`) lists
  every node once (its `done` set keeps the traversal acyclic), and since
  2026-11-14 (plan `02.749-resolver-2a`) no second copy is ever inserted —
  a slot is never listed twice (the `Row`/`Pass` pair went with the
  deletion; `alpha`, `depend` and `resolver` all print each slot once).
- Bed effect (live-verified, fixed image, seeded `@world` as in
  `l0-health.sh`): the automake finding is gone (`Locale-gettext` #24
  before `automake` #25, `perl-5.44` stays early at #3) and the plan is fully
  checker-clean (0 findings, 0 failures with the container VDB). A cycle
  with no cuttable edge is ordered implicitly and reported for the
  checker to fail, not built twice (the S3 beds re-verify the counts).
- `ordering_edges` drops `PDEPEND`-only edges (PMS: a post dependency imposes
  no order); `depend_order` serializes the rest depth-first in name order
  (deterministic run to run). A cycle is cut at the first runtime-only edge
  in cycle order from the ancestor the back edge points at; only when the
  cycle has no runtime-only edge at all (or the back edge is its only one) is
  the back edge cut, as before. Cutting a runtime edge keeps every
  build-time edge, so a build dependency is still merged before the package
  needing it (PMS `dependencies.tex`, `sec:dependency-classes`); a runtime
  dependency only needs its target merged by the end (charter property 2, and
  the health checker's own rule that a runtime cycle counts as usable).
- Why the cut matters: in the L0 health container the fast `@world` plan
  merged `dev-lang/perl-5.44.0` at #45 after `virtual/perl-ExtUtils-MakeMaker`
  (BDEPEND of `dev-perl/PerlIO-utf8_strict-0.10.0`) at #18, whose
  `RDEPEND || ( =dev-lang/perl-5.44* ~perl-core/ExtUtils-MakeMaker-7.780.0 )`
  is met only by the new perl. The `||` resolves to the perl branch
  (`branch_preference`: installed `cat/pkg` beats available, ties go to the
  first), so the `virtual -> perl` edge exists (hypothesis (a) excluded on a
  fixture: the edges print `virt -> prov` with the `RDEPEND` bit); the order
  came from cutting the cycle at the back edge instead of at a runtime edge
  (hypothesis (b)). The checker's verdict is `build-order` (not
  `cycle-noted`) because its graph has no cycle: the portage cycle runs
  through a package the plan does not merge (a kept one), which the checker
  never orders. Fixture model `w743prov -> w743zeb (kept) -> w743virt
  -RDEPEND-> w743prov` with `w743user -BDEPEND-> w743virt`: pre-fix order
  `virt, prov, user`, post-fix `prov, virt, user`, 0 findings either way on
  the shape (the consumer lands after the provider in the miniature).
- Fixture caveat recorded: with the cycle closed directly through the
  consumer (`prov -RDEPEND-> user`, plan's literal fixture (b)) the three form
  a runtime-closed cycle, not an unserializable one: the literal checker
  passes it with zero findings (backlog #749 — the strict usability model
  that failed it is gone). The fix still produces the best static order
  (provider first, every build edge kept).
- `World::edge_kinds` already holds the `*DEPEND` bits behind each edge, so
  the cut needs no second resolution pass. `build_mask` covers
  `DEPEND`/`BDEPEND`/`IDEPEND`; an edge without kind information (unit tests
  only) counts as build-time, preserving the old behaviour there. Cutting
  opens further cycles, so `depend_order` clones the edge set and rescans
  until no runtime cut remains; each rescan removes one edge, so it
  terminates, and every scan is in name order, so the result is deterministic.
- Host effect (live-verified): 25 runtime cuts on the host `--plan-world
  --sort depend` plan, same 249-row set, 0 checker findings before and after;
  21 head rows re-serialized (provider-first in runtime cycles through kept
  `systemd`/`perl`/`python`, e.g. `zlib` after `libarchive`).

## fn build_mask
- Now `portage_use_reduce::BUILD_KIND_MASK` (single source of truth with the
  bootstrap rule, which reads kinds maps in this layout). No behaviour
  change.

## fn runtime_cut (#760)
- Returns the one edge to cut. It used to return `CycleCut::Edge` or `CycleCut::BackEdge`, handled identically: cutting the back edge when it is the only cuttable one must not stop the scan, or every later cycle is left to the walk's implicit cuts, which no rule chooses (backlog #746: the perl cluster moved late).
