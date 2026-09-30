# #249 — `circular_atom` demotion through a virtual (batch-2026-09-28 Track R)

Real's own `ResolverPlayground` (`3rdparty/portage`, Portage 3.0.82.2
tree), run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH` and a copy
of `lib/portage/tests/.gnupg` as `PORTAGE_GNUPGHOME`. Captured 2026-09-30.

`u249f_pg.py` → `u249.txt`: the pmtest fixture shape — upstream
`testCircularJsoncppCmakeBootstrapOrDeps` (bug 703440) with the `||`
moved into a new-style virtual: `dev-util/u249make` BDEPENDs
`dev-libs/u249json:0=`, which BDEPENDs `virtual/u249make`, whose RDEPEND
is `|| ( dev-util/u249make-bootstrap dev-util/u249make )`.

- Default: pass 1 closes the cycle; real records `u249json -> {u249make}`,
  `virtual/u249make -> {u249json}`, `u249make -> {virtual/u249make}`, and
  on the restart the virtual's `||` (expanded inline inside `u249json`'s
  dep_check) finds `u249make` under the *puller's* key
  (`circular_dependency.get(parent)`, `dep_check.py:673-678`), demotes it
  and merges `u249make-bootstrap-1`, `virtual/u249make-0`, `u249json-1`,
  `u249make-1`.
- `--backtrack=0`: the cycle aborts.

Portuale walks the virtual as its own graph node, so its owner key is
real's `virt_parent` half; the fix adds the virtual's pullers' records to
the lookup. While probing, a variant where the puller also depends on the
build tool directly (`BDEPEND="virtual/... dev-util/..."`) aborts on both
sides; after the fix portuale's partial list matches real's (the stray
`virtual` row is gone), but the circular block starts at the other node
of the same cycle — pre-existing, filed #278.
