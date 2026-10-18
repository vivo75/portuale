# S404 matrix — backlog #404 (02.346 S11) oracle captures

Evidence: this dir (`l/`). Scripts: `scripts/` (`lib.sh` + `l.sh`;
S0-derived, no `CLEAN_DELAY` anywhere by design). Oracle: Portage
3.0.82.2 ("Portage"); portuale at `5674bc11` (mtime 2026-10-18
12:17:53 UTC, never rebuilt). All `emerge` cells carry
`--package-moves=n` on both sides.

Legend: **match** = §1 verdict confirmed as stated (fix stands).
**NEW** = behaviour the plan did not spell out (all go in the report).

| cell | setup | Portage | portuale (pre-fix) | verdict |
|---|---|---|---|---|
| cl01 | `AUTOCLEAN=yes`, 7 installed, emerge 8 (replace consumes 7) | rc 0, `>>> Auto-cleaning packages...` / `>>> Using system located in ROOT tree <root>/` / `>>> No outdated packages were found on your system.` | rc 0, nothing | match → S11 (trigger text) |
| cl01c | `AUTOCLEAN=yes`, restored leftover 7 + world, merge unrelated phaseslim | rc 0, same two header lines + selection table (`selected: 7` / `protected: 8` / `omitted: none`), `All selected packages: =app-misc/t348-b-phases-7`, the two `>>> … slated …` lines, `>>> Unmerging (1 of 1) app-misc/t348-b-phases-7...`, 7 gone from vdb | rc 0, nothing (7 stays) | match → S11 (the unmerge arm) |
| cl02 | `AUTOCLEAN=yes`, fresh merge | rc 0, same two headers + `No outdated…` | rc 0, nothing | match → S11 |
| cl03 | AUTOCLEAN unset, fresh merge | rc 0, no autoclean block | rc 0, no block (same bytes modulo build output) | match (scope: unset prints nothing) |
| cl04 | `AUTOCLEAN=no`, fresh merge | rc 0, no autoclean block | rc 0, no block | match (scope: `=no` prints nothing) |
| cl05 | `AUTOCLEAN=yes`, leftover 7 with SLOT edited to `1` (stale `metadata` deleted), merge phaseslim | rc 0, headers + `No outdated…`, both 7 and 8 survive | rc 0, nothing (both survive) | match → S11 (other-slot instances are protected, never selected) |
| cl06 | `AUTOCLEAN=yes` + `--pretend` | rc 0, merge list, no autoclean block | rc 0, no block | match (scope: `--pretend` never triggers) |
| cl07 | `AUTOCLEAN=yes` + failing merge (`die` in `src_install`) | rc 1, build-log tail, no autoclean block | rc 1, no block | match (scope: a failed merge never triggers) |

Cross-cell rules (all pinned by the captures above):

- "Outdated" = an older same-CP+slot instance only (cl01b S0: an
  unneeded package is not outdated; `--clean` is not `--depclean`).
  cl01c selects 7 (older, same slot), protects 8; cl05 protects both
  when the slots differ.
- The autoclean run has NO countdown on either side: no `CLEAN_DELAY`
  was set anywhere (default 5 would apply), and no capture contains a
  `Waiting` line — including the cl01c unmerge arm
  (`unmerge.py:638`, `not autoclean`).
- `--oneshot` does not suppress the run (cl01/cl01c/cl02/cl05 all use
  it); only the forced-yes couplings (global updates, depclean) are out
  of scope, and every cell runs with `--package-moves=n`.
