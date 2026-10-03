# Evidence 2026-10-03 — #288 argument pin vetoes the update probe

Cell `D-arg-pin` (`cells.txt`): installed `mmprov-1:0/1`, `mmprov-2:1/1`, `mmcons-1` (bound `mmprov:0/1=`),
`--pretend --update --deep =app-misc/mmprov-1 app-misc/mmcons`, real 3.0.82.2 vs portuale (L0 container probe).

- `real-*.out`: rc 0, `backtrack: 0/20`, no merge rows.
- `portuale-before.out` (main ceff9ad9): `[ebuild U] mmprov-3 [2]` + `[ebuild rR] mmcons-1`.
- `portuale-after.out` (this fix): no merge rows (matches real; portuale prints nothing at all for an empty
  merge list, real prints its header lines -- pre-existing, not row-level).
