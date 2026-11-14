# crates/portage-world/src/order.rs
`--sort depend` order of the plan rows: each slot after the slots it needs.

## fn depend_order (#743)

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
  a genuinely unserializable cycle (no static order satisfies the checker's
  strict usability model; that needs a rebuild-twice, charter property 2a,
  not implemented): pre-fix `cycle-noted`, post-fix one strict-model
  `build-order` failure naming the cut runtime edge's own need. The fix still
  produces the best static order (provider first, every build edge kept); the
  residual is checker strictness on a true cycle, not a missed edge.
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
- Three outcomes per cycle (`CycleCut`): cut the first cuttable runtime edge;
  cut the closing back edge itself when only it is cuttable, and keep
  scanning (stopping there left every later cycle to the walk below, whose
  implicit cuts follow no rule — same #746 perl-cluster symptom); or, when
  nothing is cuttable, rebuild the back-edge consumer a second time
  (charter 2a).
- The breaker loop schedules the first cycle's consumer, removes its back
  edge and repeats until the graph is acyclic; a node is scheduled at most
  once (charter 2a builds twice, not more), and its second copy lands after
  the last member across all its cycles. Termination: every iteration
  removes one edge. Determinism: name-order walk, sorted member sets, no
  hash iteration in any choice.

## fn close_usability_gaps (#745, phase 2)

- After the cut order (with §2.2 breakers) exists, walk it in merge order
  and duplicate the first package whose build-time need chain is unmet,
  at the first later position where the closure becomes satisfied; repeat.
  At most twice per node (a repeat consumer only widens the member set),
  so the loop terminates. Deterministic: first in merge order wins, edges
  in name order, positions are the first satisfied ones.
- `chain_unmet`/`usable_unmet` mirror the checker's `_Moment.gap`: only
  `DEPEND`/`BDEPEND`/`IDEPEND` out-edges constrain (a package's own
  `RDEPEND` needs are satisfiable by the end); usability means every
  package in the `RDEPEND` closure is installed or merged before the
  position; an in-progress node counts as usable (same cycle guard as the
  checker); an unmet need counts only when its provider is itself merged
  (a kept provider's unmet need was already broken before the plan) and
  only when the need can be met at all (installed or a row). The
  `installed` set comes from `World.slots` (`installed.is_some()`); the
  `rows` set decides the provider test and caps the duplicates.
- `ordered()` filters the cut order to changed rows first, then runs the
  closure walk on rows only, and maps names to `Row { slot, pass }` by
  occurrence count — so breaker and usability duplicates share the
  `built_twice` machinery without a second code path.

## fn depend_order (#745, #746)

- Returns the node names with each breaker twice: the walk itself
  (`unordered`) still lists every node once (its `done` set keeps the
  traversal acyclic); the second copy is inserted afterwards, after the last
  cycle member. Only `Order::Depend` produces the pair: `alpha` and
  `resolver` print each slot once with `Pass::First`.
- Bed effect (live-verified, fixed image, seeded `@world` as in
  `l0-health.sh`): the automake finding is gone (`Locale-gettext` #24
  before `automake` #25, `perl-5.44` stays early at #3) and the plan is fully
  checker-clean (0 findings, 0 failures with the container VDB), with 15
  second builds of the same version where the walk genuinely finds no
  cuttable edge (the python toolchain and C library rebuild clusters).
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
  a genuinely unserializable cycle (no static order satisfies the checker's
  strict usability model; that needs a rebuild-twice, charter property 2a,
  not implemented): pre-fix `cycle-noted`, post-fix one strict-model
  `build-order` failure naming the cut runtime edge's own need. The fix still
  produces the best static order (provider first, every build edge kept); the
  residual is checker strictness on a true cycle, not a missed edge.
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
