# #604 captures (2026-11-07)

Host Portage 3.0.83 (`*-pe.txt`) and portuale with the #604 fix (`*-pu.txt`), same argv
(`run.sh`; `$S` = the scratch dir). Config root: a copy of pmtest `fixtures/` with the
`w604*` fixtures. ROOT: an ad-hoc root (`mkroot.py`) holding only `dev-libs/w604prov-5.42`
(`rB`) or that plus `virtual/w604va-1` (`rA`), like pytest's `_b1_root`.

| shape | ROOT | parent | argv tail |
|---|---|---|---|
| A | `rA` | `w604par` (plain `=5.42*` + virtual) | `dev-libs/w604force dev-libs/w604par` |
| B | `rB` | `w604par` | same |
| C | `rA` | `w604paror` (`|| ( =5.42* w604none )` + virtual) | `dev-libs/w604force dev-libs/w604paror` |
| D | `rB` | `w604paror` | same |

`*-bt0-*`: `-p --backtrack=0 --verbose-conflicts`; others `-p --verbose-conflicts`;
`shared-*`: `--pretend` on the fixtures' own ROOT (the CASES rows). All rc 1, 4/20 with
backtracking on both sides.

With the fix the rows equal Portage's as sets (row order inside one instance is Portage's
`selected_for_display` set order). Before the fix (scratch probes e604/e604c/e604d, same
shapes, `LLM/evidence/2026-11-06-sweep/clusterB.md`): A and C listed the installed virtual
as a `=5.42*` parent and printed the block twice, D missed the virtual's `=5.42*` row.
Remaining difference in A and C: the installed virtual's own `=5.44*` row (backlog #606).
