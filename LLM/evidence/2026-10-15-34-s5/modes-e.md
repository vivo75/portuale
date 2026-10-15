# Task #34 S5e re-measure (`||` prefers an alternative already in the plan)

Binary: S5e worker build (uncommitted `backlog/34-s5d`). Same procedure as `modes-a.md`
(117 probes, `--getbinpkg=n --usepkg=n`, `L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1`).
Unexplained totals (missing / extra / version / action / exit), S5c in brackets:

| mode | run dir (`differential-test-bed/logs/`) | missing | extra | version | action | exit |
|---|---|---|---|---|---|---|
| `-pv` | l0-20261015T113635Z | 0 (0) | 0 (4) | 0 | 0 | 26 |
| `-pvu` | l0-20261015T114608Z | 0 (0) | 0 (4) | 0 | 0 | 26 |
| `-pvuD` | l0-20261015T115231Z | 0 (0) | 0 (4) | 0 | 0 | 26 |
| `-pvuDN` | l0-20261015T120249Z | 0 (0) | 0 (4) | 0 | 0 | 26 |
| `-pve` | l0-20261015T121422Z | **2** (0) | 0 (10) | 0 | 0 | 26 |
| default `-pvuDN` | l0-20261015T122818Z | 0 | 448 | 0 | 0 | 0 |

Default solver: identical to S5c (0/448/0/0/0).

Exit-code probes: the same 13 as S5c (G4a/G4b/G4c), none changed.

## The one new finding: `virtual/pager` under `-pve` (missing 2)

Portage: `app-misc/editor-wrapper-4-r1` and `sys-apps/less-704` (`R`, from `virtual/pager`
`|| ( sys-apps/less sys-apps/most sys-apps/util-linux[ncurses] ... )`).
portuale: picks `sys-apps/util-linux[ncurses]`, which another parent (systemd) also pulls.
Cause: Portage prefers an alternative only if it is already in the graph at the moment
the `||` is processed (depth-first order); `util-linux` enters the graph after
`virtual/pager`, so Portage takes the first-listed `less`. The bridge fixpoint treats any
independently selected package as "in the graph", so it is order-blind. Net effect of S5e:
-14 extras (4+4+4+4+10 -> 0) against +2 missing in one probe. Filed as #366
(`owner: portuale-bug`), not fixed here: a faithful fix needs Portage's processing order.

## After the allowlist (pmtest `known-divergences.yaml`, layer `l0-pubgrub`)

Re-graded the same run dirs: missing/extra/version/action/exit = 0/0/0/0/0 in all five
modes. 13 exit probes -> #363 (9), #364 (3), #365 (1); `virtual_pager` -> #366.
