# Merge-gate evidence: review/opencode-fixes

The branch touches `crates/portuale/src/emerge_build/` (S1.5 #759: a resume
keeps the bootstrap marker until the definitive row merges; S1.6: the failure
summary names an interrupted bootstrap), so the merge-path safety gate applies
(`agent-context.md`, plan `02.759-764-opencode-review-fixes` gates). No
unmerge, VDB or binpkg code changed. Gate run on portuale `46a238b9`,
pmtest `11ac29c`, Portage 3.0.83, frozen binary. Reports are under
`differential-test-bed/logs/<run id>/` in pmtest.

- L0 resolver (`l0-20261115T204204Z`, rc 0): 120 probes, UNEXPLAINED 0,
  0 invariant violations, no cell changed verdict against
  `l0-20261115T003208Z`. Report: `l0-report.txt` in that directory.
- L1 merge-from-binpkg, default list (`l1-20261115T204812Z`, rc 0):
  merged_count 18/18, hard findings 0, UNEXPLAINED 0, payload diffs 0
  (927 mtime-only, non-fatal), health of the installed state 0 failures
  introduced. Report: `l1-report.txt` in that directory.
- sys-libs/glibc + app-shells/bash test merges
  (`differential-test-bed/atomlists/l1-merge-gate.txt`):
  - `l1-20261115T205516Z`, rc 0: sys-libs/glibc-2.43-r4 and
    app-shells/bash-5.3_p20-r1 both merged, merged_count 2/2, hard 0,
    UNEXPLAINED 0, payload diffs 0 (1420 mtime-only), health 0 failures
    introduced. Report: `l1-report.txt` in that directory.
  - `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`, `l1-20261115T212837Z`, rc 0:
    both packages re-merged (sys-libs/glibc and app-shells/bash),
    merged_count 2/2, hard 0, UNEXPLAINED 0, payload diffs 0, health 0
    failures introduced. Report: `l1-report.txt` in that directory.

Other gate results: fmt, clippy (0 warnings), build, `cargo test` (3554
passed, 0 failed), pmtest suite (5005 passed, 0 failed, 2 skipped,
2 xfailed, no corpus drift).
