# STOP-0: runbook 2, Task 1, Step 4 (before-probe check)

## Rule
STOP-0 (`LLM/02.770-runbook.md`, Task 1 Step 4): "`0` means the default solver lost the row:
STOP-0 as well." Also: "`2` means root `pull` no longer reproduces: STOP-0."

## Command (verbatim, from the runbook)
    grep -A3 "pull-uDN/" LLM/evidence/02.770/before/probes.txt | grep -c "rR.*p569virt-1"

## Expected output
    1

## Actual output (verbatim)
    0

## Diagnostic (run after the STOP fired; not a substitute for the command above)
The runbook's `-A3` window ends before the `[ebuild  rR ]` row: in the `pull-uDN/df` block
the `argv:` line is line 1 and the `rR` row is line 9 of the block. With `-A8`:

    grep -A8 "pull-uDN/" /var/tmp/770/raw/before1/probes.txt | grep -c "rR.*p569virt-1"   -> 1

The raw default-solver output contains the row (`/var/tmp/770/raw/before1/pull-uDN.df.out`,
line 13):

    [ebuild  rR    ] dev-libs/p569virt-1::testrepo to /var/tmp/770/roots/pull 0 KiB

and the pubgrub output of the same probe has no `p569virt-1` row
(`/var/tmp/770/raw/before1/pull-uDN.pg.out`, grep for `p569virt` returns only the
`p569prov-5.44` row and the `p573virt-1` NOTE line).

So: root `pull` reproduces, and the default solver does not lose the row. The count 0 comes
from the window of the command, which the runbook wrote too narrow. This file records the
literal STOP-0 trigger; the decision whether it is a false trigger is left to the owner or
coordinator.

## Probe runs (both identical)
- `LLM/evidence/02.770/probe.sh` run twice: `/var/tmp/770/raw/before1/probes.txt` and
  `/var/tmp/770/raw/before2/probes.txt`; `diff` printed nothing (IDENTICAL). 28 `argv:` blocks
  each (14 runs x 2 solvers).
- Each run was in a scope (`770-probe1`, `770-probe2`); both scopes exited 1. Cause: the
  script's last line `[ -d /var/tmp/770/roots/twohop ] && run ...` returns 1 because
  `twohop` is absent from `/var/tmp/770/roots` (`mkroots.sh` did not build it). The
  probes themselves completed.

## What had been changed so far
- Branch `backlog/770-pubgrub-walked-stale-rebuild` created in `portuale` and in `pmtest`,
  from `backlog/768-769-fast-plan-policy` (pmtest `5ec1f1b8`, portuale `7e4ccb49`).
- Created (untracked): `LLM/evidence/02.770/` with `before/NOTES.txt`, `before/inventory.txt`,
  `before/probes.txt` (copy of before1), `probe.sh`, and this file.
- Scratch created: `/var/tmp/770/` (fx, roots, bin, raw, probe logs, mkroots.log).
- No product code, fixture, test, pin or allowlist edited. No commit. No `git stash` or `git reset`.

## Resolution (coordinator, 2026-11-22)

False trigger. The runbook's `grep -A3 "pull-uDN/"` window is too short: the `rR` row of
`p569virt-1` sits 8 lines below the `argv:` line of the block. With `-A8` the count is `1`
(the default solver prints the row; the pubgrub block of the same probe does not, as the
agent's raw files show). Root `pull` still reproduces exactly as plan 02.770 §0 says. The
runbook command is corrected to `-A8`. The two probe scopes exited 1 only because the last
line of `probe.sh` (`[ -d .../twohop ] && run ...`) returns 1 when the optional `twohop`
root does not exist; it is now an `if ... fi`, so the exit status is 0. Task 1 continues.
