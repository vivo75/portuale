# Recurring issues (from LLM/what-this-proves.md + backlog)

> Source: `LLM/what-this-proves.md` (19,674 lines) + `LLM/backlog-tasks.md`
> (771 lines, 373 items) + `LLM/scope-backlog.md` (1007 lines).
> Method: parallel subagents, `grep -n` + `python3` counts, context windows.
> This file lives in `helpers/` until the current batch lands, then moves to `LLM/`.

## 1. Silent-drop parity bugs — dominant family (~40 core, 285 visited/dedupe)

Same failure mode, different layer: real dep treated as "not a dep".
No entry, no `NoVisibleCandidate`, no warning.

* Slot ops `foo:0=` (`what-this-proves.md:61-66`), USE deps (`:74-77`),
  slot visited-set (`:396`), sub-slot feed (`:1229`),
  blockers `!!` (`:1473`), `package.provided` (`:8504`),
  nested fragments (`:17303`), `[use]`-unsat (`:8461`).
* Choke: `crates/portage-repo/src/resolve/pass/prepare_atom.rs:48-52`
  → `note_unparsed_dep_token` + `Break`; `globals.rs:123-142` debug-only log;
  `queue.rs:215-304` enqueue-everything / pop-drops split.
* Invariant: "never silently wrong" (`code-notes/portage-use-reduce/lib.md:77,113`).

## 2. Scope-cut → follow-up → stale doc (~102 + 53 narrowings)

`since shipped:16`, `Documented cuts/narrowings:17`, `v1 scope cuts:9`,
`Now stale:9`, e.g. `:98,336,448,2924`, `:6406,6891,8248`.
V1 cuts in module docs (`portage-dep/lib.rs`, `portage-repo/lib.rs`,
`ebuild_*.rs`) → later slice closes → old para frozen, correction appended
per header rule `:1-10`.

## 3. New families from harder pass (counts case-insensitive)

* fetch/mirror gaps: 896 — `:3275` `SRC_URI`/Manifest, `mirror://`, `GENTOO_MIRRORS`.
* color/bracket output: 486 — `:5076` `dblink._collision_protect`, `[ebuild N~]`, counters.
* LICENSE/PROPERTIES/RESTRICT: 391 — `:3303` `RESTRICT=fetch` no auto-download.
* repo/masters/overlay scoping: 366 — `repos.conf`/`layout.conf`/`masters`, `::repo` auto-scope.
* CLI surface transcription: 343 — `:744` `emerge_options.rs`, `ebuild_options.rs`, short-bundling.
* blocker/slot-conflict report-only: 330 — `:396` `SlotConflict`, `resolve_blockers`, pretend never touches.
* USE_EXPAND: 108 — `:286` `USE_EXPAND_HIDDEN/_IMPLICIT`, prefix shorthand, `-v` grouping.
* backtracking/no-backtracking: 84 — `:482` single-pass, no backtrack arch.
* EAPI parametrization cuts: 46 — `:8472` `allows_package_provided`, `eapi=None` default.
* merge-order: 46 — `:1530` `topological_merge_order`, discovery rank, Kahn walk.
* faithful quirk ports: 23 — `:38` PMS ambiguous `foo-bar-2`, `${VAR}` USE exclusion.
* user-confirmed scope: 20 — `:225` explicit cut before implementing.
* wrong-reason/misread: 12 — `:2377` silently right for wrong reason.
* report-only/pretend: 9 — `:415` calculate-and-show, never merge.

## 4. Backlog recurrences

* Residue → new number chains: ~140 `residue`, 43 `filed from #` —
  e.g. #426→#437, #441→#473/#474/#475, #482→#483-486.
* Oracle-grounded parity: ~132 `oracle`, 77 `parity|divergence|drift` —
  expected value from Portage source/live probe, never second copy.
* Wrong-premise corrections: ~27 (+60 w/ stale) — e.g. #408, #438, #440, #449, #453.
* Slot/backtrack trajectory: ~102 — largest open cluster #477-482, refs #212/#213/#243/#257.
* Corpus-bless + paired-commit hygiene: ~33 — bless rides pmtest commit, e.g. #453 (340 entries).
* Merge-path gate + perf: ~61 — glibc+bash test merge required; #441 S2 measured, S3 cache built→REMOVED→#474.

## Prevention checklist

1. Single choke: all skips via `note_unparsed_dep_token`; log in release; assert zero in L0 + contract; `output_invariants.py` requires every token accounted.
2. Parse + resolve pair: grammar slice needs `RDEPEND`-using fixture + resolved vs `NoVisibleCandidate` assert; `python3 -c "from portage.dep import Atom"` probe first.
3. Candidate-feed check: new `Atom` field → `Candidate` carriage (`repo/sub_slot/IUSE`); unsatisfiable variant proves real matching.
4. Visited exact-text; two slots → two entries; `||` fallback keeps all alts.
5. Docs: cuts only in module doc + forward link; `fact_coverage.py`; `**Now stale**`, never rewrite.
6. New: EAPI/masters/USE_EXPAND/fetch/merge-order slices need oracle capture + corpus-bless + merge-path gate (glibc+bash) where touched.

## 5. Alternative-method squeeze (2026-10-22, +15 min)

* Coverage audit (grep `parse_atom` vs `note_unparsed_dep_token`): only
  `resolve/pass/prepare_atom.rs:50` counts. Five filters silently drop without
  counting — `slot_operator/bind.rs:160` (`is_none_or`), `feedback/solvable_slot_conflicts.rs:35`,
  `resolve/root_deps.rs:147` (`is_some_and` filter), `resolve/rev_dep.rs:121`,
  `resolve/graph_lookup.rs:275`. Fix: route all through counter or prove filter is
  intentionally narrow (blocker-only) with unit test.
* Orphan-cut list (grep `Still deferred|remains open|not yet attempted`):
  `:760` `world_sets @`-refs in depclean-args mode, `:1577` `package.use` remains open,
  `:2804` global-config value, `:7000` `KNOWN, DOCUMENTED SCOPE CUT (still open)`.
  These are the cuts that never got a follow-up — prioritize over new features.
* Live OPEN frontier (`backlog-tasks.md`, 63 `OPEN (filed` hits, ~15 true OPEN):
  solver-pubgrub gap cluster `#430,#431`, `@world` divergence `#477-#486`
  (trajectory, mask_built, uninstall-block, binary-vs-ebuild), porting `#442`
  (3.0.82.2→3.0.83), plan-cache `#473,#475`. The recurring bug moved from
  atom-grammar to solver-trajectory — same "report, don't enforce" + backtrack shape.
