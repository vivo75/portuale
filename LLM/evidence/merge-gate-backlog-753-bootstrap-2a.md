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
