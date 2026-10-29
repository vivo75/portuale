# Merge-path safety gate: branch batch/2026-10-29-easy-six (Task 1, #574)

Recipe: binary frozen from this branch's tree
(`cp -p target/release/portuale /var/tmp/bedbin/portuale`, `cmp`-verified),
then from pmtest:

```
PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 \
  GENTOO_MIRRORS=http://10.48.0.229:8080 \
  differential-test-bed/run/l1-merge-from-binpkg.sh \
  differential-test-bed/atomlists/l1-merge-gate.txt .
```

## Task 1 — the endgame/interactive root lines carry Portage's slash (#574)

Run `l1-20261029T075605Z` (tree 4be8b0f7 + the Task 1 working-tree change:
`emerge_getbinpkg.rs`, `elog.rs`, `emerge_build/status.rs`).

- runner rc 0
- `sys-libs/glibc` (2.43-r4) and `app-shells/bash` (5.3_p20) merged by both
  PMs: `merged_count 2` on both sides, `merge_rc 0` both sides
- hard findings 0, UNEXPLAINED 0, explained 0, payload diffs 0
- mtime-only diffs 1420 (non-fatal)
- the report's `findings` list is empty, so this run surfaced no VDB
  `environment` findings either (the known mtime-only noise is unchanged)

Logs are local-only under pmtest `differential-test-bed/logs/l1-20261029T075605Z/`.
Verdict: PASSED.
