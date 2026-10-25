# Merge-path safety gate: #562 applied-updates AUTOCLEAN force

Branch `batch/2026-10-26-491-570`. Task 7 touches the merge gate
(`pretend/mod.rs`: the `updates_force` OR) and `mtimedb.rs` persistence,
but no unmerge-half files — zero changes under `ebuild_unmerge.rs`,
`pretend/removal/`, merge-tree/ordering, or `ebuild_merge/` — so no
third `INSTALL_MASK` invocation; the L1 reinstall gate below is the
required verification.

Gate: `L1_CONSUME_REINSTALL=1` over `l1-merge-gate.txt`
(`sys-libs/glibc`, `app-shells/bash`), binary frozen from this
branch's tree (`cp -p target/release/portuale /var/tmp/bedbin/portuale`,
`cmp`-verified), invocation
`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 L1_SKIP_BUILD=1
L1_CONSUME_REINSTALL=1 GENTOO_MIRRORS=http://10.48.0.229:8080
differential-test-bed/run/l1-merge-from-binpkg.sh
differential-test-bed/atomlists/l1-merge-gate.txt`
(`L1_SKIP_BUILD=1` per the #556 precedent; the `>>> Emerging (`
parity assertion stayed silent, so the pkgcache was complete).

Result (log `differential-test-bed/logs/l1-20261025T154857Z/`,
local-only): runner rc 0 — `sys-libs/glibc-2.43-r4` merge_rc 0 both
sides, `app-shells/bash-5.3_p20` merge_rc 0 both sides; hard findings
0, explained 0, UNEXPLAINED 0 (4005 files compared per side, payload
diffs 0). No movement vs the #556 gate-cell run (still 0/0/0).

Verdict: PASSED, no regressions.
