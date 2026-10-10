# #250 — fixture first (owner B6): the USE-mismatched installed instance

Real's own `ResolverPlayground` (`3rdparty/portage`, Portage 3.0.82.2
tree), `PATH=$PWD/bin:$PATH`, a copy of `lib/portage/tests/.gnupg` as
`PORTAGE_GNUPGHOME`. Captured 2026-09-30.

Shape: bug 703440's cycle — `dev-util/u250make` BDEPENDs
`dev-libs/u250json:0=`, `u250json` BDEPENDs
`|| ( dev-util/u250make-bootstrap dev-util/u250make[foo] )` — with
`u250make` already installed but built `-foo` (tree ebuild `IUSE=+foo`).

- `u250_pg.py` → `u250.txt` (bootstrap listed first, the pmtest fixture)
  and `u250r_pg.py` → `u250r.txt` (the `[foo]` branch listed first): in
  both orders and for all three entry points (`u250make`, `u250json`,
  `-uDN u250make`) real merges `u250make-bootstrap-1` and `u250json-1`
  (plus the `u250make` reinstall when it is the target). The installed
  `-foo` build does not satisfy `u250make[foo]` (`vardb.match_pkgs`
  honours use-deps), so the build edge is unbreakable, the cycle forms and
  the circular restart demotes the branch.

Before the fix portuale reinstalled `u250make` with `foo`, ordered
`u250json` first and never saw the cycle, in both orders: its build-edge
satisfaction check (`run_pass`'s `edge_kind_map`) was version/slot only.
With that check USE-aware, both orders match real — and the merge-order
`||` suppression the backlog entry names (`suppressed_alt_edges`, also
version/slot only) never reopens a phantom edge on either order: that
half is closed by this proof, with no `Config` threading.
