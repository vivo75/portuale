# #269 S0 — the new-slot arm must not report the provider

Fresh captures, container `localhost/test-portuale:latest`
(real Portage 3.0.82.2), 2026-10-01. Staged fixture tree via the bed's
own `differential-test-bed/layers/l0-fixture-oracle/stage.sh`, ad-hoc
ROOTs with EAPI-bearing vdb (the #253 recipe), identical effective
options both sides (`--ignore-default-opts`), `PYTHONHASHSEED=0`.
Probe scripts: `/tmp/opencode/r2/evidence-probe.sh` (v1 + cm),
`/tmp/opencode/r2/cm-rootslash.sh` (cm control with `ROOT=/`),
`/tmp/opencode/r2/dbg-extract.sh` (the `--debug` run: `real/v1-debug.out`
is the full 609-line stdout, `real/v1-debug.err` stderr,
`real/mechanism.txt` the decisive excerpts).

## V1 cell (this slice's pin)

`emerge --ignore-default-opts --pretend --color=n --backtrack=20
--update --deep app-misc/mmcons dev-libs/slotconflictoldconsumer
dev-libs/slotconflictnewconsumer` (only `mmcons` requested).
Ad-hoc ROOT: installed `mmprov-1` (`0/1`) + `mmprov-2` (`1/1`) +
`mmcons-1` bound `mmprov:0/1=`, empty world.

- `real/v1.out` (rc 0): `backtrack: 3/20`, five rows --
  `[ebuild N] slotconflicttarget-1.0`, `[ebuild U] mmprov-3 [2]` **bare**
  (no `r`), `[ebuild rR] mmcons-1`, `[ebuild N] oldconsumer`,
  `[ebuild N] newconsumer` -- the conflict `WARNING` on stderr
  (`real/v1.err`), **no** `causing rebuilds` block.
- `portuale/v1.out` (rc 0, post-fix binary): the same five rows byte for
  byte (modulo real's `to '<root>'` suffix and timing line, both
  standing display differences), the `WARNING` on stdout (standing
  difference: real prints it to stderr), no block. `--json`
  `backtrack.restarts == 3`, `abi_rebuilds == []`.
- `real/mechanism.txt`: the update probe finds `mmprov-3` among the
  *available* packages (`new child package`, no pass had scheduled it),
  `backtracking due to missed slot abi update` files
  `new_child_slot=mmprov-3`, `forced reinstall atoms: app-misc/mmcons:0`
  alone (the `depgraph.py:2442` gate), and `forced rebuilds:` stays
  empty in every pass.

Pre-fix portuale on the same probe printed only three rows
(`slotconflicttarget-1.0`, `oldconsumer`, `newconsumer` plus the
`WARNING`): its scan ranged over scheduled entries only, so with no
provider entry it scheduled nothing at all. The brief's S0 line
("portuale merges ... `[ebuild r U]` plus the block") does not
reproduce on current `main` -- the over-report it describes is what the
scan *would* print once it fires (cf. the conflict-mass control
below), and the slice ports both halves: available-ranging *and* the
pair gate.

## Conflict-mass control (bug 486580)

`emerge --ignore-default-opts --pretend --color=n --backtrack=3
--update --deep app-misc/somassa`, installed `somassb-1` (`1`) + five
leaves bound `somassb:1/1=`.

- `real/cm-rootslash.out` (rc 0, `ROOT=/` so real's build-root
  resolution lands in the same root -- single list): `[ebuild NS]
  somassb-2 [1]` bare, five `[ebuild rR]` leaves, `[ebuild N]
  somassa-1`, `backtrack: 1/3`, **no** block. (The earlier
  `real/cm.out` with `ROOT=<adhoc>` shows the same target-root rows
  plus bare `/` duplicates: real has no `PORTAGE_RUNNING_ROOT` env and
  resolves the leaves' `DEPEND` against `/`. Kept for the record.)
- `portuale/cm.out` (rc 0): the same rows, no block (post-fix; pre-fix
  it printed the `causing rebuilds` block from the scan's new-slot
  pairs). This corrects the old pin docstring, which claimed live real
  prints the block -- it does not.

## Real-source gate (fresh read, `3rdparty/portage`)

- `lib/_emerge/depgraph.py:2442-2445`: `if new_child_slot is None and
  child.installed:` -- the provider reinstall joins
  `slot_operator_replace_installed` only on the same-slot arm.
- `lib/_emerge/depgraph.py:3123-3128`: the new-slot arm calls
  `_slot_operator_update_backtrack(dep, new_child_slot=new_dep.child)`.
- `lib/_emerge/depgraph.py:3015-3057` (`_iter_similar_available`) and
  `:2677-2692`: the probe ranges over every available package (other
  slot + higher version on the new-slot arm), not scheduled entries.
- `lib/_emerge/Package.py:157-161`: fresh packages default to
  `operation = "merge"`, so the probe's `new child ... scheduled for
  merge` label does not mean the package was in the graph.
