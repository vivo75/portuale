# #236 — real Portage probes (batch-2026-09-28_236_107, Slices A/B)

Real's own `ResolverPlayground` (`3rdparty/portage` @ `1d95fc2c`, Portage
3.0.82.2 tree), run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH`, a
copy of `lib/portage/tests/.gnupg` as `PORTAGE_GNUPGHOME`, and `PGDEBUG=1`
for `--debug` output (see pmtest's `resolver-playground-oracle` notes).
Captured 2026-09-29.

- `r25_pg.py r25` → `r25-debug.log`: the #25 r25 shape (installed
  `r25mid` binds `r25lib` through live `r25lib:=` + built
  `>=r25lib-1.0:0/1=`; installed world consumer `r25consumer` pins
  `<r25lib-2.0:=`), `-uDN --oneshot dev-libs/r25target`. Both of r25mid's
  atoms resolve to the **installed** `r25lib-1.0` (`_minimize_children`),
  the update probe runs and is refused by `r25consumer`'s `<2.0`
  (`_slot_operator_check_reverse_dependencies`), no backtrack: merge list
  `r25up-2.0`, `r25target-1.0`.
- `r25_pg.py slotop` → `slotop-debug.log`: the slotop argument shape
  (installed `provpkg-1.0:0/1`, tree `2.0:0/2`, installed `consrdep` with
  live `provpkg:=` + built `>=provpkg-1.0:0/1=`), `-uDN --oneshot
  dev-libs/consrdep`. Both atoms bind the installed `provpkg-1.0`; the
  probe finds `2.0` ("new child package"), `backtracking due to missed slot
  abi update`, and the restart merges `U provpkg-2.0` + `rR consrdep-1.0`
  through `@__auto_slot_operator_replace_installed__`.
- `r25_pg.py slotopw` → `slotopw-debug.log`: the same with `@world`.
- `lg_pg.py` → `libgit2-glib.log`: the fixture's `dev-libs/libgit2-glib`
  (ebuild parent, `<libgit2-1:0= >=libgit2-0.26.0`, nothing installed):
  `_minimize_children` drops `libgit2-1.0.0` for the `>=` atom, so real
  merges `libgit2-0.99.0-r1` + `libgit2-glib-0.99.0.1` with no problems
  block (portuale before Slice B: the same rows plus a skipped-update
  warning).
