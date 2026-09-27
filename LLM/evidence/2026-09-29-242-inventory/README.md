# Slice A inventory — every observable the root split would move (backlog #242)

Status: **inventory only — no product code, no pin moved.** Slices B/C/D pin
against this directory.

## 0. Method and provenance

All `probes/default-*.txt` captures are **fresh real-`emerge` bytes** taken
2026-10-01 for this slice (not copied from older reports), one `podman run`
per the checked-in `probes/probe.sh`:

- Image: `localhost/test-portuale:latest` (`5616d41d00cd`, built 2026-09-30;
  `emerge --version` inside: **Portage 3.0.82.2**, the L0 pin — see
  `probes/fingerprint.tsv`).
- Fixture tree: pmtest @ `5e9a428` (fixtures at `5a67314`), staged by
  `differential-test-bed/layers/l0-fixture-oracle/stage.sh` verbatim.
- Default staging: `PORTAGE_CONFIGROOT=$FX ROOT=$FX
  PORTAGE_RUNNING_ROOT=$FX` (`probes/default-*.txt`).
- Control: `PORTAGE_CONFIGROOT=$FX ROOT=/ PORTAGE_RUNNING_ROOT=/`
  (`probes/hostroots-*.txt`) — the `FX_HOST_ROOTS=1` shape of
  `layers/l0-fixture-oracle/in-container.sh:84-89`.
- `PYTHONHASHSEED=0` throughout, as the beds do.
- The `3rdparty/portage` checkout this inventory verifies against is
  3.0.82.2 (`1d95fc2c5`, via `./3rdparty/setup.sh portage`); the probe
  image's 3.0.82.2 is the same release (the n230 round-0 diff check
  already established the display/resolver paths used here are identical
  to 3.0.81.3's, so no version delta applies).

Boilerplate `Global Updates` / `license group 'FREE'` noise appears only
in the first container run of each staging (portageaujit); the bytes below
are quoted from the second and later runs where both stagings are quiet.

Companion files: `real-source-map.md` (the verified line-level map every
later slice stands on), `pin-table.md` (every pin/row/entry with "moves in
B / C / D" or "must not move" — B/C/D's guard checklist).

## 1. The three folded symptoms, re-probed fresh

### 1a. #207 — `=dev-libs/cyc0z-1` (and `-2`): real backtracks and blames `cyc0z-3`

Default (`probes/default-cyc0z-1.txt`), rc 1:

```
Dependency resolution took 0.13 s (backtrack: 1/20).

[ebuild  N     ] dev-libs/cyc0z-1::testrepo to /tmp/xa-inv-fx/fixtures/ USE="foo -bar" 0 KiB
[nomerge       ]  dev-libs/cyc0y-1::testrepo
[ebuild  N     ]   dev-libs/cyc0z-3::testrepo  USE="foo -bar" 0 KiB
[ebuild  N     ]    dev-libs/cyc0y-1::testrepo  0 KiB

Total: 3 packages (3 new), Size of downloads: 0 KiB

 * Error: circular dependencies:

(dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) depends on
 (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
  (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)

It might be possible to break this cycle
by applying the following change:
- dev-libs/cyc0z-3 (Change USE: +bar -foo)
```

Control (`probes/hostroots-cyc0z-1.txt`), rc 1:

```
Dependency resolution took 0.06 s (backtrack: 1/20).

[nomerge       ] dev-libs/cyc0z-1::testrepo  USE="foo -bar"
[ebuild  N     ]  dev-libs/cyc0y-1::testrepo  0 KiB
[ebuild  N     ]   dev-libs/cyc0z-1::testrepo  USE="foo -bar" 0 KiB

Total: 2 packages (2 new), Size of downloads: 0 KiB

 * Error: circular dependencies:

(dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) depends on
 (dev-libs/cyc0z-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)

It might be possible to break this cycle
by applying any of the following changes:
- dev-libs/cyc0z-1 (Change USE: -foo)
- dev-libs/cyc0z-1 (Change USE: +bar)
```

What moves between the two stagings (Slice B's target): the cycle forms in
`/` around `cyc0z-3` under default staging (`backtrack: 1/20`, Total 3, the
argument version coexists with `cyc0z-3`, blame `cyc0z-3 (+bar -foo)`) and in
the single root around the reused argument under host-exact roots
(`backtrack: 1/20`, Total 2, blame `cyc0z-1` with two suggestions). The
`--debug` digraphs (`probes/default-debug-cyc0z-1.txt`,
`probes/hostroots-debug-cyc0z-1.txt`) confirm the roots: default stages the
argument as `... merge to '/tmp/xa-inv-fx/fixtures/'` while `cyc0y-1` /
`cyc0z-3` are bare (running root `/`); host-exact roots render every node
bare. `=dev-libs/cyc0z-2` behaves the same way (Total 3, blame `cyc0z-3
(+bar -foo)` by default; Total 2, blame `cyc0z-2 (+bar -foo)` as control —
one suggestion there, since `-2`'s own USE already carries `bar`).

New vs the g207 report: the old probes predate the #206 node-text slice,
so their "portuale blames the argument" comparison now has a second axis —
these captures also pin the **merge-list `to <ROOT>` suffix** (Slice D):
the FX-rooted row carries unquoted `to /tmp/.../fixtures/` while the
`/`-rooted rows stay bare, and the cycle-node text stays bare on both sides
(see §4 for why that is consistent, not contradictory).

Portuale today (unchanged by this slice): single-root graph, reuses the
argument, prints the `cyc0z-1`-blame rotation — i.e. the hostroots shape
minus the second suggestion. Pinned as strict-xfail
`test_circular_dependencies_upstream_pg0_real_text[=dev-libs/cyc0z-1]`
(pmtest `test_emerge_pretend_contract.py`, `_CYC0_REAL_BLOCKS["=dev-libs/cyc0z-1"]`
matches this probe's default-staging block byte for byte). **Moves in B**
(resolution + blame); the suffix column moves in D.

### 1b. #208(a) — the `cyc0w` start node: settled since #278, only display residue left

`=dev-libs/cyc0w-3` is byte-identical across stagings except the suffix
(`probes/default-cyc0w-3.txt` vs `probes/hostroots-cyc0w-3.txt`), rc 1,
`backtrack: 0/20` both sides:

```
[ebuild  N     ] dev-libs/cyc0w-3::testrepo to /tmp/xa-inv-fx/fixtures/ 0 KiB   # bare under hostroots
[ebuild  N     ]  dev-libs/cyc0y-1::testrepo  0 KiB
[ebuild  N     ]   dev-libs/cyc0z-3::testrepo  USE="bar foo" 0 KiB

Total: 3 packages (3 new), Size of downloads: 0 KiB

 * Error: circular dependencies:

(dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) depends on
 (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)

It might be possible to break this cycle
by applying the following change:
- dev-libs/cyc0z-3 (Change USE: -foo)
```

plus the autounmask tail (`>=dev-libs/cyc0z-3 bar`, early-backtrack note).
`=dev-libs/cyc0w-1` / `-2` likewise start at `cyc0y-1` under **both**
stagings (see the captures). The g207 report's "rotation is a cross-root
artifact with no portable fix" is therefore **superseded**: backlog #278
(now DONE) showed the start node follows real's `shortest_cycle[0]` under
both stagings, portuale matches it, and
`test_circular_dependencies_upstream_pg0_real_text_cyc0w3` pins it passing.
The pin table carries the `=cyc0w-3` block under "must not move" for B/C;
its merge-row suffix moves in D.

### 1c. #230's residue — the blk0 missed line sits in `/`

Two-parent cell (`--backtrack=0 dev-libs/blk0b dev-libs/blk0c
dev-libs/blk0a`; `probes/default-blk0-bca.txt`), rc 0:

```
[ebuild  N     ] dev-libs/blk0x-1
[uninstall     ] dev-libs/blk0y-1
[blocks b      ] =dev-libs/blk0y-1 ("=dev-libs/blk0y-1" is soft blocking dev-libs/blk0x-1)
[ebuild  N     ] dev-libs/blk0b-1 to /tmp/xa-inv-fx/fixtures/
[ebuild  N     ] dev-libs/blk0c-1 to /tmp/xa-inv-fx/fixtures/
[ebuild  N     ] dev-libs/blk0a-1 to /tmp/xa-inv-fx/fixtures/

WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/blk0x:0

  (dev-libs/blk0x-3:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with
    <dev-libs/blk0x-2 required by (dev-libs/blk0b-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/xa-inv-fx/fixtures/') USE="" ELIBC="glibc"
    ^               ^
    <dev-libs/blk0x-3 required by (dev-libs/blk0c-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/xa-inv-fx/fixtures/') USE="" ELIBC="glibc"
    ^               ^
```

Control (`probes/hostroots-blk0-bca.txt`): merge rows bare, parents bare,
and the missed line renders the fixture profile —
`(dev-libs/blk0x-3:0/0::testrepo, ebuild scheduled for merge) USE=""
ELIBC="glibc"` — with the parents in the **opposite order** (blk0c first,
then blk0b; default lists blk0b first). The order flip is systematic
(`-bca` and `-cba` both flip; `PYTHONHASHSEED=0` in both runs, so it is
graph fallout of the second root, not hash noise) — recorded here so B/C
know the parent order is root-sensitive; D must not "fix" it by sorting.

Full cell matrix (all six orders probed under both stagings):

| Cell | Default missed line | Control missed line |
|---|---|---|
| `abc`, `bac` | `blk0x-2`, one parent (blk0b) | `blk0x-2`, one parent (blk0b) |
| `bca`, `cba` | `blk0x-3` `ABI_X86="(64)"`, two parents (blk0b, blk0c in that order) | `blk0x-3` `ELIBC="glibc"`, two parents (blk0c, blk0b in that order) |
| `acb`, `cab` | no block on either side | no block on either side |

Portuale today renders every missed line as `USE="" ELIBC="glibc"` with
bare parents — i.e. the control shape. Allowlisted as
`skipped-updates-cross-root-missed-line`
(pmtest `differential-test-bed/compare/known-divergences-fixture-oracle.yaml`,
`a7dba52`). **The missed-line USE moves in B** (it needs the running-root
tree to exist before any paint can read it); **the parent `to` suffixes
move in D**; the allowlist entry retires when both land. The comparator's
`to`-suffix normalisation stays until D (see §3).

## 2. The bed cells that hide the split today — four of thirteen

`run/l0-fixture-oracle-all.sh`'s `LISTS` table carries thirteen lists; four
run forced through `FX_HOST_ROOTS=1`:

| List | Rows | Knob verdict |
|---|---|---|
| `l0-fixture-oracle-host.txt` (1 row: `=dev-libs/disjtarget-2.0`) | host-exact roots | **Intrinsic.** The header names the shape: a `BDEPEND` resolved between the target and running roots, which `create_trees` splits when `ROOT=$FX` (B0b, `findings/l0.md` "#68/#72 B0b"). The single row is the cross-root shape itself — it stays host-exact. |
| `l0-fixture-oracle-slotop.txt` (18 rows) | `FX_SLOTOP_BDEP=1 FX_HOST_ROOTS=1` | **Workaround the split retires in C.** Header: without host-exact roots "the bed's `ROOT=$FX != "/"` splits real's slot-op cascade across two trees and the comparison is meaningless (the #71 S0 finding)". Once portuale models the second root, this list runs under default staging; the `@world` merge-order class (`slotop-world-complete-graph-order`, #17 family) is orthogonal and stays. |
| `l0-fixture-oracle-g215.txt` (4 rows) | `FX_SOUSAT_UNSAT=1 FX_HOST_ROOTS=1` | **Workaround the split retires in C.** Header borrows "the slotop matrix's own knob" to keep "the host-exact single-root shape the S0 probes ran". RDEPEND-only shape, so B does not move it; C re-runs it under default staging. |
| `l0-fixture-oracle-g216.txt` (4 rows) | `FX_HOST_ROOTS=1` | **Workaround the split retires in C.** Header is explicit (quoted in `_242.md` §4 point 2): default staging prints dual `g216comp` rows with Total 4, "needs multi-root graph modeling (residual)". Fresh bytes for that claim: `probes/default-g216top-b0.txt` (Total 4, dual rows) vs `probes/hostroots-g216top-b0.txt` (Total 3, single rows); default `app-misc/g216top` resolves 5 rows (`boot`, `comp`, `comp to <ROOT>`, `mid to <ROOT>`, `top to <ROOT>`) vs 4 bare rows as control. |

The nine default-staging lists (`l0-fixture-oracle.txt` 48 rows,
`-rdcpin`, `-whpin`, `-r25`, `-g210`, `-g212`, `-g213`, `-g214`, `-244`
18 rows) run with `ROOT=$FX` today, and the bed additionally pins
`PORTAGE_RUNNING_ROOT=$FX` (`layers/l0-fixture-oracle/in-container.sh:57-89`)
so both sides resolve `BDEPEND`/`IDEPEND` against the fixture rather than
the container's real `/` — a bed-side approximation of real's
`create_trees` (`portage/__init__.py:497-529`), which equates the running
root with the target root only when `ROOT == "/"`. Once portuale models
the second root while the bed keeps this pin, rows whose build deps real
takes from `/` still move on portuale's side toward real; rows in the
"must not move" column are exactly the RDEPEND-only cells (notably the
g212 `somaskvisparent` cell, annotated "RDEPEND-only, so no staged-ROOT
cross-root split (#242) is involved" — keep) and every L0 single-root row.

**BED-PENDING (coordinator):** the default-staging pass of the four lists
recording every differing row by name. These four run host-exact in the
bed today, so the pass means staging them with `FX_HOST_ROOTS` unset and
diffing real vs portuale per row; the prediction from this inventory is:
`-host` 1/1 differing (the row *is* the cross-root shape),
`-slotop` the buildtime-key argument cells plus the three `@world` cells
(order class, pre-existing) differing, `-g215` clean (RDEPEND-only),
`-g216` 4/4 differing by the dual-row/Total shape captured above. The
coordinator's pass confirms or corrects this row by row.

## 3. Comparator and corpus machinery that abstracts the split away

- **`_norm_skipped_detail`** (pmtest `differential-test-bed/compare/resolve-compare.py:131`,
  pmtest `2963703`): strips `to '<root>'` on both sides of skipped-block
  detail lines (plus the `for <ROOT>` group headers); installed
  `in '<root>'` lines untouched; USE content deliberately not normalised
  (test 12 pins that a suffix-only pair compares clean while a missed-line
  USE delta still fires). **Verdict: keep through B/C, retire-or-keep
  decided in D** — retiring it is only safe once portuale emits the
  suffixes; until then it is the only thing keeping the blk0 parent lines
  green. Finding/test name: `skipped-updates-cross-root-missed-line` /
  `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`.
- **Dual-row collapse by `(type, cp, slot)`** (`resolve-compare.py:448-451`;
  `MERGE` regex `:40`): the trailing `to <path>` is not part of the
  package identity key, so real's dual `g216comp` rows collapse to one map
  entry on real's side too — comparator-blind except via `Total:` (verbose
  tree) and the row count. **Verdict: keep** — it is a general identity
  rule (multi-slot `llvm-core/llvm:21 + :22` needs the slot in the key),
  not a cross-root workaround; C's dual rows must survive it, which they
  do as long as row identity stays `(type, cp, slot)` per root-qualified
  row. The g216 header's note stands.
- **Harvested corpus** (`pytests-contract-suite/corpus/`, drift is a
  warning): the #230 bless (`3126268`) moved 65 corpus cases in the
  skipped block only. B/C/D re-bless whatever their rows move, reviewed
  first, in the pmtest commit (`PORTUALE_CORPUS_BLESS=1`).

## 4. Deliberate approximations — verdicts

- **`root_deps_running_root` is `None` at every resolve call site**
  (`rust/portage-repo/src/lib.rs:40793` test-boundary constructor and the
  other `None` call sites; `solver_bridge.rs` carries no override).
  Real's `depend_root` selection (`depgraph.py:4218-4238`, verified in
  `real-source-map.md`) always resolves EAPI ≥ 7 `DEPEND` against
  `ESYSROOT` and `BDEPEND`/`IDEPEND` against the running root. **Verdict:
  close in B** — this is the root split itself. (Note: `pretend.rs`'s
  `--root-deps` plumbing, `resolve_root_deps_running_root` +
  `root_deps_satisfied_atoms` / `unsatisfied_root_deps_atoms` /
  `resolve_root_deps_build_entries`, only answers "is it already there"
  and stays as is per B8 unless B's S0 proves otherwise.)
- **`running_root_satisfies_atom`'s own doc** (`lib.rs:6997`): "a fuller,
  recursive second-root graph isn't attempted". **Verdict: close in B** —
  the doc's narrowing is exactly what B implements; update the doc there.
- **Scope-backlog's "edge-by-edge approximation" row and its permanent
  non-gap** (`docs/scope-backlog.md:264-265`): a running-root entry's
  `PDEPEND` stays a target-`ROOT` concern — **not reopened** (matches
  real's deps queue: `PDEPEND → myroot`). The "edge-by-edge" row narrows
  in B/C; D updates the row text.
- **The #206 display convention** ("merge nodes kept bare",
  `docs/what-this-proves.md:18721`): portuale's `root_suffix`
  (`rust/portuale/src/pretend.rs`, `darkgreen("to " + pkg.root)` port at
  `output.py:841-862`) annotates **only** `targets_running_root`
  (`--root-deps`) entries, never every entry under a non-`/` ROOT — the
  deliberate determinism cut (a literal port would print each fixture
  test's `mktemp -d` ROOT). **Verdict: revisit in D per display site**
  (B9 default: match real). The per-site split this inventory settles
  from fresh captures + verified source:
  - Merge-*list* rows: `resolver/output.py:462,475,861` (`"to " + pkg.root`,
    `pkg.root` = EROOT, **unquoted**) — capture-backed: every FX-rooted
    merge row in `probes/default-*.txt` carries `to /tmp/.../fixtures/`,
    `/`-rooted rows are bare.
  - Cycle-node text: `_prepare_circular_dep_message`
    (`circular_dependency.py:76-99`) formats via `f"{pkg}"` =
    `Package.__str__` (`Package.py:568-608`), which appends **quoted**
    `to '{settings["ROOT"]}'` for a merge node when `ROOT != "/"`.
    All printed cycle nodes in these captures sit in `/` (the cycle
    forms in the running root), hence bare — consistent with `__str__`,
    no container/checkout delta, and the `_242.md` §4-point-4 "unsettled"
    question is answered as far as these shapes go. Not capture-backed:
    whether an FX-rooted node inside `shortest_cycle` would print the
    quoted suffix — source says yes; if such a shape exists, D's S0
    captures it before S1.

## 5. Real-source map

`real-source-map.md` (verified 2026-10-01 against `3rdparty/portage`
3.0.82.2 `1d95fc2c5`): `_add_pkg_deps`'s `depend_root` selection
(`depgraph.py:4218-4238`) and five-group `deps` queue (`:4255-4291`),
`_dep_expand`'s `root_config` (`:4928`), `_cross` (`:4921`),
`create_trees` (`portage/__init__.py:497-529`), the display sites
(`Package.py:568-608`, `output.py:462/475/861`,
`circular_dependency.py:76-99`, `UseFlagDisplay.py:55`), and the
serializer abort (`depgraph.py:10262-10289`).

## 6. Pin table

`pin-table.md`. Guard shorthand for B/C/D: `l0-resolver.sh` identical row
by row (single-root — any movement stops the slice); fixture oracle 0
unexplained with only the rows the table assigns to that slice moving,
and only toward real; #161's `lib.rs` driver unit tests green.
