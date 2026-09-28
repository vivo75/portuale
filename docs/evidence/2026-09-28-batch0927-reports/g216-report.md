# G12 (#216) report — the `||` pick that keeps in-graph `>=dev-lang/go` live

Status: DONE (BED-PENDING: oracle + L0 beds are the coordinator's;
see §5)

NOTE ON INTERRUPTION: the previous run of this brief was cut off by a
provider quota outage after completing S0 (fixtures + the one allowed
real-Portage probe + portuale-baseline recording) and starting the S1
design. This run resumed from the uncommitted fixture files and
`/tmp/opencode/g216/probe.{sh,log}` (both intact), re-verified the
baseline, and continued. Nothing committed was redone; the fixtures are
adopted verbatim (md5 verified). No `RATE_LIMIT` budget was spent on a
second probe: rule 13 allows one, the prior run used it, and its
verbatim output is pasted in §2.

## 0. Real behavior (3rdparty/portage 3.0.82.2; probe ran 3.0.81.3)

- Pass-1 `||` choice (`dep_check.py::dep_zapdeps`): no self-exclusion.
  An alternative whose atoms match already-in-graph packages ranks
  `preferred_in_graph` (bin 0) even when the atom names the package
  being resolved. So `go`'s `BDEPEND=|| ( >=dev-lang/go-X
  dev-lang/go-bootstrap )` keeps `>=dev-lang/go` live while `go` is
  in-graph (real adds the package to the digraph in `_add_pkg` before
  walking its deps; the `||` pops off `_dep_disjunctive_stack` later,
  `depgraph.py:3254-3271`).
- The cycle dead-ends `_serialize_tasks` (`depgraph.py:10262-10289`):
  every (node → predecessor) edge lands in the `circular_dependency`
  backtrack map; the next pass demotes the matching `||` branch to
  `other` (`dep_check.py:673-691`, with the `vardb.match(atom)`
  installed exemption). Unsolved cycles (`unsolved_cycle`) skip the
  restart and display.
- `--backtrack=0` (`_allow_backtracking=False`) displays the pass-1
  graph as-is: verbose tree + `Total:` + `* Error: circular
  dependencies:` + rc 1.

## 1. S0 fixtures (pmtest commit; EAPI gate: all EAPI 8)

`app-misc/g216top` RDEPENDs `dev-libs/g216mid` RDEPENDs
`dev-lang/g216comp` whose `BDEPEND=|| ( >=dev-lang/g216comp-1.0
dev-lang/g216boot )` (the plasma-meta/go shape). md5-cache entries
verified (`md5sum` == `_md5_`).

## 2. S0 probe (verbatim; the one rule-13 probe, run by the prior run)

`podman run --rm -v "$PWD/fixtures:/fixtures:ro" -v
"$PWD/differential-test-bed:/TEST:ro" -v
/tmp/opencode/g216/probe.sh:/probe.sh:ro --entrypoint /bin/bash
localhost/test-portuale:latest /probe.sh` (probe.sh stages via
`layers/l0-fixture-oracle/stage.sh`, then `emerge -p --color=n` on the
four cells; `PYTHONHASHSEED=0`). Full log: `/tmp/opencode/g216/probe.log`
(real_portage: Portage 3.0.81.3). Results:

- default `app-misc/g216top`: rc 0, `backtrack: 1/20`, 5 merge rows
  (`g216boot`, `g216comp`, `g216comp to $ROOT`, `g216mid to $ROOT`,
  `g216top to $ROOT`) — the dual `g216comp` rows are the cross-root
  artifact (BDEPEND at ESYSROOT=/ vs RDEPEND at ROOT=$FX), not two
  versions.
- default `dev-lang/g216comp`: rc 0, `backtrack: 1/20`, 3 rows (same
  dual-comp shape).
- `--backtrack=0` both cells: rc 1, verbose tree + `Total: 4` resp. `2`
  + the self block:
  `(dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled for merge)
  depends on` / ` (dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled
  for merge) (buildtime)` + the generic advisory.
- portuale pre-fix (all four cells): rc 0 + bootstrap, 4 resp. 2 plain
  rows. Divergences: b0 exit+error (rc 0 vs 1, missing block);
  default merge-list shape (4 rows vs 5 incl. dual-root row;
  comparator-blind: identity is `(type, cp, slot)`, `to <ROOT>`
  unparsed).

Verbatim merge-list sections (boilerplate EAPI/profile Global-Update
noise stripped; full log `/tmp/opencode/g216/probe.log`):

```
### emerge -p --color=n app-misc/g216top
These are the packages that would be merged, in order:

Calculating dependencies  ... done!
Dependency resolution took 0.24 s (backtrack: 1/20).

[ebuild  N     ] dev-lang/g216boot-1.0
[ebuild  N     ] dev-lang/g216comp-1.0
[ebuild  N     ] dev-lang/g216comp-1.0 to /tmp/g216-probe/fixtures/
[ebuild  N     ] dev-libs/g216mid-1.0 to /tmp/g216-probe/fixtures/
[ebuild  N     ] app-misc/g216top-1.0 to /tmp/g216-probe/fixtures/
rc=0
### emerge -p --color=n --backtrack=0 app-misc/g216top
These are the packages that would be merged, in order:

Calculating dependencies  ... done!
Dependency resolution took 0.24 s (backtrack: 0/0).



[ebuild  N     ] app-misc/g216top-1.0::testrepo to /tmp/g216-probe/fixtures/ 0 KiB
[ebuild  N     ]  dev-libs/g216mid-1.0::testrepo to /tmp/g216-probe/fixtures/ 0 KiB
[ebuild  N     ]   dev-lang/g216comp-1.0::testrepo to /tmp/g216-probe/fixtures/ 0 KiB
[ebuild  N     ]    dev-lang/g216comp-1.0::testrepo  0 KiB

Total: 4 packages (4 new), Size of downloads: 0 KiB

 * Error: circular dependencies:

(dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled for merge) depends on
 (dev-lang/g216comp-1.0:0/0::testrepo, ebuild scheduled for merge) (buildtime)

 * Note that circular dependencies can often be avoided by temporarily
 * disabling USE flags that trigger optional dependencies.
rc=1
```

## 3. S1 rule (portuale commit; one rule)

`resolver: prefer the in-graph self || branch and re-resolve the
self-cycle (backlog #216)`. In `portage-repo/src/lib.rs` only:

1. `disjunction_preference`: the `circular_self` bolt-on (real has no
   equivalent) now fires only when the self atom matches nothing
   installed AND nothing in-graph (`atom_matches_graph`, split out of
   `atoms_all_in_graph`). Otherwise the alternative competes normally
   and an in-graph self match ranks `Installed` (bin 0) -- real's
   pass-1 pick.
2. Same function: the `circular_atom` demotion (`dep_check.py:673-691`)
   -- first half of #22-slice-5's documented cut -- via a new
   `circular_dep` argument (owner cp → child cpvs): demote to `Other`,
   with real's blocker skip and `vardb.match(atom)` exemption
   (`atom_matches_installed`). Threaded to all three walk sites (main
   walk, `enqueue_dependencies`, blocker scan). `parent.onlydeps` /
   `virt_parent` stay documented cuts.
3. `find_hard_cycles` sees true self-loops (`owner_self_loop`: kept
   `||`/inline edge whose atom matches the owner's own merge-bound
   cpv; cross-slot same-cp edges excluded) that touch no installed
   instance (scope: reporting one would flip an outcome-correct
   resolve into a spurious abort; the retry past them is #221's).
4. `BacktrackParams::circular_dependency` + `BacktrackFeedback::Circular`
   (budget-free, Config-like; driver settles it as-is under
   `--backtrack=0`) + `collect_feedback` gate (`cycle_restartable`:
   single self-loop closed by a kept `||` self-branch, fresh owner).

   SCOPE (deliberate, not a default): the retry fires ONLY for the
   fresh self-pick shape. A general retry was implemented and verified
   first -- it reaches real's ccd1c-default answer `[bootstrap,
   jsoncpp, cmake]` -- but it also drags three follow-up rules with
   it: display-side `kept_alt_branches` re-derivation agreement
   (ccd4b merge order), merge-vs-installed record unification (ccr0r
   pinned U-1.47.0-r2), and NVC precedence. Those belong to #221
   (G18, EAPI-gated general branch-switching), so the retry stays
   scoped here and every non-scope report is byte-identical to
   pre-change (verified: ccr0r, ccd1c-b0, ccd4b, abort-masked-cycle,
   full suite green with no drift).
   Scope: multi-node rings (ccd1c/ccd4b/cyc0/abort-cycle), inline
   self-deps, and installed-involving loops keep today's path for
   #221 (G18, EAPI-gated).

## 4. Verification

- `cargo fmt --check`: clean (run on the final tree).
- `cargo clippy --release --all-targets`: 0 warnings (final tree).
- `cargo test --release` (whole workspace, final tree):
  1819 passed, 0 failed, 31 sections -- incl. `portage-repo` 867
  (new unit tests) and `portuale` bin 708/708. Log:
  `/tmp/opencode/g216/test-release.log`. (One procedural note: the
  bin suite needs `cargo build --release --bin portuale` first --
  without it 10 ask/eselect/resume tests fail at spawn with
  `NotFound`; unrelated to this slice, all 10 pass once built.)
- pmtest suite (final behavior; two doc-comment-only edits landed
  after its binary build -- zero behavioral delta, clippy-clean):
  **2117 passed, 0 failed**, 37 skipped, 5 xfailed. Log:
  `/tmp/opencode/g216/pmtest-full2.log`. The 4 failures of the
  intermediate (general-restart) run are gone by scoping (§3);
  no corpus drift remains (do NOT bless -- nothing to bless).
- Focused green: new unit tests (pick narrowing ×3 incl. installed
  exemption, demotion, self-loop detection ×4 incl. installed
  invisibility, restart gate ×4), all pre-existing disj/find-cycles
  tests, the 4 new CASES + 2 new pins, ccr0r/ccd1c-b0/ccd4b/abort-
  masked-cycle outputs byte-identical to pre-change, output invariants
  20/20.
- `--json` on `app-misc/g216top`: `backtrack.restarts == 1` (pass-0
  self-cycle → demote → boot), mirroring real's `backtrack: 1/20`.

## 5. Oracle + L0 expectations (BED-PENDING -- coordinator)

- `differential-test-bed/run/l0-fixture-oracle-all.sh
  differential-test-bed/atomlists/l0-fixture-oracle-g216.txt`
  (new list, `FX_HOST_ROOTS=1`): expect 4/4 cells green. The
  host-exact staging is deliberate: it is the single-root shape of
  the L0 rows (no ESYSROOT split, no dual rows); the default staging
  would leave a `totals` finding (real Total 4 vs portuale 3) that
  needs multi-root modeling (residual below). Default cells are
  control-green pre- and post-fix; the `--backtrack=0` cells are the
  pins (pre-fix: exit 1-vs-0).
- `differential-test-bed/run/l0-resolver.sh`: expect row-identical to
  the last green (podman/plasma-meta stay `truncated`: this slice
  corrects the pass-1 pick and the hermetic re-resolve, not the
  autounmask-restart prefix that truncates real's lists -- parked
  analysis `docs/circular-dep-backtracking-plan.md`).

## 6. Residues (no new backlog numbers filed -- coordinator owns numbers)

- R1 (deferred to #221/G18): general circular-`||` branch-switching
  (multi-node rings: ccd1c-default still rc 1 where real is rc 0 with
  `[bootstrap, jsoncpp, cmake]`; ccd4a; installed-involving self-loops
  e.g. ccr0r resolve through the invisible-self path to the
  outcome-correct answer without a visible restart). Verified during
  this slice that the general mechanism reaches real's ccd1c answer,
  but it needs display-side (`kept_alt_branches`) agreement and
  merge-vs-installed record care that belong to #221 with its EAPI
  re-checks.
- R2: default-staging dual-root rows + `Total: 4` need multi-root
  (ESYSROOT) graph modeling. Comparator-blind today except via
  `Total`.
- R3: `suppressed_alt_edges`' self-branch skip still encodes the old
  bolt-on rationale (display-side re-derivation); harmless while the
  retry stays self-scoped, must be revisited with R1.

## 7. Commits (paired; pmtest first)

- pmtest: `ae75be8` -- fixtures + `l0-fixture-oracle-g216.txt` + runner
  wiring + CASES + 2 pins.
- portuale: `ebebbed8` -- the S1 rule + unit tests (quotes pmtest sha).

## Round 2 (2026-09-28; status NEEDS_CONTEXT for the b0 cell, DONE for M3)

Bed evidence (STOP on `ebebbed8`,
`pmtest/differential-test-bed/logs/l0-fx-20260928T114021Z/`):
`--backtrack=0 app-misc/g216top` is the only unclean cell (rc 1/1
matches). Real prints 3 tree-indented merge rows, no nomerge --
`top` (d0), `mid` (d1), `comp` (d2) -- `Total: 3`, then the self
block. Portuale prints 5 rows -- `[nomerge] top` (d0), `[nomerge]
mid` (d1), `[ebuild] comp` (d2), `[ebuild] top` (d0), `[ebuild]
mid` (d1) -- `Total: 3`, then the byte-identical self block. The
other three cells are clean (verified in the captures, and
`dev-lang/g216comp` b0 matches real row-for-row). No container probe
was run (rule 13: the round-1 probe budget is spent, beds are the
coordinator's); everything below is source-grounded in
`3rdparty/portage` 3.0.82.2 (worktree) plus local portuale
reproduction (`emerge --pretend [--tree]`, single-root
`fixture_env`).

Mechanism traced to the forced-tree display of the abort partial,
not to resolution (exit code, block text, `Total: 3` all match):

- The b0 abort renders the `UnserializableCycle` partial
  (`abort_outcome`, `lib.rs:20582`, order = `cycle_display`) through
  the #206-S2 forced `--verbose --tree` path (`pretend.rs:12717`),
  i.e. real `_show_circular_deps`' `display(handler.merge_list)`
  (`depgraph.py:10425-10435`).
- Portuale's `cycle_display` for this cell is `[top, mid, comp]`
  (`--json aborted.members`; `reduced_merge_order`,
  `merge_order.rs:2564`: no leaf exists while the kept unsatisfied-
  buildtime self-edge `comp -> comp` is in the drain set, so the
  `tempgraph.order[0]` fallback drains insertion order). The tree
  walk (`print_tree`, reversed `tree_display_order`) then meets
  `comp` first and pulls `mid`, `top` in as `ordered=False`
  ancestors -- the 2 `[nomerge]` rows plus the duplicated merges.
  Default `--tree` on the same fixture is clean (no self-edge once
  the retry picks `boot`), confirming the self-loop is the trigger.
- Real's `_prepare_reduced_merge_list`
  (`circular_dependency.py`) is the same leaf-drain-with-`order[0]`-
  fallback, and its `_ordered_tree_display`/`_prune_tree_display`
  (`output_helpers.py:434-535`) is what portuale ports -- yet real
  shows 3 ordered rows and no ancestors. Simulating real's walk on
  the graph-with-self-edge with drain `[top, mid, comp]` yields 5
  rows (ancestors + duplicates; the prune keeps all five -- traced
  line by line), so real's drain/display inputs must differ in a way
  source reading does not resolve: either its scheduler-graph
  insertion order puts a leaf first (drain `[comp, mid, top]`,
  reversed walk clean), or its display graph lacks the self-edge at
  display time despite the `pkg != dep.parent or (buildtime and not
  satisfied)` keep-rule (`depgraph.py:3765-3769`, which keeps this
  unsatisfied-buildtime self-edge -- verified the `satisfied` flag
  only ever comes from installed packages, and the fixture vdb is
  empty). Disambiguating needs a live real `--debug` digraph dump
  of this cell (insertion order + whether the self-edge survives
  into `conf.digraph`), which is a coordinator bed/probe action.

Why no fix was made: every candidate is a guess with a broad blast
radius. Reordering the partial, dropping the self-edge from
`build_digraph`/`resolved_dep_targets`, or special-casing ancestors
would move the pinned tree display (`output_helpers.py` port,
#75/#131/#155) for all circular cells, not just this one -- a
judgment call beyond this brief (rule 4). The resolution itself
(rc, block, `Total:`, retry) is correct; only the stuck-remainder
tree shape diverges, and the shipped doc already calls
tree/`[nomerge]` rendering a deliberate cut (`AbortReason::
UnserializableCycle`, `lib.rs:20448`).

Questions for the coordinator (do not pick a default):

1. Run the `--debug` digraph dump for this cell on the bed image
   and share `digraph.order` + the `comp` row's parent/child sets,
   or authorize one rule-13 diagnostic probe for it?
2. If real drains `[comp, mid, top]`: port the insertion-order
   difference (wherever `_create_graph` order diverges from
   `build_digraph`'s discovery simulation) as its own slice -- it
   will touch merge order generally, not just this display.
3. Or scope it: record the b0 multi-level self-cycle tree shape as
   a documented display cut (like R2's dual-root rows) with a fresh
   backlog number, and bless the 3 oracle findings as explained?

M3 DONE (separate rule, separate commit): `dev-lang/g216comp`
default + `--backtrack=0` pins mirroring `g216top`'s, pmtest
`c9f6e8a` (pmtest-only; no portuale counterpart -- states so in its
message). New fns `test_or_pick_direct_target_resolves_to_bootstrap`
(`[boot, comp]`, rc 0) and
`test_or_pick_direct_target_backtrack0_reports_the_self_cycle` (rc 1
+ `_G216_SELF_BLOCK`); grounded on the S0 probe plus the L0 oracle
single-root captures. Gates: focused `or_pick` pins (4) + `or-pick`
CASES (4) green; full `test_emerge_pretend_contract.py` green
(**1387 passed, 5 xfailed**, `--basetemp=/var/tmp/pmtest-g216b`,
`-p no:cacheprovider`); no corpus drift (nothing to bless, none
set). No portuale code changed, so no fmt/clippy run applies;
binary reused from the `ebebbed8` tree.

BED-PENDING: none from this round (M3 is contract-only; the b0 cell
still diverges as documented above -- no READY-FOR-BEDS written).

Commits this round: pmtest `c9f6e8a` only. Portuale tree untouched
(still `ebebbed8`).
