# Evidence — backlog #244 (batch-2026-09-28_244), Slice A

Real-Portage captures for the argument-order-dependent autounmask display
(upstream `test_autounmask_use_breakage.py`, fixtures `dev-libs/aub0{a,b,c,d}`),
the abort family and the #195 batch-4 sister cells. The analysis is in
[`report.md`](report.md).

## `fixture-oracle/` — the main capture (container, real 3.0.82.2)

`differential-test-bed/run/l0-fixture-oracle.sh` over the atom list copied
here as `atomlist.txt` (pmtest `differential-test-bed/atomlists/l0-fixture-oracle-244.txt`,
first 31 lines). Each line runs as `emerge -p --color=n <line>` for real and
for portuale, inside `localhost/test-portuale:latest` upgraded to Portage
3.0.82.2, on the staged `fixtures/` tree (`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=<staged>`,
`PYTHONHASHSEED=0`, `LC_ALL=C.UTF-8`, `TZ=UTC`, `PORTAGE_REPOSITORIES` from
the staged repos.conf). Log dir `logs/l0-fx-20260929T151941Z`.

- `real/<slug>.txt`, `portuale/<slug>.txt` — combined stdout+stderr per case.
- `meta.tsv` — slug, exit codes (real, portuale). Every cell is rc 1 on both sides.
- `fingerprint.tsv` — date, real version line, pin.
- `pm.json` — portuale revision under test: `feedb558` (release profile, `main`).

## `fixture-oracle-2/` — follow-up capture, same method

Log dir `logs/l0-fx-20260929T152543Z`, same binary. Six more cells (last six
lines of the pmtest atom list): the two-argument `aub0b`/`aub0a` variants under
both `--autounmask-backtrack` values, and `dev-libs/aubreaktop` (the breakage
pin's fixture) under the default and `--autounmask-backtrack=y`.

## `playground/` — real's resolver, introspected

`aub0_playground.py` runs real Portage's own `ResolverPlayground` (vendored
3rdparty checkout, 3.0.82.2) on the upstream test's five ebuilds with
`make.conf` `USE="foo"` — the pmtest fixture profile's
`repo/profiles/base/make.defaults` sets `USE="foo"`, which is why the fixture
cells fail on `aub0d[-foo]` (A) where the upstream test's comment shows
`D[foo]` (B). With that one line the playground reproduces the container
text of all twelve cells exactly (`bt-{y,n}-<order>.txt`), and the
`--debug` twins (`bt-{y,n}-<order>.debug.txt`) show the walk: argument
nodes added in argv order, `_dep_stack` popped LIFO, the pass that fails,
`autounmask breakage detected`, and the final clean pass.

Run it from `3rdparty/portage` with `PORTAGE_GNUPGHOME=<copy of
lib/portage/tests/.gnupg>`, `PATH=$PWD/bin:$PATH`, `PGUSE='USE="foo"'`:
`python3 aub0_playground.py C,B,A --autounmask-backtrack=y [--debug]`
(`PGDEBUG=1` for the resolver's own debug output).
