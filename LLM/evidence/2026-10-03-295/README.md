# Evidence 2026-10-03 — #295 restart-budget accounting (real 3.0.82.2 vs portuale)

Probe: `oc-290/probe/probe-in.sh` format, cells in `cells.txt` (#293 shapes B/A/C at `--backtrack` 1/2/3) and
`cells-btg.txt` (`testBacktrackingGoodVersionFirst` shape at 1..4). Compare `[ebuild` rows and rc.

| cell | real | portuale (after #295) |
|---|---|---|
| B/A/C-bt1 | rc 1, full list (best run = the replace-set node, `prune_rebuilds` wanted but not takeable) | same rows, rc 1 |
| B/A/C-bt2, B-bt3 | rc 0, `--backtrack=0` rows (bt2) / full list (bt3) | same rows, rc 0 |
| G-bt1, G-bt2 | rc 1, conflict stands | same |
| G-bt3 | rc 1 (needs 4 restarts) | **rc 0**, masked graph "triggered by backtracking" -- residue |
| G-bt4 | rc 0 `[btgc-1, btgb-1, btgp-1]` | same |

Real's backtracker trace for B-bt2 (monkeypatched `get_best_run`): nodes root, `replace_installed={pcons,pprov}`
(depth 1), `prune_rebuilds` (depth 2, empty replace set) -> best run = the depth-2 node.
