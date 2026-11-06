# #663 — autounmask suggests an implicit-IUSE flag (2026-11-06)

Portage 3.0.83 via `l0-fixture-oracle.sh` on `dev-libs/implicitiusepkgmusl`
(plain, `--quiet`, `--verbose`, `--tree`, `--emptytree`, `--update`,
`--update --deep --newuse`) next to portuale after the fix. All rc 1, zero
unexplained findings. Source: `_emerge/Package.py`, `_iuse.get_flag` accepts
`_iuse_implicit_match`, so `depgraph._pkg_use_enabled` flips implicit flags.
The real Gentoo tree avoids the suggestion only because
`profiles/base/use.mask` masks `elibc_musl`; the fixture has no such mask.
