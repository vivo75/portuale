# Bed run for S5 of #369 (deferred disjunction stack)

Branch `backlog/369-deferred-disjunctions` (portuale `f8c76bcf`, pmtest
`0d5b63a`), run 2026-10-17 as the owner required before any merge.
Baseline: the same cells on the 359 side earlier the same day.

| Cell | Baseline run | S5 run | Verdict |
|---|---|---|---|
| `l0-resolver.sh` | `l0-20261017T092303Z`: 31 UNEXPLAINED, 8 explained | `l0-20261017T205457Z`: 31 UNEXPLAINED, 6 explained | identical unexplained set; no new `[order]` row (17 in both) |
| `l0-fixture-oracle.sh` | `l0-fx-20261017T093034Z`: 4 UNEXPLAINED, 19 explained | `l0-fx-20261017T210235Z`: 4 UNEXPLAINED, 19 explained | identical (the 4 `blk0*` skipped-updates rows, #399) |

Two allowlisted rows of `gnome-shell-cluster-a-abort` disappeared
(`[extra]` dev-lang/nasm-3.02 and `[totals]` 453 vs 454): the deferred
stack no longer pulls the extra package. Fewer divergences, none new.

The allowlist entry `slotop-world-complete-graph-order` matched nothing
in the fixture-oracle run; the baseline report says the same.

No new UNEXPLAINED or `[order]` row, so the hard stop did not trigger.
The branch stays unmerged; the autounmask-use flip path is #398.
