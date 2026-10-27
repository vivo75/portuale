# Merge-path safety gate: branch fix/2026-10-26-575-residues (plan 02.589)

Recipe (every entry below): binary frozen from the branch tree
(`cp -p target/release/portuale /var/tmp/bedbin/portuale`, `cmp`-verified),
`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1 L1_SKIP_BUILD=1
L1_CONSUME_REINSTALL=1 GENTOO_MIRRORS=http://10.48.0.229:8080
differential-test-bed/run/l1-merge-from-binpkg.sh
differential-test-bed/atomlists/l1-merge-gate.txt` from pmtest. Logs are
local-only under pmtest `differential-test-bed/logs/`.

## Task 2 — mtimedb in Portage's full shape (mtimedb.rs, pretend/mod.rs)

Run `l1-20261026T232346Z` (tree 5ab1a6fc + Task 2 working-tree change):
runner rc 0; `sys-libs/glibc-2.43-r4` and `app-shells/bash-5.3_p20`
merged by both PMs (`*.merged-cpvs.txt`), merge_rc 0 both sides; hard
findings 0, UNEXPLAINED 0, payload diffs 0, mtime-only diffs 1420.
Verdict: PASSED.

## Task 3 — every hash crate on digest 0.11 (Cargo.toml ×4, hex_lower call sites)

Run `l1-20261026T233746Z` (tree 26ddf226 + Task 3 working-tree change;
`ebuild_merge/tree.rs` computes the CONTENTS MD5s with the moved `md-5`):
runner rc 0; `sys-libs/glibc-2.43-r4` and `app-shells/bash-5.3_p20`
merged by both PMs, merge_rc 0 both sides; hard findings 0, UNEXPLAINED 0,
payload diffs 0, mtime-only diffs 1420. Verdict: PASSED.

## Task 4 — RMD160 and WHIRLPOOL on binpkgs (binpkg/digest.rs, emerge_getbinpkg.rs)

Run `l1-20261026T235140Z` (tree 684c19d3 + Task 4 working-tree change;
the verifier runs before every binary merge): runner rc 0;
`sys-libs/glibc-2.43-r4` and `app-shells/bash-5.3_p20` merged by both
PMs, merge_rc 0 both sides; hard findings 0, UNEXPLAINED 0, payload
diffs 0. Verdict: PASSED.

## Task 6 — remote-binary pkg_pretend in the pass (emerge_getbinpkg.rs, pretend_pass.rs, fetch.rs)

Run `l1-20261027T012438Z` (tree 280e5c5d + Task 6 working-tree change;
this runner merges with `emerge -k --getbinpkg`, so remote binaries go
through the new pass fetch): runner rc 0; `sys-libs/glibc-2.43-r4` and
`app-shells/bash-5.3_p20` merged by both PMs, merge_rc 0 both sides;
hard findings 0, UNEXPLAINED 0, payload diffs 0. Verdict: PASSED.
