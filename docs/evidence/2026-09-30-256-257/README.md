# #256 + #257 — the unsatisfied-probe family (batch-2026-09-28 Track R)

Real's own `ResolverPlayground` (`3rdparty/portage`, Portage 3.0.82.2
tree), run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH`, a copy of
`lib/portage/tests/.gnupg` as `PORTAGE_GNUPGHOME` and `PGDEBUG=1`.
Captured 2026-09-30.

- `u256_pg.py <a|b> <atoms>` → `u256-<shape>-<atoms>.log`. Two installed
  parents `u256pa`/`u256pb`, each with a built `>=app-misc/u256prov-1:0/1=`
  the tree no longer satisfies (the provider moved to slot `2/2`).
  - **Shape a** (both same-slot ebuilds carry `u256prov:=`): in every
    argument order real logs `backtracking due to unsatisfied built
    slot-operator dep` **twice in the same pass**, then one `backtracking
    try` — `_add_dep`'s `return 1` (`depgraph.py:3453-3455`) ends only
    that edge; the walk continues and probes the other. Merge list
    `u256prov-2` + both reinstalls. This is portuale's one-pass seeding:
    #256's premise (real restarts on the first hit) does not hold.
  - **Shape b** (`u256pb`'s ebuild pins `<u256prov-2:=`): real first
    masks the *installed* `u256pb` for the missing dependency
    (`runtime_pkg_mask` "missing dependency"), then under `@world` drops
    it as an argument ("The following update has been skipped due to
    unsatisfied dependencies", "Problems have been detected with your
    world file") and heals `u256pa` alone (5 tries); with explicit
    arguments both orders fail with "All ebuilds that could satisfy
    app-misc/u256pb have been masked". Portuale aborts on
    `>=app-misc/u256prov-1:0/1=` in all three: it has no
    missing-dependency backtrack for an installed parent. Filed as #276.
- `u257_pg.py app-misc/u257par <opts>` → `u257-usepkg.log`,
  `u257-usepkgonly.log`. A local binary `u257par-1` records
  `app-misc/u257prov:0/1=` (the ebuild's `u257prov:=` bound to a sub-slot
  the tree no longer has; a binary whose recorded dep differs by more
  than the binding is dropped by `--binpkg-changed-deps` before any
  probe). `--usepkg`: one `backtracking due to unsatisfied built
  slot-operator dep`, one restart, `[ebuild N] u257prov-2` + `[ebuild N]
  u257par-1`. `--usepkgonly`: no ebuild replacement exists, the run fails
  on the binary's dep.
