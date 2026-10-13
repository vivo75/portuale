# #339 baseline (main a0e09835 + tools commit, 2026-10-13)

- gate: fmt/clippy/build 0 warnings, cargo test 2554 passed 0 failed.
- contract-names.txt: 3757 PASSED. One test (`test_info_atom_prints_package_settings_for_a_pkg_info_package`) fails only if the ignored leak `pmtest/fixtures/var/db/pkg/dev-libs/pkginfopkg-1.0` exists; remove it before every gate.
- l0-unexplained.tsv: 31 lines from `l0-20261013T063504Z` (rc=1 is the normal state, compare line for line). fx-all: 13/13 green.
- m-cells/ (git-ignored logs; re-capture before use if missing): l32 C1/C4, l31, l1-merge, l1-porttest, l1-smoke, m1.log (`../m1-cells.sh`: #335 + #328 container oracles; the bare `scripts/335|328-*.sh` need container args, so the plan's direct call does not work).
- out-of-scope.txt: the 6 test functions that stay long.
