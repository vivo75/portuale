# #602 regression: the complete walk dropped the first walk's backtrack masks (2026-11-07)

The L0 bed run `l0-20261107T041916Z` had one new divergence against
`l0-20261105T215133Z`: `emerge -pv virtual/perl-ExtUtils-MakeMaker` printed
`U 7.780.0` (portuale) where Portage 3.0.83 prints `R 7.760.0`.

Bisect with the bed's one-atom probe (a release build of each commit in a
worktree under `/var/tmp`, copied to `/var/tmp/bedbin`, `PMTEST_PM=portuale-bed`):

| commit | row |
|---|---|
| `f5ca4324` (#611e) | `R 7.760.0` |
| `7a818198` (#603) | `R 7.760.0` |
| `382c9a19` (#602) | `U 7.780.0` |
| `9bad4749` (#605/#671) | `U 7.780.0` |

Cause: the first walk backtracks (masks `perl-5.44.0`, then
`MakeMaker-7.780.0`, whose `|| ( =dev-lang/perl-5.44* ~perl-core/... )` is left
unsatisfiable) and settles on `R 7.760.0`. Its passes held a slot conflict, so
#602 runs the complete walk. That walk started from empty backtrack masks,
re-picked the top-level `7.780.0`, and in complete-locked mode dropped the
unsatisfiable `||` without backtracking. Portage runs `_complete_graph`
inside the depgraph that settled (`depgraph.py` `_resolve_conflicts`, 9473-9480),
under its `runtime_pkg_mask`. The same happened before #602 with an explicit
`--complete-graph` (bed: `U 7.780.0`, Portage `R 7.760.0`).

Fix: `GraphResult::settled_masks` carries the settled node's
`runtime_pkg_mask`; the CLI passes it to the complete walk as
`ResolveRequest::seed_masks`, the search's root params.

Hermetic shape: the existing `p651*` fixtures (`virtual/p651th`,
`virtual/p651enc` mirror the perl virtuals). Captures, `$FX` a copy of the
pmtest fixtures used as both config root and ROOT:

- `p651th-pv.*`: Portage `R p651th-3.500.0-r1`, skip block, "triggered by
  backtracking"; `head-9bad4749` printed `U p651th-3.520.0`; the fix prints
  Portage's rows (count 5/20 vs 4/20: #670).
- `p651enc-pv-complete-graph.*`: the same with `--complete-graph`;
  `head-9bad4749` printed `U p651enc-3.240.0`; the fix prints `R`. The count
  is 0/20 against Portage's 4/20 (#674).

Unrelated finding, filed as #675: `r675-pv.both.txt` (`w602r-repo.json` added
to a scratch copy of the fixtures with `../603/mkfx.py`, ROOT from `r675.json`
with `../603/mkroot.py`, compared with `../603/cmpr.sh`). portuale prints a
"causes rebuilds" block for a consumer that is upgraded anyway; Portage
prints none. Present on `f5ca4324` too.
