# Evidence 2026-10-03 — #293 exhausted-budget cells (real 3.0.82.2 vs portuale)

Cells (`cells.txt`, probe format of `oc-290/probe/probe-in.sh`: installed set | world | argv), run by
the L0 container probe on `main` ceff9ad9:
`B` = installed `pprov-1`/`pcons-1` (#290 shape, `pprov` not requested), `A` = same with `pprov`
requested, `C` = the `mm*` pair; each at `--backtrack` 1 and 2 (`B` also 3).

| cell | real | portuale |
|---|---|---|
| B-bt3 | rc 0, `backtrack: 3/3`, full list (`r U pprov-2`, `rR pcons`) | same rows |
| B-bt2 / A-bt2 | rc 0, `backtrack: 2/2`, **only** `[sct, oldc, newc]` (= the `--backtrack=0` output) | full list incl. `pprov-2` + `pcons` rebuild |
| C-bt2 | same withhold shape | full list incl. `mmprov-3` + `mmcons` rebuild |
| B/A/C-bt1 | rc **1**, `backtrack: 1/1`, full list, conflict WARNING on stderr | rc 0, full list |

Reading: real's last run after the budget is spent is a no-backtrack run, so the bt2 output equals the bt0
output. Portuale does NOT exhaust at these budgets: it budgets `--backtrack=N` as mask *steps*
(documented deviation at `ResolveRequest::backtrack_max`), real counts restarts (the shape needs 3), so
the divergence is budget accounting, not the rebuild-trigger exemption's gate. A `ctx.backtracking()` flag
(set before the best-run re-pass) moved none of these cells, so #293's gate is unobservable here.
