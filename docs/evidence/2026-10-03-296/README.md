# Evidence 2026-10-03 — #296 installed holder in world holds the old instance

Cells `cells.txt` (the #289 cells; holder `bdhold-1` `RDEPEND=app-misc/bdprov:0` in world for E1/E2/E3/E6),
real 3.0.82.2 vs portuale (L0 container probe). Before the fix (main 9999b0e0; see
`../2026-10-03-289/portuale-before-*`) portuale printed no rows / rc 0 for E1/E2 and no `bdprov` update for
E3/E6. After: all seven cells match real in rows and rc (`*-after-*`). Two changes: an atom with an explicit
slot is not broken by an upgrade in another slot (reverse-dep pins), and installed packages the required sets
reach hold, through their recorded vdb atoms, an instance only a slot atom still selects.
