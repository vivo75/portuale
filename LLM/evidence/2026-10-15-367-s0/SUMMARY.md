# #367 S0 SUMMARY (2026-10-15): Portage oracle matrix for the pg367 fixture

Provenance: Portage 3.0.82.2 (host `emerge`, Q10 fallback -- see cmd.txt);
portuale f221c03f, binary mtime 2026-10-15 19:18:07 UTC (no rebuild);
pmtest f8c4bff with 9 uncommitted pg367 fixture paths. 36/36 cells rc=0.
All rows below are the verbatim `[ebuild]` lines; `N`/`R`/`NS` spellings
are Portage's own. No masked/error footers in any cap file.

## Fixture (pmtest, uncommitted, collision-checked: no pg367 name existed)
- `fixtures/repo/dev-libs/pg367-top/pg367-top-1.ebuild`: EAPI=8, SLOT=0, KEYWORDS="amd64", RDEPEND="dev-libs/pg367-s" (not installed)
- `fixtures/repo/dev-libs/pg367-s/pg367-s-1.ebuild`: SLOT=1 (installed in shared vdb); `pg367-s-2.ebuild`: SLOT=2 (not installed)
- `fixtures/repo/dev-libs/pg367-itop/pg367-itop-1.ebuild`: installed-parent variant, same RDEPEND (installed in shared vdb, vdb carries RDEPEND like other parent entries)
- md5-cache entries for all four ebuilds (`_md5_` = md5 of exact ebuild bytes, verified format-identical to pg34 entries); neither package in world

## Matrix 1: dev-libs/pg367-top (new parent), all with --getbinpkg=n --usepkg=n

| mode | Portage | portuale default | portuale --solver=pubgrub | verdict vs plan S0 expectation |
|---|---|---|---|---|
| -pv | N top | N top | N top | matches |
| -pvu | N top | NS s-2 + N top | N top | matches (incl. the predicted default-solver divergence) |
| -pvD | N top | N top | NS s-2 + N top | matches (incl. the predicted pubgrub -pvD divergence) |
| -pvuD | NS s-2 + N top | same | same | matches |
| -pve | NS s-2 + N top | same | same | matches |
| -pvuDN | NS s-2 + N top | same | same | matches |

## Matrix 2: dev-libs/pg367-itop (installed parent)

| mode | Portage | portuale default | portuale --solver=pubgrub | verdict vs plan S0 expectation |
|---|---|---|---|---|
| -pv | R itop | R itop | R itop | matches |
| -pvu | (no rows) | (no rows) | (no rows) | matches (`[]` on all three) |
| -pvD | R itop | R itop | NS s-2 + R itop | matches (pubgrub adds NS s-2, Portage/default keep R itop) |
| -pvuD | NS s-2 | same | same | matches (parent row absent on all three) |
| -pve | NS s-2 + R itop | same | same | matches |
| -pvuDN | NS s-2 | same | same | matches (parent row absent on all three) |

(Totals cross-check over all 36 caps: 3x "Total: 0", 6x "1 in new slot",
7x "1 new", 5x "1 reinstall", 4x "1 new slot + 1 reinstall",
11x "1 new + 1 new slot" -- every cell accounted for.)

## Stop rules
- Stop rule 1 (Portage -pvuD not `NS s-2`, or pubgrub -pvuD already differs
  from Portage): NOT FIRED. Portage -pvuD is `NS s-2 (+ N top / nothing)`,
  pubgrub -pvuD is identical to Portage on both atoms. The shape isolates
  the `set_prefer_newest_slot` switch as predicted.
- Stop rule 2 (Portage -pvD shows `NS s-2`, or Portage -pvu is not `N top`
  alone): NOT FIRED. Portage -pvD is `N top` / `R itop` with no new-slot
  row, and Portage top -pvu is `N top` alone. Consequences per the plan:
  (a) the S1 fix gate is OPEN -- `(deep && req.update) || req.empty` may
  proceed; (b) the default-solver -pvu new-slot divergence (top -pvu:
  default plans `NS s-2`, Portage plans `N top` alone) MAY be filed as a
  residue (next free number >= 374, cross-tier grep at filing time).

## Notes for S1/S2
- The mutant-kill pin is top -pvuD (pubgrub currently matches Portage;
  `deep && req.empty` would drop the NS s-2 row); the fix-verdict pins are
  top -pvD and itop -pvD (pubgrub currently adds NS s-2, Portage does not).
- The `:*` spelling was not re-captured (plan S0 cells use the bare dep,
  which §0.3 already reports as identical rows).
- No code, backlog, or plan files touched; no commits/branches; scratch
  /var/tmp/s367 deleted after the run.
