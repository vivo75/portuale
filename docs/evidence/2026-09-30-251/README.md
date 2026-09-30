# #251 — three `circular_dependency` corners (batch-2026-09-28 Track O)

Real's own `ResolverPlayground` (`3rdparty/portage`, Portage 3.0.82.2
tree), `PATH=$PWD/bin:$PATH`, a copy of `lib/portage/tests/.gnupg` as
`PORTAGE_GNUPGHOME`. Captured 2026-09-30. Fixtures in pmtest under
`dev-libs/u251*` and `dev-util/u251*`.

- **(a) `u251a_pg.py` → `u251a.txt`** — the flat-list blocker disposition
  with a demoted branch: `u251m`'s `|| ( u251ta u251tb )` loses `u251ta`
  to the circular restart; `u251ta` blocks installed `u251olda`,
  `u251tb` blocks installed `u251oldb`. Real shows only `u251tb`'s
  uninstall/blocks pair for `u251m`, both pairs for `u251ta`. Portuale:
  identical rows.
- **(b) `u251b_pg.py` → `u251b.txt`** — `--deep` installed-cp collision:
  installed `u251p-1` (slot 1) and merging `u251p-2` (slot 2) carry the
  same `||`; the cycle record is keyed by the slot-2 node, so only its
  choice demotes. Real merges `u251r-1`, `u251p-2`, `u251q-1` in every
  argument order. Portuale (empty map at the installed deep-walk site):
  identical.
- **(c) `u251c_pg.py` → `u251c.txt`** — every branch demoted: `u251y`'s
  `|| ( u251c u251d ) u251d` with `u251c` already in the graph records
  both branches in one pass; the cycle through the direct dep persists and
  both sides abort. The forced partial list matches in every argument
  order; the circular block matches with `u251c` first, and in the other
  two orders starts at the other node — the rotation class #278.

No corner showed a divergence of its own subject; each is pinned.
