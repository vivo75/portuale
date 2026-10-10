# #107 re-probe, 2026-09-27 (batch-2026-09-27 P-R4 step 1)

Host: the new server (32 cores, 2128 installed packages, Portage 3.0.82.2),
portuale release build of `main` @ `8b7890cd`. Command on both sides, as a
non-root user: `emerge -puD --getbinpkg net-libs/rest`.

- `real-full.log` / `ptl-full.log`: stdout+stderr of real and portuale (default).
- `ptl-backtrack0.log`: portuale with `--backtrack=0` (stdout).
- `timing.log`: `/usr/bin/time -v`, three warm runs each (real, portuale default,
  portuale `--backtrack=0`). Portuale's times are not comparable with real's here:
  portuale finds no binhost data on this host (#192) and so skips real's index fetch.
- `host-facts.txt`: nproc, package count, binary, installed weston/libdisplay-info.

Portuale's `--json` reports `"backtrack": {"restarts": 1}` by default and
`{"restarts": 0}` with `--backtrack=0`. Real prints `backtrack: 0/20`.
