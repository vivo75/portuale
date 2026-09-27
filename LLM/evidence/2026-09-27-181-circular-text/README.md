# #181 — live real text for the circular-dependency CASES (2026-09-27)

Bed: `differential-test-bed/run/l0-fixture-oracle.sh` on a temporary list
holding the eight `test_circular_dependencies` pg0 cases from #50 batch 2
(`=dev-libs/cyc0z-{1,2,3}`, `=dev-libs/cyc0w-{1,2,3}`, `=dev-libs/cyc0b-{1,2}`),
run id `l0-fx-20260927T202111Z`, portuale `main` at `b8126446`, real
Portage 3.0.82.2 in the stock container image, `emerge -p --color=n`.

`real/` and `portuale/` hold the verbatim captures (stdout and stderr
together); `meta.tsv` has both exit codes (1 on both sides for all eight);
`l0-report.txt` is the bed's own merge-list comparison.

The batch-2 CASES labels say the suggestions "match the oracle solutions".
That oracle was the `ResolverPlayground` result object, not live `emerge`.
Live `emerge` disagrees with portuale in three separate ways:

| case | exit | cycle nodes | suggestion | filed |
|---|---|---|---|---|
| all eight | same | real prints `(cpv:slot/sub::repo, ebuild scheduled for merge)`, portuale prints the bare cpv; real re-displays the merge list in forced `--verbose --tree` form | — | #206 |
| `cyc0z-1`, `cyc0z-2` | same | real backtracks once (`backtrack: 1/20`) and schedules `cyc0z-3` for `cyc0y`'s unversioned `DEPEND` next to the argument; the cycle is `cyc0z-3 ↔ cyc0y-1` | real `cyc0z-3 (+bar -foo)`; portuale names the argument (`cyc0z-1` gets two one-flag changes) | #207 |
| `cyc0z-3`, `cyc0b-1`, `cyc0b-2` | same | same pair (format only, #206) | same | — |
| `cyc0w-1`, `cyc0w-2` | same | real starts the cycle at `cyc0y-1`, portuale at `cyc0z-3` | same text | #208 |
| `cyc0w-3` | same | as `cyc0w-1` | real finds `cyc0z-3 (Change USE: -foo)` on the autounmasked `USE="bar foo"`; portuale prints the generic advisory | #208 |

The playground's expected solution for `=Z-1` (`{-foo}` or `{+bar}` on
`Z-1`) matches portuale, not live `emerge`. The difference is the backtrack
run that live `emerge` takes; the playground result may come from a
different backtrack budget. That is part of #207's diagnosis.
