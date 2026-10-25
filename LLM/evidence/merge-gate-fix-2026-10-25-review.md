# Merge-path safety gate: branch fix/2026-10-25-review (backlog #575)

Recipe (every entry below): binary frozen from the branch tree
(`cp -p target/release/portuale /var/tmp/bedbin/portuale`, `cmp`-verified),
`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 L1_SKIP_BUILD=1
L1_CONSUME_REINSTALL=1 GENTOO_MIRRORS=http://10.48.0.229:8080
differential-test-bed/run/l1-merge-from-binpkg.sh
differential-test-bed/atomlists/l1-merge-gate.txt` from pmtest. Logs are
local-only under pmtest `differential-test-bed/logs/`.

## Task 2 — merge-wait queueing (scheduler.rs)

Run `l1-20261025T223145Z` (tree 6ed0d5fe + Task 2 working-tree change):
runner rc 0; `sys-libs/glibc-2.43-r4` and `app-shells/bash-5.3_p20`
merged by both PMs (`*.merged-cpvs.txt`), merge_rc 0 both sides; hard
findings 0, explained 0, UNEXPLAINED 0, payload diffs 0 (4005 paths per
side). Verdict: PASSED.

## Task 3 — pkg_pretend pass (scheduler.rs, pretend_pass.rs, emerge_getbinpkg.rs)

Run `l1-20261025T234042Z` (tree 523bfd7e + Task 3 working-tree change):
runner rc 0; `sys-libs/glibc-2.43-r4` and `app-shells/bash-5.3_p20`
merged by both PMs, merge_rc 0 both sides; hard findings 0, explained 0,
UNEXPLAINED 0, payload diffs 0 (4005 paths per side). Verdict: PASSED.
