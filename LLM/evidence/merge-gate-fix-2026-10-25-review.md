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

## Task 4 — binpkg digest verification (digest.rs, emerge_getbinpkg.rs)

Run `l1-20261026T000922Z` (tree 13148bb5 + Task 4 working-tree change;
this runner merges with `emerge -k --getbinpkg`, so it is also the L1
getbinpkg cell the plan asks for): runner rc 0; glibc-2.43-r4 and
bash-5.3_p20 merged by both PMs; hard findings 0, UNEXPLAINED 0, payload
diffs 0 (4005 paths per side). Verdict: PASSED.

## Task 6 — global updates, vdb moves (global_updates/{mod,vdb}.rs)

Run `l1-20261026T005709Z` (tree dd518948 + Task 6 working-tree change;
the new code is not called from production yet, so this run checks that
nothing else in the merge path moved): runner rc 0; glibc-2.43-r4 and
bash-5.3_p20 merged by both PMs; hard findings 0, UNEXPLAINED 0, payload
diffs 0 (4005 paths per side). Verdict: PASSED.

## Task 8 — global updates, vdb update_ents (global_updates/vdb.rs, portage-repo updates.rs)

Run `l1-20261026T110941Z` (tree f328574d + Task 8 working-tree change):
runner rc 0; glibc-2.43-r4 and bash-5.3_p20 merged by both PMs; hard
findings 0, UNEXPLAINED 0, payload diffs 0. Verdict: PASSED.

## Task 9 — global updates, binary packages (global_updates/binpkg.rs and the shared compressor, digest and writability helpers)

Run `l1-20261026T125729Z` (tree 48c45732 + Task 9 working-tree change;
the compressor command used by `quickpkg`/`--buildpkg` now resolves
through the shared `compression_command_argv`): runner rc 0;
glibc-2.43-r4 and bash-5.3_p20 merged by both PMs; hard findings 0,
UNEXPLAINED 0, payload diffs 0 (4005 paths per side). Verdict: PASSED.

Run `l1-20261026T160916Z` (tree 6a968ff1 + Task 11 working-tree change;
global updates now run before every emerge on the target and running
roots): runner rc 0; glibc-2.43-r4 and bash-5.3_p20 merged by both PMs;
hard findings 0, UNEXPLAINED 0, payload diffs 0 (4005 paths per side),
mtime-only diffs 1420. Verdict: PASSED.
