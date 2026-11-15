# Merge-gate evidence: backlog/753-bootstrap-2a

Slice changes what gets merged (a temporary reduced-USE merge plus a
definitive reinstall of the same slot), so the merge-path safety gate
applies (`agent-context.md`, plan `02.753-bootstrap-2a` §4). Product
change: the resolver synthesizes a marked boot row after the sort and
re-renders the cycle report over the reduced edge set; the fast plan
inserts a boot pass. No ebuild-execution, unmerge, VDB, or binpkg path
changed (the `bootstrap: None` literals elsewhere are mechanical struct
fills for the new `GraphEntry` field).

- L1 merge-from-binpkg (`l1-20261114T183539Z`, rc 0): hard findings 0,
  UNEXPLAINED 0, payload diffs 0, 0 failures introduced by the merge
  (927 mtime-only diffs, non-fatal). Bed log:
  `differential-test-bed/logs/l1-20261114T183539Z/l1-report.txt`.
- sys-libs/glibc + app-shells/bash test merges
  (`differential-test-bed/atomlists/l1-merge-gate.txt`):
  - first run (build into the shared pkgcache,
    `l1-20261114T184257Z`, rc 0): both packages merged (merged_count 2),
    hard findings 0, UNEXPLAINED 0, payload diffs 0 (1420 mtime-only
    diffs, non-fatal), health of the installed state 0 failures
    introduced. Bed log:
    `differential-test-bed/logs/l1-20261114T184257Z/l1-report.txt`.
  - `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1` (force `--reinstall-atoms`,
    both PMs re-merge the pair over their own container,
    `l1-20261114T192008Z`, rc 0): both packages merged (merged_count 2),
    hard findings 0, UNEXPLAINED 0, payload diffs 0 (1420 mtime-only
    diffs, non-fatal), health of the installed state 0 failures
    introduced. Bed log:
    `differential-test-bed/logs/l1-20261114T192008Z/l1-report.txt`.
  - health of the installed state (`health_checker.py --installed`,
    before and after): 0 failures introduced on both runs (see above).

## Post-review re-verification (commits 9c8fadc6 / pmtest 530ce18)

The review fixes touch the merge path (scheduler wait edges, per-entry
build USE), so the safety gate re-ran on the committed binary
(`target/release/portuale` sha256
`cfd1e024e1ab4cf18c6896f47e1d952360d406733540390d4ba6cbe2bb28db4d`;
`pm.json` in each run below pins `version: 9c8fadc6` with that sha).

- L0 resolver (`l0-20261115T003208Z`, rc 0): 120 probes, 91 clean,
  UNEXPLAINED 0, same 2 unused allowlist entries as the pre-slice
  baseline. Bed log:
  `differential-test-bed/logs/l0-20261115T003208Z/l0-report.txt`.
- L0 health (`l0-health-20261115T003208Z`, rc 0): 122 plans, 113
  checked, 262 finding instances / 75 unique — identical to the
  baseline. Bed log:
  `differential-test-bed/logs/l0-health-20261115T003208Z/health-report.txt`.
- L1 merge-from-binpkg, gate atomlist
  (`differential-test-bed/atomlists/l1-merge-gate.txt`,
  `l1-20261115T003210Z`, rc 0): merged_count 2 (glibc + bash), hard
  findings 0, UNEXPLAINED 0, payload diffs 0 (1420 mtime-only diffs,
  non-fatal), installed-state health 0 failures introduced. Bed log:
  `differential-test-bed/logs/l1-20261115T003210Z/l1-report.txt`.

The L1 bed cannot exercise a bootstrap shape (Portage builds the set
from source first and cannot build a cyclic set at all), so the
bootstrapping-shape merge gate is the new real-merge regression test
(`crates/portuale/src/emerge_build/tests/bootstrap_merge.rs`: boot-only
ROOT plus the full 3-row plan at -j1 AND -j2), green in the workspace
run above.
