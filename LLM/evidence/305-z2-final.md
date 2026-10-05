# #305 (feat#157) — Z.2 final verification

Run 2026-10-05 on the execution host, portuale `e55583c0` plus the Z.2
test-only cfg fixes (committed with this file), pmtest
`backlog/305-vdb-backends` at `1b57f48`. Compared with the P0.2
baseline (`305-baseline.md`).

| Check | Command | Result | Baseline |
|---|---|---|---|
| fmt | `cargo fmt --check` | clean | clean |
| clippy, default | `cargo clippy --release --all-targets` | the 4 baseline warnings (portage-repo) only | 4 |
| clippy, each combination | `cargo clippy --release -p portuale -p portage-vdb --no-default-features [--features …] --all-targets` for none, `vdb-sqlite`, `vdb-redb`, `vdb-fuse`, `vdb-sqlite,vdb-redb` | no warning outside the baseline 4 (three test-only cfg gaps fixed in Z.2: below) | — |
| cargo test, default | `cargo test --release` (workspace) | 2257 passed, 0 failed (portage-vdb is new) | green, 1966 |
| pmtest suite | `python3 -m pytest pytests-contract-suite -q` (in `../pmtest`) | **2248 passed, 37 skipped, 2 xfailed** (480 s), no corpus drift | same |
| cargo test, portuale per combination | separate `CARGO_TARGET_DIR`, binary built first (the portageq / phase tests exec it) | none 782, `vdb-sqlite` 799, `vdb-redb` 793, `vdb-fuse` 790, `vdb-sqlite,vdb-redb` 807 — all passed | 746 |
| cargo test, portage-vdb per feature | `cargo test --release -p portage-vdb --features vdb-sqlite` / `vdb-redb` | 159 / 156 passed | — |
| L0 | `differential-test-bed/run/l0-resolver.sh` | `l0-20261005T105509Z`: 120 probes, parity 0.842, 8 explained, 31 unexplained, invariants 0 violations, rc 1; **unexplained and explained finding lists identical** to `l0-20261004T172739Z` (diffed line by line) | same |
| L1 build | `l1-merge-from-binpkg.sh atomlists/l1-merge-gate.txt` | `l1-20261005T110956Z`: 0 hard / 0 unexplained / 0 payload / 7 mtime-only, rc 0 | same |
| gate, files | `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 …` | `l1-20261005T114439Z`: glibc + bash merged 2/2, merge_rc 0, 0 hard / 0 unexplained, 1415 mtime-only | same |
| gate, sqlite | same + `L1_PORTUALE_VDB=sqlite` | `l1-20261005T114628Z`: merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only | — |
| gate, redb | same + `L1_PORTUALE_VDB=redb` | `l1-20261005T114827Z`: merged 2/2, 0 hard / 0 unexplained, 1415 mtime-only; redb status: nothing pending | — |

The first per-combination test run failed 5–6 portageq / ebuild-phase
tests in every combination: those tests exec `target/release/portuale`,
which a fresh `CARGO_TARGET_DIR` does not have until `cargo build` runs.
With the binary built they pass; not a product issue.

Z.2 fixes (tests only, no product change): `vdb-redb` alone did not
compile the portuale tests (`crash_before_commit` names `SqliteDb`; it,
`HookGuard`, `vdb_cli` and `payload_bytes` are used only by sqlite tests
and now carry `cfg(feature = "vdb-sqlite")`); `vdb-fuse` alone warned
about an unused `mut` and unused imports in `vdb_view.rs` tests.
