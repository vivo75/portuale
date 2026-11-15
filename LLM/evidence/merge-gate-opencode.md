# Merge-gate evidence: branch `opencode`

`LLM/tools/pair_check.sh` names its evidence file after the current branch,
so the integration branch needs its own. This one covers every merge-path
change `opencode` carries over its merge-base with `main`
(`841e6633`): as of `cf4dabdd` that is **#753 only** — two commits,
`04968f8b` (the bootstrap representation) and `9c8fadc6` (the review fixes
to the scheduler and the per-entry build USE), touching
`crates/portuale/src/emerge_build/{scheduler,entry_env,failure,resume}.rs`,
`crates/portuale/src/emerge_getbinpkg.rs` and their tests. The full
per-slice narrative is `LLM/evidence/merge-gate-backlog-753-bootstrap-2a.md`;
this file records the runs that apply to the branch head and names both
safety-gate packages, as the checker requires.

## The safety gate: `sys-libs/glibc` + `app-shells/bash`

Bed: `differential-test-bed/run/l1-merge-from-binpkg.sh` with
`differential-test-bed/atomlists/l1-merge-gate.txt` (the two packages every
process maps). Run `l1-20261115T003210Z`, rc 0, on the committed tree —
portuale `9c8fadc6`, binary `target/release/portuale` sha256
`cfd1e024e1ab4cf18c6896f47e1d952360d406733540390d4ba6cbe2bb28db4d`
(`pm.json` in the run pins `version: 9c8fadc6`):

- `sys-libs/glibc` **merged** and `app-shells/bash` **merged**
  (`merged_count 2` on both the Portage and the portuale side);
- hard findings 0, explained 0, **UNEXPLAINED 0**, payload diffs 0
  (1420 mtime-only diffs, non-fatal);
- installed-state health: **0 failures introduced by the merge**.

Report: `differential-test-bed/logs/l1-20261115T003210Z/l1-report.txt`.
Earlier runs of the same gate on the pre-fix tree
(`l1-20261114T184257Z`, `l1-20261114T192008Z`, `l1-20261114T183539Z`) are
recorded in the per-slice file; they agree.

## Supporting runs on the same binary

- L0 resolver `l0-20261115T003208Z`, rc 0: 120 probes, 91 clean,
  UNEXPLAINED 0, the same 2 unused allowlist entries as the pre-slice
  baseline.
- L0 health `l0-health-20261115T003208Z`, rc 0: 122 plans, 113 checked,
  262 finding instances / 75 unique — identical to the baseline.

## Why L1 is not the whole gate here

The L1 bed cannot exercise a bootstrap shape: Portage builds the set from
source first and cannot build a cyclic set at all. The bootstrapping-shape
merge gate is therefore the real-merge regression test in
`crates/portuale/src/emerge_build/tests/bootstrap_merge.rs` — a boot-only
ROOT (marker and VDB `USE` lack the reduced flag) and the full 3-row plan
at **`-j1` and `-j2`** (every row lands, the survivor carries the declared
`USE`) — green in the workspace run of `gate.sh` on this branch.

## Combined gate on this branch head

`LLM/tools/gate.sh` on `opencode` at `cf4dabdd`+: fmt clean, clippy 0
warnings, workspace 3543 passed / 0 failed, contract suite 4960 passed /
2 skipped / 2 xfailed. `doc_status_check.py` reports the 39 pre-existing
violations (unchanged), `skills_check.py` and `cut_registry_check.py`
clean.
