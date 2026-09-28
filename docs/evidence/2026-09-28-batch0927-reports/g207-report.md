# g207 report — #207 then #208 S0: the `cyc0*` circular-dependency cases (#181 residue)

Status: **NEEDS_CONTEXT** (both items). No product commits, no pmtest commits,
no pins touched, no beds needed (nothing to gate). Worktrees left clean
(`git status` empty in `wt-207-cyc0-circular-text/{portuale,pmtest}` and in
`portuale/3rdparty/portage`, whose one temporary instrumentation was reverted).

TL;DR: #207's divergence is the bed's staged `ROOT`, not a portuale bug —
live real resolves `DEPEND` (EAPI ≥ 7) against `ESYSROOT=/` while the target
is `$FX`, so the cycle forms in `/` around the highest version (`cyc0z-3`)
and the merge-order dead-end spends one `circular_dependency` backtrack run;
the playground (and portuale's single-root model) resolve everything in one
root, reuse the argument version, and blame `cyc0z-1`. Same root cause
explains #208's first half (cycle rotation is `get_cycles` first-minimal
order over the residual graph — an artifact of the cross-root graph, no
portable rule). #208's second half (suggestion on autounmasked USE) is a
genuine, already-documented portuale simplification with a concrete one-rule
fix — but shipping it alone cannot turn the pin while the rotation differs,
so it is proposed, not shipped (owner call).

## #207 S0 — why live real backtracks for `=dev-libs/cyc0z-1` (and `-2`)

Question from the entry: live real reports `backtrack: 1/20`, schedules
`cyc0z-3` next to the argument `cyc0z-1`, prints cycle `cyc0z-3 ↔ cyc0y-1`
with suggestion `cyc0z-3 (+bar -foo)`; portuale (like the
`ResolverPlayground` expectation) reuses `cyc0z-1` and offers `+bar` / `-foo`
on it.

### Real mechanism (named before code)

1. `DEPEND` root selection, `_add_pkg_deps`
   (`3rdparty/portage:lib/_emerge/depgraph.py`, EAPI-attribute branch):
   with `eapi_attrs.bdepend` (EAPI ≥ 7, `lib/portage/eapi.py:292`)
   `depend_root = pkg.root_config.settings["ESYSROOT"]`, else the running
   root. Native (non-cross) bed: `ESYSROOT=/`.
2. The bed runs with `ROOT=$FX` (`in-container.sh` exports
   `PORTAGE_CONFIGROOT/ROOT/PORTAGE_RUNNING_ROOT=$FX`, `DISTDIR`,
   `PYTHONHASHSEED=0`, `PORTAGE_REPOSITORIES`). So the argument lands as
   `cyc0z-1` in root `$FX`, but its `DEPEND="foo? ( !bar? ( dev-libs/cyc0y ) )"`
   resolves with `dep_root=/`: `cyc0y-1` in `/`, whose unversioned
   `DEPEND="dev-libs/cyc0z"` selects highest-available `cyc0z-3` in `/`.
   The package tracker is per-root, so `_check_slot_conflict`
   (`depgraph.py:3532`) finds no `$FX`-side `cyc0z-1` under key `/` — no
   reuse, no slot conflict, both versions scheduled (the `[nomerge]`
   `cyc0y-1` row is the `$FX`-side view; the cycle is `cyc0y-1(/) ↔
   cyc0z-3(/)`).
3. The backtrack run is spent by the merge-ordering dead-end, not by a
   conflict: `_serialize_tasks` (`depgraph.py:9457`, leaf-selection
   `if not selected_nodes:` arm ~`:10262-10305`) records the residual
   `circular_dependency` map into `_backtrack_infos["config"]` and sets
   `_need_restart`; `_backtrack_depgraph` (`depgraph.py:12192`,
   `max_retries = myopts.get("--backtrack", 20)`) runs try 1, which hits
   the already-recorded cycle (`unsolved_cycle` → `_skip_restart`) and
   displays. Hence `backtrack: 1/20`.
4. The playground is single-root by construction (the staged root *is* the
   `EPREFIX`, so `ESYSROOT` == target root): `cyc0y-1`'s dep reuses the
   in-graph `cyc0z-1` via `_check_slot_conflict`'s
   `atom.match(existing_node...)` arm, cycle `cyc0z-1 ↔ cyc0y-1`,
   solutions `{foo off}` / `{bar on}` — exactly upstream's checked-in
   expectation and portuale's output. Verified by running the upstream
   `=Z-1` case through the worktree's own 3.0.82.2 `ResolverPlayground` on
   the host (edges `Z-1 → Y-1 → Z-1`; also with EAPI 8 ebuilds: same
   reuse — EAPI is *not* the trigger).

### Environment suspects from the brief, ruled out one by one

- `--backtrack`: default 20 on both sides (playground `options={}` →
  `myopts.get("--backtrack", 20)`; bed passes no flag; probe myopts show
  no override). Live `--backtrack=0` shows the identical graph, so the
  budget is not the difference.
- `--autounmask`: `True` on both sides (`create_depgraph_params.py`
  default; probe myparams confirm `autounmask: True` live).
- Installed set: fixture vdb contains no `cyc0*`; a host-side repro with an
  *empty* vdb is byte-identical in shape. Ruled out.
- Profile / `make.conf` USE / world: a minimal host-side repo+profile
  (4 ebuilds, empty vdb/world, bare profile) reproduces the live shape
  exactly. Ruled out.
- `EMERGE_DEFAULT_OPTS`: container's is empty (bed creates an empty
  `/etc/make.local`); probe myopts confirm. Ruled out.
- EAPI of the ebuilds: playground with EAPI 8 ebuilds still reuses.
  Ruled out (EAPI 1 in a modern repo behaves differently again —
  RDEPEND-default duplication — but that is not this item).

### The difference (environment, not a portuale bug)

Staged non-`/` `ROOT` (`$FX`) with native `ESYSROOT=/`: real splits the
graph across two roots; the single-root playground (and portuale, whose
`--root-deps` `ESYSROOT` handling is deliberately scoped to
`root_deps_running_root: Some(..)` — `rust/portage-repo/src/lib.rs:25302`,
`None` at every resolve call site) never can. Matching live here means
implementing cross-root `DEPEND` resolution for `ROOT≠/` — a resolver
architecture feature, not one rule. Per the brief: stop, NEEDS_CONTEXT —
withdraw-or-reclassify call for the owner. (Reclassify sketch, if wanted:
file "resolve EAPI ≥ 7 DEPEND against ESYSROOT when it differs from ROOT"
as its own project with #25's guard; it will move every staged-ROOT
output, not just these pins.)

### Probe 1 (#207, the one allowed; verbatim)

Command (single `podman run`, staging reused from the bed, then one
`emerge`; host-side `emerge` runs during S0 were free oracles per
`AGENTS.md` step 4 and are *not* counted here):

```
podman run --rm --name g207-probe --security-opt seccomp=unconfined \
 -v <pmtest>/fixtures:/fixtures:ro \
 -v <pmtest>/differential-test-bed:/TEST:ro \
 -v /tmp/opencode/g207:/out --entrypoint /bin/bash \
 localhost/test-portuale:latest -c '/TEST/layers/l0-fixture-oracle/stage.sh
 /tmp/probe-fx >/out/stage.log 2>&1 && FX=/tmp/probe-fx/fixtures &&
 rm -rf /var/db/pkg && cp -r "$FX/var/db/pkg" /var/db/pkg &&
 export PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX"
 DISTDIR="$FX/distfiles" PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC &&
 export PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)" &&
 /usr/sbin/emerge -p --debug --color=n "=dev-libs/cyc0z-1"
 >/out/debug-cyc0z-1.txt 2>&1; ...'
```
(rc=1. Decisive excerpt — run 0 selects highest `cyc0z-3` with no
`Re-used Child:`, both versions in the digraph, then `backtracking try 1`:

```
Parent:    (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge)
Depstring: dev-libs/cyc0z
Priority:  buildtime
Candidates: ['dev-libs/cyc0z']
   ebuild: dev-libs/cyc0z-3::testrepo
Child:         (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) USE="foo -bar" ABI_X86="(64)"
Parent Dep:    dev-libs/cyc0z required by (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge)
...
digraph:
(dev-libs/cyc0z-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/probe-fx/fixtures/') depends on
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
=dev-libs/cyc0z-1 depends on
  (dev-libs/cyc0z-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/probe-fx/fixtures/') (soft)
(dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) depends on
  (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)
(dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) depends on
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)

backtracking try 1
...
Dependency resolution took 0.26 s (backtrack: 1/20).
```

A temporarily-instrumented worktree `depgraph.py` (reverted afterwards;
`3rdparty/portage` tree clean) additionally proved the roots:
`cyc0z-1` added under root `/tmp/.../fixtures/`, `cyc0y-1`/`cyc0z-3`
under root `/` — cross-root tracker miss, no reuse. Same shape reproduced
host-side with stock `emerge` 3.0.82.2 against a 4-ebuild minimal repo.)

## #208 S0 — `cyc0w-{1,2,3}` start node + the `cyc0w-3` suggestion

### (a) Cycle starts at `cyc0y-1`, portuale starts at `cyc0z-3`

Real mechanism: `digraph.get_cycles` (`3rdparty/portage:lib/portage/util/digraph.py:387`)
emits, per node in residual-graph insertion order, the shortest path of
each child back to the node; `circular_dependency_handler._find_cycles`
(`lib/_emerge/resolver/circular_dependency.py:48`) keeps the *first*
minimal-length cycle as `shortest_cycle`, whose `[0]` is the printed start
node. Both rotations of the `{cyc0y-1, cyc0z-3}` pair are always emitted —
the winner is traversal fallout of the residual order in
`_serialize_tasks`. Derived end to end from the probe digraphs:

- `=cyc0z-1`: residual peels the parentless arg first → order
  `[cyc0y-1, cyc0z-3]` → node `cyc0y-1` yields `[cyc0z-3, cyc0y-1]` first →
  starts at `cyc0z-3` ✓ (matches real).
- `=cyc0w-3`: residual peels `cyc0w-3` first → order `[cyc0z-3, cyc0y-1]`
  → node `cyc0z-3` yields `[cyc0y-1, cyc0z-3]` first → starts at `cyc0y-1`
  ✓ (matches real; same for `cyc0w-1/-2`).

Portuale prints the same *node set* but the opposite rotation — an artifact
of a graph it does not build (cross-root, #207). No portable one-rule fix:
mirroring it means replicating real's residual traversal order. Same
withdraw-or-reclassify bucket as #207 (the `l0-report.txt` "order" rows for
`cyc0w-*` are this).

### (b) `cyc0w-3` suggestion evaluated on autounmasked `USE="bar foo"`

Real mechanism: `_pkg_use_enabled` (`lib/_emerge/depgraph.py:7669`) returns
"effectively enabled USE flags, **including changes made by autounmask**"
(from `_needed_use_config_changes`); `_find_suggestions`
(`circular_dependency.py:114`) uses it as `current_use` while
`_get_autounmask_changes` (`:104`) puts the autounmasked flags (`bar`) in
the *untouchable* set. So on `USE={bar,foo}` with `affecting={foo}` the
only assignment dropping `dev-libs/cyc0y` from Z-3's `DEPEND` is `foo off`
→ `- dev-libs/cyc0z-3 (Change USE: -foo)`. Probe shows `USE="bar foo"` on
the selected `cyc0z-3` and the `-foo` suggestion verbatim (below).

Portuale side: `circular_dep_solutions`
(`rust/portage-repo/src/lib.rs:17012`) documents the gap explicitly —
"`_pkg_use_enabled` is `effective_use_flags` **without the (rare)
autounmask-USE overlay** (autounmask-*changed* flags are still honoured as
untouchable); ... documented in `docs/history/find-suggestions-plan.md`".
With `bar` off, `!bar? ( dev-libs/cyc0y )` still requires `cyc0y`, so no
assignment drops the atom → generic "temporarily disabling USE flags"
advisory. Concrete one-rule fix (NOT shipped — owner call, see verdict):
apply the `autounmask_use_changes` overlay to `parent_use` in
`circular_dep_solutions`, mirroring `_pkg_use_enabled`.

### Verdict and proposal

- #207: NEEDS_CONTEXT — environmental (`ROOT=$FX` vs single `/`);
  withdraw, or refile cross-root `DEPEND` resolution as its own guarded
  project. No commit.
- #208(a): NEEDS_CONTEXT — same bucket (rotation artifact of the
  cross-root graph). No commit.
- #208(b): diagnosed, fixable in one rule (autounmask-USE overlay in
  `circular_dep_solutions`, docstring cites
  `depgraph.py:7669`+`circular_dependency.py:104-114`), but shipping it
  alone cannot turn the pin (the cycle lines still rotate the other way)
  and would move output under a still-red strict-xfail. Proposal: owner
  approves a follow-up slice for (b) *iff* the (a) rotation is accepted as
  a documented divergence (or the pin is split); otherwise keep (b) parked
  with (a). No commit here — judgment call per common rule 4.

### Probe 2 (#208, the one allowed; verbatim)

```
podman run --rm --name g208-probe --security-opt seccomp=unconfined \
 -v <pmtest>/fixtures:/fixtures:ro \
 -v <pmtest>/differential-test-bed:/TEST:ro \
 -v /tmp/opencode/g207:/out --entrypoint /bin/bash \
 localhost/test-portuale:latest -c '/TEST/layers/l0-fixture-oracle/stage.sh
 /tmp/probe-fx >/out/stage208.log 2>&1 && FX=/tmp/probe-fx/fixtures &&
 rm -rf /var/db/pkg && cp -r "$FX/var/db/pkg" /var/db/pkg &&
 export PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX"
 DISTDIR="$FX/distfiles" PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC &&
 export PORTAGE_REPOSITORIES="$(cat $FX/etc/portage/repos.conf/repos.conf)" &&
 /usr/sbin/emerge -p --debug --color=n "=dev-libs/cyc0w-3"
 >/out/debug-cyc0w-3.txt 2>&1; ...'
```
(rc=1, `backtrack: 0/20`. Decisive excerpts — cross-root digraph,
autounmasked `USE="bar foo"`, Y-1 start, `-foo` suggestion, autounmask
block:

```
(dev-libs/cyc0w-3:0/0::testrepo, ebuild scheduled for merge to '/tmp/probe-fx/fixtures/') depends on
  (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
=dev-libs/cyc0w-3 depends on
  (dev-libs/cyc0w-3:0/0::testrepo, ebuild scheduled for merge to '/tmp/probe-fx/fixtures/') (soft)
(dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) depends on
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
(dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) depends on
  (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)
... done!
Dependency resolution took 0.22 s (backtrack: 0/20).
...
Child:         (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) USE="bar foo" ABI_X86="(64)"
...
 * Error: circular dependencies:
(dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) depends on
 (dev-libs/cyc0z-3:0/0::testrepo, ebuild scheduled for merge) (buildtime)
  (dev-libs/cyc0y-1:0/0::testrepo, ebuild scheduled for merge) (buildtime)
It might be possible to break this cycle
by applying the following change:
- dev-libs/cyc0z-3 (Change USE: -foo)
Note that this change can be reverted, once the package has been installed.
...
The following USE changes are necessary to proceed:
 (see "package.use" in the portage(5) man page for more details)
# required by dev-libs/cyc0w-3::testrepo
# required by =dev-libs/cyc0w-3 (argument)
>=dev-libs/cyc0z-3 bar
 * In order to avoid wasting time, backtracking has terminated early
 * due to the above autounmask change(s). The --autounmask-backtrack=y
 * option can be used to force further backtracking, but there is no
 * guarantee that it will produce a solution.
```

## BED-PENDING

None — no product commit, so no beds to gate. (If the owner approves the
#208(b) slice, it will need the fixture oracle + L0 per Track G's guard.)

## Questions for the owner (Italian, per batch §2)

1. #207 e #208(a): la divergenza nasce dal `ROOT=$FX` dello stage (real
   risolve `DEPEND` contro `ESYSROOT=/`, playground e portuale in un'unica
   radice). Vada in withdraw, o la riclassifichiamo come progetto a sé
   (risoluzione cross-root di `DEPEND` per `ROOT≠/`, con la guardia di #25)?
2. #208(b): spedisco in uno slice a parte l'overlay autounmask-USE in
   `circular_dep_solutions` anche se il pin resta rosso per la rotazione
   (a), o resta parcheggiato finché (a) non è decisa?

## #208(b) — shipped as G8b (owner decision B17; 2026-09-28)

Status: **DONE** (product commit gated; beds pending coordinator —
`READY-FOR-BEDS 6fa90a45…` in `g208b-progress.md`, polling for
`BEDS <sha>: OK` / `STOP`). No judgment calls taken; entry not flipped.

What changed: `circular_dep_solutions`
(`rust/portage-repo/src/lib.rs`, was `:17012`) now applies the
`autounmask_use_changes` overlay to the parent's USE before enumerating
assignments — the one rule from the S0 diagnosis: real `_pkg_use_enabled`
(`3rdparty/portage:lib/_emerge/depgraph.py:7669`) returns the
`_needed_use_config_changes` set (autounmask included) as `current_use`
for `_find_suggestions` (`lib/_emerge/resolver/circular_dependency.py:114`),
while `_get_autounmask_changes` (`:104`) keeps the changed flags
untouchable. Overlay semantics mirror the change record: plain/`+flag`
tokens insert, `-flag` tokens remove, matched per parent with the same
`matches_config_entry` predicate the untouchable loop already used. The
bug-#555698 `parent_use` gate rides along (it is the same overlaid set).
The "without the (rare) autounmask-USE overlay" narrowing is gone from
the doc comment; `docs/history/find-suggestions-plan.md` (history of the
original plan) intentionally left as written.

Tests: new Rust unit test
`circular_dep_solutions_applies_the_autounmask_use_overlay` resolves
`=dev-libs/cyc0w-3` through `graph_result_autounmask` (the CLI-faithful
helper — `graph_result_real` runs with `autounmask_suggest_use=false`,
so it records no `bar` change; first version of the test caught exactly
that) and asserts the single suggestion `cyc0z-3 (Change USE: -foo)`
with `followup=false` (grandparent `cyc0w-3`'s `[bar]` use-dep touches
no solution flag). Live binary check: `=dev-libs/cyc0w-3` now prints
`- dev-libs/cyc0z-3 (Change USE: -foo)` verbatim (probe 2's text), under
portuale's own `cyc0z-3`-first rotation.

Pin handling (per brief): the `cyc0w-3` block sits only inside the shared
strict-xfail `test_circular_dependencies_upstream_pg0_real_text`
(`=cyc0z-1` + `=cyc0w-3` params; no separate suggestion pin exists — the
only other "temporarily disabling" pin is `gpcyclec`, which carries no
autounmask change and is unaffected). Both params stay xfail on the
rotation (#242); the shared reason now names #242 (pmtest `7780459`,
committed first).

Gates (all in-worktree, release profile): `cargo fmt --check` clean;
`cargo clippy --release --all-targets` 0 warnings; `cargo test
--release` whole workspace green (note: the first pass showed 10
`portuale`-crate spawn failures — `target/release/portuale` absent
because `cargo test` alone doesn't place the sibling binary; after
`cargo build --release` the full suite is green, 711/711 portuale +
868/868 portage-repo); pmtest contract suite with a private fresh
basetemp (`--basetemp=/var/tmp/pmtest-g208b -p no:cacheprovider`):
**2111 passed, 37 skipped, 5 xfailed, 0 failed** — the 5 xfails are the
3 pre-existing oracle/v2 items plus the 2 cyc0 params, all still failing
as intended, so zero drift and nothing to bless. No gpg homes created
(rule 17: nothing to kill).

## BED-PENDING (g208b; coordinator runs these)

- Fixture oracle, from the pmtest worktree:
  `../pmtest/differential-test-bed/run/l0-fixture-oracle-all.sh`
  (expect 0 unexplained).
- L0 resolver bed, from the pmtest worktree:
  `../pmtest/differential-test-bed/run/l0-resolver.sh`
  (expect identical to or better than the last green L0 row by row;
  per the Track G guard any lost row stops the slice).
- Rationale for expecting green: the only output change is one
  suggestion line on autounmasked cycles (`-foo` where the generic
  advisory was); the contract suite already pins every other circular
  text byte-identically and shows no drift.
