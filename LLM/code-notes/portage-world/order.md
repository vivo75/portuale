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
