# crates/portage-world/src/order.rs
`--sort depend` order of the plan rows: each slot after the slots it needs.

## fn depend_order (#743)

- `ordering_edges` drops `PDEPEND`-only edges (PMS: a post dependency imposes
  no order); `depend_order` serializes the rest depth-first in name order
  (deterministic run to run). A cycle is cut at the first runtime-only edge
  in cycle order from the ancestor the back edge points at; only when the
  cycle has no runtime-only edge at all (or the back edge is its only one) is
  the back edge cut, as before. A cycle with only build-time edges is genuine:
  no cut is chosen, the walk orders it implicitly, and `build_time_cycles`
  reports it for the checker to fail (backlog #749: reported, never a
  second build). A cycle with a runtime edge that is uncuttable only because
  of the build-used rule is ordered by the walk and NOT reported (#771, shape
  `w764d*`: `build_time_cycles` sees build edges only). Cutting a runtime edge keeps every
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
  `RDEPEND` (the owner's ruling of 2026-11-15, measured in #764 below). The
  earlier rationale ("a spurious second build",
  `a_cycle_with_a_cuttable_runtime_edge_needs_no_second_build`) is stale:
  #749 deleted both the second build and that test.
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
  evidence `LLM/evidence/02.751/`): on the amended R/B/R probe (kept
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

## fn the_mixed_cycle_cut_is_the_order_the_closure_walks

- Charter property 2 (backlog #745): the middle link — `depend_order`
  on the mixed `R/B/R` `w745` graph yields the cut
  `[prov, virt, user]`, which the literal checker passes with no
  finding (every build atom is satisfied at start).

## fn a_cycle_with_a_cuttable_runtime_edge_lists_each_node_once

- Charter property 2: a cycle with a cuttable runtime-only edge is cut,
  so every node is still listed once (backlog #743).

## fn the_virtual_provider_edge_is_the_only_cuttable_edge

- Charter property 2 (PMS 8.1; backlog #751, plan `02.751` §6-A): a
  kept `virtual/*` middle confers no build-time status, so with `prov`
  build-time-used the cycle's only cuttable edge is the virtual's own
  provider edge — cut as the back edge once the genuine `base <-> prov`
  pair is left to the implicit walk (`LLM/evidence/02.751/`, once cited as `docs/evidence/02.751/`).
  Theoretical, pinned, not proven safe: no live shape exhibits a
  consumer building against a broken installed provider.
- The first cycle the walk meets is the genuine `base <-> prov`
  pair, with no cuttable edge: the cut loop leaves it alone (the
  walk orders it implicitly and the checker fails it — backlog
  #749: reported, never a second build) and cuts the remaining
  cycle at the virtual's provider edge instead — the back edge,
  and the only cuttable one. It is not a build-time cycle: the
  `prov -> base` half is `RDEPEND`, so the report reads nothing.

## fn a_runtime_edge_that_carries_a_build_time_need_is_not_cut

- Charter property 2 (PMS 8.1: a build tool must be usable, so its
  `RDEPEND` closure constrains the order): the cut spares a runtime-only
  edge out of a package the plan builds with, and cuts the next
  runtime-only edge instead, so the repaired provider still comes first
  (backlog #746: `help2man -> Locale-gettext`, not `automake -> help2man`).

## fn an_all_build_time_cycle_is_ordered_once_and_reported

- Charter property 2a (backlog #749): a pure build-time cycle has no
  cuttable edge, so no cut is chosen for it — the walk orders it
  implicitly (here `y` before `x`, the walk's name order) and
  `build_time_cycles` reports the pair, which the checker fails.
  Nothing is ever listed twice.

## fn build_time_cycles

- The build-time cycles of `edges`: strongly connected components over
  build-kind edges only (`build_mask`; an edge without kind information
  counts as build-time, as before), members in walk order, deterministic.
  Empty means every build need is serializable; non-empty is genuine: the
  bootstrap choice reads this (the plan-world report line no longer does:
  it is a check of the produced order, `late.rs`), and the checker fails it
  (charter 2a: a cycle no order satisfies is reported, never papered over
  by building a package twice).

## fn is_cuttable_runtime_edge

- Whether the runtime-only edge can be cut to serialize a cycle: an edge
  out of a package the plan builds with carries a build-time need in fact
  (backlog #746), so cutting it would build against an unusable package.

## fn build_time_used

- The nodes the plan builds with: the targets of a `DEPEND`, `BDEPEND` or
  `IDEPEND` edge, except `virtual/*` targets. PMS 8.1 makes such a package
  usable, so its `RDEPEND` closure constrains the order too. Excluding
  virtuals is a heuristic that keeps the perl/libcrypt rebuild cluster
  ordered, not PMS soundness: `BDEPEND virtual/foo` makes the provider a
  build input in fact, so a kept virtual's provider edge stays cuttable
  even when the installed provider is broken. The known hole (and the real
  fix) is filed as #751. One level only (#764, owner ruling 2026-11-15:
  documented limitation). Measured on 2026-11-16 with the 764 fixtures:
  a tool's depth-2 runtime dependency can merge after its consumer (`w764a*`,
  V2: `am, at, ab, ax, au`, `au` late; literal checker clean, the strict PMS
  8.1 model flags it). Propagating the status one level (k=2) only moves the
  late package one level deeper (V2 `au, am, at, ab, ax`, `ax` late);
  propagating it along the whole runtime closure leaves cycles with no
  cuttable edge: V2 happens to come out clean (`ax, au, am, at, ab`) but V1
  (`w764c*`, `cu` genuinely needs `cb`) becomes a literal `build-order`
  failure with no report line (`cu, cm, ct, cb`). A cut preference among the
  cuttable edges (target nearest the tool) fixes V2 and leaves V1 alone; it
  changes plan order, so it is the owner's call. On the host fast plan (349
  rows) and the bed's (`FAST_world.json`, 172 rows) there is no #764-shaped
  row: the strict model's 49 host rows are all the openssl `:0/3=` binding
  move. The bed plan has 0 strict rows. Evidence: `LLM/evidence/02.764/`.

## fn depend_order

- every run. A cycle is cut at the first cuttable runtime-only edge (a
  runtime edge out of a package the plan builds with still orders, so it is
  not cuttable); a cycle with no cuttable edge is genuine: no cut is
  chosen for it, the walk below orders it implicitly, and
  [`build_time_cycles`] reports it for the checker to fail. `kinds`
  carries the `*DEPEND` variables behind each edge, as bits of
  [`crate::DEP_KEYS`]; an edge without one counts as build-time.

## fn (see source)

- (`DEPEND`, `BDEPEND`, `RDEPEND`, `IDEPEND`). A `PDEPEND`-only edge
  puts no constraint on the order (PMS). A cycle is cut at a cuttable
  runtime-only edge when it has one (a runtime edge out of a package
  the plan builds with still orders, PMS 8.1); a cycle with none is
  genuine: the walk below orders it implicitly and
  [`build_time_cycles`] reports it, for the checker to fail (charter
  2a: a cycle no order satisfies is reported, never papered over by
  building a package twice).

## fn w764_a_tool_runtime_dependency_on_a_cut_cycle_lands_after_its_consumer (#764)

- Pins shape V2 by `depend_order` on a literal graph: `am, at, ab, ax, au`. The
  order is derived by hand from the cut rule (plan `LLM/02.764-build-closure.opus.md`
  sec. 1), not from a run; the pytest pin `test_764_a_tool_runtime_dependency_on_a_cut_cycle_lands_after_its_consumer`
  checks the same shape end to end over the committed `w764a*` fixtures. Proven
  not vacuous by the `k2` mutation (`LLM/evidence/02.764/after/red.txt`).

## fn w764_an_unavoidable_late_tool_dependency_is_literal_legal (#764)

- Shape V1, `cm, ct, cb, cu`: `cu` after `cb` is PMS-genuine (`cu -B-> cb`), and
  the one-level cut gives it. Propagating build-used status down the runtime
  closure would turn it into a literal `build-order` failure.

## fn w764_an_uncuttable_runtime_cycle_is_left_to_the_walk (#764)

- Shape V3, OBSERVED DEFECT pinned exactly (residue #771, owner decision of
  2026-11-16: not fixed in #764): `runtime_cut` is `None`, `build_time_cycles`
  is empty, the walk gives `du, dt, db`, which breaks the literal edge
  `du -B-> db`. The fix (fall back to the first runtime-only edge, about three
  lines) would also move `the_virtual_provider_edge_is_the_only_cuttable_edge`.
  Proven by the `v3fix` mutation.

## fn build_path
- The shortest path `from -> ... -> to` over build-kind edges, breadth first,
  neighbours in name order (an edge without kind information counts as build,
  as in `build_time_cycles`). The plan-world cycle line names a cycle only
  along such a path, never a Tarjan member list (a discovery order, not a
  path: the 11- and 26-member lines of the host and bed plans were member
  lists, and so was the 186-member emptytree one).

## fn drop_path
- The bootstrap announcement names the cycle the reduction really breaks: the
  first edge is one `Bootstrap::drops` removes, then the shortest build path
  back. A plain shortest cycle through the boot node could leave that edge out
  (`a -> b -> c -> a` with a chord `a -> c`, `a -> b` dropped: `a, b, c, a`,
  not `a, c, a`); pinned by `a_bootstrap_path_starts_with_a_dropped_edge`.

## fn cycle_cut, build_time_used, depend_order (comment history)
- The ten backlog numbers `466b1a2e` put in comments live here: shape V3 and
  the fallback to the first runtime-only edge are #771 (owner decision
  2026-11-16, Q1: it ships only if a measurement on the host and bed plans
  moves no bed row and adds no literal failure; it did not, evidence
  `LLM/evidence/02.771-772/before/v3fix-verdict.txt`, so `cycle_cut` is
  unchanged and #771 stays open); the one-level build-time closure is #764
  (owner ruling 2026-11-15: documented limitation, charter 2a is literal);
  `virtual/*` is left out of `build_time_used` for the perl cluster (#751,
  a heuristic, not PMS soundness). Since #772 a V3 order is not silent: the
  plan-world check reports the late `du -> db` need ("no build cycle joins
  them") because no build path joins the two.
