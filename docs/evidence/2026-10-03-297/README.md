# Evidence 2026-10-03 — #297 failed iterations count as restarts

`btgp` (`testBacktrackingGoodVersionFirst`) at `--backtrack` 1..4, real 3.0.82.2 vs portuale after the fix; plus the
#295 B/A/C cells (`cells.txt`, all 11 match rows and rc). Real's `Backtracker` trace for bt3 (`real-G-bt3.bttrace`,
`get_best_run`/`feedback` monkeypatched): pass 1 root -> slot conflict (nodes: mask btgc-2, mask btgc-1); pass 2 =
mask btgc-1 -> missing dependency (node: + mask btgp-1); pass 3 = that node: top-level `btgp` masked, the pass fails
with NO feedback, and real's loop still does `elif backtracker: backtracked += 1`; pass 4 = mask btgc-2 fails at
`backtracked 3 >= 3`. Portuale's loop dropped a failed iteration (`run_pass` Err) without counting it, so it reached
the settling mask node one restart early.

Second half (found by the full suite): counting failed iterations alone broke three real-oracle restart pins (`prune_rebuilds`
7/20, autounmask-breakage 3/20, a corpus entry) -- they had matched by two errors cancelling. (a) a failed iteration only
counts when candidates remain (`elif backtracker:`); (b) real's first pass carries the replace set in the same `infos`
as the slot conflict (`['config','slot conflict']`), so portuale folds the replace-set scan into the conflict node
(`P-bt20`: real 7/20; portuale now 7 restarts, rows equal). `needer/othermod` triangle JSON restarts 2 -> 3 (real 4/20).
