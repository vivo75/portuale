# Evidence 2026-10-03 — #289 world slot atom holds the old instance

Cells `cells.txt` (installed `bdprov-1:0/1`, `bdprov-2:1/1`, tree `bdprov-3` `1/2` soft-blocking slot 0), real
3.0.82.2 vs portuale (L0 container probe), `*-before-*` = main 2c8cf3cb, `*-after-*` = this fix.

| cell | real | portuale before | portuale after |
|---|---|---|---|
| E0 base | U + uninstall + `[blocks b]`, rc 0 | same | same |
| E4 world `bdprov:0`, `@world` | U + `[blocks B]`, rc 1 | U + uninstall + `[blocks b]`, rc 0 | = real |
| E5 world `bdprov:0` | U + `[blocks B]`, rc 1 | U + uninstall + `[blocks b]`, rc 0 | = real |
| E1/E2/E3/E6 holder `bdhold` in world | U + `[blocks B]`, rc 1 | does not update `bdprov` (E1/E2 none; E3/E6 other rows only), rc 0 | unchanged (not modelled) |

E1/E2/E3/E6 (an installed holder the walk never visited) stay open: see #296.
