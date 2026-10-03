# #270 S0 — real vs portuale at `--backtrack` 0/3/20 (2026-10-02)

Captured by `probe/probe-host.sh` + `probe/probe-in.sh` (container
`localhost/test-portuale:latest`, real Portage 3.0.82.2, staged fixture tree via
the bed's `stage.sh`, ad-hoc ROOT with EAPI-bearing vdb — the #253/#269 recipe,
`--ignore-default-opts --color=n`, `PYTHONHASHSEED=0`). Provenance: `provenance.txt`
(portuale `24564d39`; the "dirty" count there is only untracked 3rdparty symlinks).
The 12 cells are `probe/cells-s0.txt`; only the discriminating ones are kept here
(`real/`, `portuale/`; real's `.err` has the image's license-group noise stripped).
Rows below are the `[ebuild` lines with the `to <root>` suffix removed.

| Cell | Result |
|---|---|
| A `pprov` requested, bt 3 and 20 | rows identical (5 rows) |
| **A `pprov` requested, bt 0** | **DIFF — the #270 target**: real `[sct-1.0, oldc, newc]`; portuale adds `U pprov-2 [1]` |
| B `pprov` not requested, bt 0 | rows identical (3 rows) |
| **B `pprov` not requested, bt 3 and 20** | **DIFF, not #270**: real `N sct`, `r U pprov-2 [1]`, `rR pcons-1`, oldc, newc + "causing rebuilds"; portuale `N sct`, `N pprov-2 [1]`, oldc, newc |
| C, D (`mmprov`/`mmcons`), bt 0/3/20 | rows identical in all six |

## What S0 changes in the plan

1. **The target shape is cell A (`pprov`/`pcons`, the #253 pin's own cell), not
   `mmprov`/`mmcons`.** The `mm*` cells already agree at bt0 — they are guards.
2. **Real's bt0 WARNING carries two conflict blocks**, not one: the
   `slotconflicttarget:0` block (portuale prints it) *and* an `app-misc/pprov:0`
   block (`pprov-2` scheduled, conflicts with `>=app-misc/pprov-1:0/1=` required
   by installed `pcons-1`). The second is exactly what recording the conflict at bt0
   (gap a) produces; S4/S5 acceptance must cover it, not only the row set.
   (The rows of real's WARNING go to stderr, portuale's to stdout — standing difference.)
3. **Cell B at bt>0 is a separate pre-existing divergence** (a pulled-in provider
   with a slot-operator consumer: real schedules `r U pprov-2` + `rR pcons-1`,
   portuale a plain `N pprov-2`). Not touched by #270; to be filed as its own
   backlog item in S7 (next free number checked then).
