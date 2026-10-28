# R3 (#594) targets, written before the L0 run (2026-10-27)

Amended scope (premise.md): T1, T2, G0 display gate. Expected output = Portage
captures `captures/A-*` (Portage) vs `P-*` (portuale before).

## Must move (RED, fixtures dev-libs/r594*, recipe mk.sh)
- T1: `--autounmask=n r594ctop` (also `--backtrack=1`) and default `r594etop`:
  Portage 0/20 rc 1, USE-flags block only (portuale 1/20 rc 0).
- T2: default `r594qtop` and `--backtrack=1`: Portage 0/20 rc 1 "backtracking has
  terminated early" (portuale 1/20, 4 rows). Control `--autounmask-backtrack=y`:
  1/20 on both. Keeper `r594ctop` default: 1/20 rc 0.
- G0 display: `dev-libs/p569pull dev-libs/abort-masked-mid` (default,
  `--backtrack=1`, `=0`): no `p569prov:0` slot-conflict block. The count (1/20)
  is R6, not asserted here.
- Out: `r594qtop --autounmask=n` (needs #609's LIFO order).

## Must not move
p/k/l/u/g/v/x cells and variants, c default, d, f, j, installed cells ip/iq/i1,
`--complete-graph` cells; all t8/t10/R1 pins and keepers. Corpus drift: only G0
rows lines may change; read line by line.

## L0 (baseline r1/l0-after.txt = l0-20261027T225721Z)
No move expected. May move, only toward Portage: `kde-plasma_plasma-meta`
(T2 shape), `gnome-base_gnome-shell`, `gnome-base_nautilus`, `app-editors_gedit`.
Any other move is a stop; perl-544-only improvements follow the owner ruling.
