# #366 bed run on main (2026-10-15)

Binary: `target/release/portuale` built from main at portuale `83a0375e` (pmtest `f8c4bff`).
Six `l0-resolver.sh` runs in parallel, started 60 s apart, all with
`GENTOO_MIRRORS=http://10.48.0.229:8080 L0_SKIP_MULTI=1 L0_SKIP_INVARIANTS=1` and
`L0_EMERGE_OPTS="<mode> --getbinpkg=n --usepkg=n"`. The five pubgrub runs add
`L0_PORTUALE_OPTS=--solver=pubgrub`; the default run uses `-pv` without it.
Counted with `resolve-compare.py --count-categories missing,extra,version,action,exit <rundir>
differential-test-bed/compare/known-divergences.yaml` (unexplained only, 117 probes each).

| run | pmtest log dir | missing | extra | version | action | exit |
|---|---|---|---|---|---|---|
| pubgrub `-pv` | `l0-20261015T192320Z` | 0 | 0 | 0 | 0 | 0 |
| pubgrub `-pvu` | `l0-20261015T192420Z` | 0 | 0 | 0 | 0 | 0 |
| pubgrub `-pvuD` | `l0-20261015T192520Z` | 0 | 0 | 0 | 0 | 0 |
| pubgrub `-pvuDN` | `l0-20261015T192620Z` | 0 | 0 | 0 | 0 | 0 |
| pubgrub `-pve` | `l0-20261015T192720Z` | 0 | 0 | 0 | 0 | 0 |
| default `-pv` | `l0-20261015T192820Z` | 0 | 0 | 0 | 0 | 0 |

- The allowlist entry `pubgrub-or-order-blind-virtual-pager` is gone, so `-pve` is clean without it.
- The whole run took about 10 minutes in parallel; no container timeouts.
- Out of gate: 39–141 unexplained findings per run remain in the non-gate categories
  (`order`, `use`), the known informational rows.
- Open question: the default run was recorded earlier as 448 extra; it now counts 0.
  The earlier counting method was not re-checked, so this is not evidence that the default
  solver is unchanged.
